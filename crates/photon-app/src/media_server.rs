//! Serves videos to the webview over loopback HTTP.
//!
//! Not `photon://`: WebKitGTK hands a media URL to GStreamer, whose WebKit source takes only
//! http(s) and blob URLs, so `<video src="photon://…">` fails before a pipeline exists
//! (spike, spec `2026-09-26-photon-video-design.md`). Plain HTTP with ranges is what every
//! webview plays best, so all three platforms use this one path.
//!
//! A loopback port is reachable by anything on the machine, and by a page in the user's
//! browser through DNS rebinding. Hence the per-launch token, compared in constant time; the
//! `Host` check, which is what defeats rebinding (a rebound page sends its own host name);
//! the exact origin in `Access-Control-Allow-Origin`; and one route that serves video rows
//! by id and never turns any part of a URL into a path.
//!
//! Every request is answered on a thread of its own, not by a fixed pool. tiny_http writes a
//! response body synchronously on the thread that answers it, with no write timeout and no
//! access to the socket to set one, so a playing, paused or not-yet-collected `<video>` holds
//! that thread for as long as its connection lives. A pool of any fixed size is exhausted by
//! that many such videos, and then the thumbnailer's next job and the viewer's next seek wait
//! forever. tiny_http already spends a thread per connection reading requests, so one more
//! per request in flight is the same kind of cost.

use crate::engine::Engine;
use photon_core::media::MediaKind;
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    panic::AssertUnwindSafe,
    path::Path,
    sync::Arc,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub struct MediaServer {
    pub(crate) port: u16,
    pub(crate) token: String,
    // Held so the listener lives as long as the app's managed state. Dropping it would stop
    // the accept thread; the dispatcher blocked on it keeps its own `Arc` regardless.
    _server: Arc<Server>,
}

/// The page's own origin, which is the only one allowed to read a response - and the one
/// that keeps a canvas the frame was drawn on readable.
///
/// Deliberately exact, so `npm run dev` - whose page is `http://localhost:1420` - can neither
/// play nor thumbnail a video: widening this for the dev server would widen it for anything
/// else that can serve a page from that origin.
pub(crate) fn app_origin() -> &'static str {
    if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

impl MediaServer {
    pub fn start(engine: Arc<Engine>) -> std::io::Result<Self> {
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(std::io::Error::other)?);
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| std::io::Error::other("the media server is not on an IP socket"))?;
        let mut raw = [0u8; 16];
        // `getrandom::Error` is a `std` error only with its `std` feature; its message is enough.
        getrandom::fill(&mut raw).map_err(|e| std::io::Error::other(e.to_string()))?;
        let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let host = format!("127.0.0.1:{port}");
        let dispatcher = server.clone();
        let dispatch_token = token.clone();
        std::thread::Builder::new()
            .name("photon-media".to_owned())
            .spawn(move || {
                // Not `incoming_requests()`: that iterator ends on the first `Err` from `recv`,
                // which would stop the server for good.
                loop {
                    match dispatcher.recv() {
                        Ok(request) => {
                            let (engine, token, host) =
                                (engine.clone(), dispatch_token.clone(), host.clone());
                            // A thread per request; the module doc says why. A panic costs
                            // only its own request - dropped while unwinding, it is answered
                            // 500 by tiny_http's own `Drop` - with or without `catch_unwind`,
                            // which stays so that remains true if this closure ever does more
                            // after `serve`.
                            let spawned = std::thread::Builder::new()
                                .name("photon-media-request".to_owned())
                                .spawn(move || {
                                    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
                                        serve(&engine, &token, &host, request)
                                    }));
                                });
                            // Out of threads: the request went down with the closure and was
                            // answered 500 by its `Drop`. The next one may fare better.
                            if let Err(err) = spawned {
                                tracing::warn!(%err, "could not start a media request thread");
                            }
                        }
                        // The only error `recv` returns is the accept thread's last words: it
                        // stops listening after one failed `accept`. Nothing more will arrive,
                        // but this thread has nothing better to do than block here, and the
                        // app goes on without video.
                        Err(err) => tracing::error!(%err, "the media server stopped accepting"),
                    }
                }
            })?;
        Ok(Self {
            port,
            token,
            _server: server,
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/{}", self.port, self.token)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RangeAnswer {
    Whole,
    /// Inclusive, as HTTP writes it.
    Partial(u64, u64),
    Unsatisfiable,
}

/// One `bytes=` range. A multi-range, an unknown unit or a malformed value is answered with
/// the whole file, which HTTP permits and every player accepts. An open-ended range runs to
/// the end of the file: capped, it broke every far seek in WebKitGTK (the spike's
/// `downloadbuffer` offset mismatch).
pub(crate) fn parse_range(header: Option<&str>, len: u64) -> RangeAnswer {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return RangeAnswer::Whole;
    };
    if spec.contains(',') {
        return RangeAnswer::Whole;
    }
    let Some((a, b)) = spec.split_once('-') else {
        return RangeAnswer::Whole;
    };
    let (a, b) = (a.trim(), b.trim());
    let parse = |s: &str| s.parse::<u64>().ok();
    match (a.is_empty(), b.is_empty()) {
        // The suffix form: the last `n` bytes. Nothing to take the last bytes of in an empty
        // file, and `len - 1` would underflow if it got that far.
        (true, false) => match parse(b) {
            Some(0) => RangeAnswer::Unsatisfiable,
            Some(_) if len == 0 => RangeAnswer::Unsatisfiable,
            Some(n) => RangeAnswer::Partial(len.saturating_sub(n), len - 1),
            None => RangeAnswer::Whole,
        },
        (false, _) => {
            let end = if b.is_empty() {
                Some(u64::MAX)
            } else {
                parse(b)
            };
            match (parse(a), end) {
                (Some(start), Some(end)) if start > end => RangeAnswer::Unsatisfiable,
                // Also what keeps `len - 1` below from underflowing: `start >= 0 == len`.
                (Some(start), Some(_)) if start >= len => RangeAnswer::Unsatisfiable,
                (Some(start), Some(end)) => RangeAnswer::Partial(start, end.min(len - 1)),
                _ => RangeAnswer::Whole,
            }
        }
        (true, true) => RangeAnswer::Whole,
    }
}

/// Whether a presented token is ours, taking the same time whichever byte differs, so the
/// answer's timing cannot be used to guess the token a byte at a time. The length is not
/// secret: every token is 32 hex digits.
fn eq_constant_time(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && std::hint::black_box(
            a.bytes()
                .zip(b.bytes())
                .fold(0u8, |acc, (x, y)| acc | (x ^ y)),
        ) == 0
}

fn header(name: &str, value: &str) -> Header {
    // Every name and value passed here is ASCII built in this file, which is all
    // `from_bytes` checks.
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("ASCII header")
}

/// Every refusal looks the same - a wrong token, a wrong host, a photo, a row that does not
/// exist - so the server confirms nothing about what it holds.
fn not_found(request: Request) {
    let _ = request
        .respond(Response::empty(StatusCode(404)).with_header(header("Cache-Control", "no-store")));
}

/// The request's single `Host` header, or `None` for none or several: a request carrying
/// two has no one host to check.
fn the_host(request: &Request) -> Option<&str> {
    let mut hosts = request.headers().iter().filter(|h| h.field.equiv("Host"));
    match (hosts.next(), hosts.next()) {
        (Some(h), None) => Some(h.value.as_str()),
        _ => None,
    }
}

fn serve(engine: &Engine, token: &str, host: &str, request: Request) {
    if the_host(&request) != Some(host) {
        return not_found(request);
    }
    if !matches!(request.method(), Method::Get | Method::Head) {
        return not_found(request);
    }
    let id = match request
        .url()
        .strip_prefix('/')
        .map(|rest| rest.split('/').collect::<Vec<_>>())
        .as_deref()
    {
        Some([t, "video", id]) if eq_constant_time(t, token) => id.parse::<i64>().ok(),
        _ => None,
    };
    let Some(item) = id.and_then(|id| engine.lib.item(id).ok().flatten()) else {
        return not_found(request);
    };
    // Hidden videos are served: the Hidden view plays them.
    if item.kind != MediaKind::Video || item.missing_since.is_some() {
        return not_found(request);
    }
    // Read-only, like every open of a watched file.
    let Ok(mut file) = File::open(&item.path) else {
        return not_found(request);
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return not_found(request);
    };
    let range = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_owned());
    let mut headers = vec![
        header("Content-Type", mime_for(Path::new(&item.path))),
        header("Accept-Ranges", "bytes"),
        header("Access-Control-Allow-Origin", app_origin()),
        header("Cache-Control", "no-store"),
    ];
    let (status, start, count) = match parse_range(range.as_deref(), len) {
        RangeAnswer::Whole => (200, 0, len),
        RangeAnswer::Partial(start, end) => {
            headers.push(header(
                "Content-Range",
                &format!("bytes {start}-{end}/{len}"),
            ));
            (206, start, end - start + 1)
        }
        RangeAnswer::Unsatisfiable => {
            headers.push(header("Content-Range", &format!("bytes */{len}")));
            let _ = request.respond(Response::new(
                StatusCode(416),
                headers,
                std::io::empty(),
                Some(0),
                None,
            ));
            return;
        }
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return not_found(request);
    }
    // Streamed from the file in 64 KB reads, never built in memory. tiny_http sends no body
    // for a HEAD itself, and never reads the file then. The threshold is lifted because
    // above it (32 KB by default) tiny_http switches to a chunked body and drops
    // `Content-Length`, which a HEAD then cannot answer and a player sizing its buffer
    // wants. That holds unless the client itself asks for chunks with a `TE` header, which
    // tiny_http honours first; players do not send one. An error here is the player hanging
    // up after a seek - the normal end of most requests, not something to log.
    let response = Response::new(
        StatusCode(status),
        headers,
        BufReader::with_capacity(64 * 1024, file).take(count),
        usize::try_from(count).ok(),
        None,
    )
    .with_chunked_threshold(usize::MAX);
    let _ = request.respond(response);
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp4" | "m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, jpeg};
    use std::io::{Read, Write};
    use std::net::TcpStream;

    #[test]
    fn ranges_as_http_reads_them() {
        use RangeAnswer::*;
        assert_eq!(parse_range(None, 100), Whole);
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Partial(10, 19));
        assert_eq!(
            parse_range(Some("bytes=10-"), 100),
            Partial(10, 99),
            "open-ended runs to the end, uncapped"
        );
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Partial(90, 99));
        assert_eq!(parse_range(Some("bytes=-30"), 100), Partial(70, 99));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Partial(0, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=20-10"), 100), Unsatisfiable);
        assert_eq!(
            parse_range(Some("bytes=0-1,5-9"), 100),
            Whole,
            "multi-range: the whole file is allowed"
        );
        assert_eq!(
            parse_range(Some("pages=1-2"), 100),
            Whole,
            "an unknown unit is ignored"
        );
        assert_eq!(
            parse_range(Some("bytes=a-b"), 100),
            Whole,
            "a malformed range is ignored"
        );
    }

    #[test]
    fn a_range_on_an_empty_file_is_unsatisfiable() {
        assert_eq!(parse_range(Some("bytes=0-"), 0), RangeAnswer::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-1"), 0), RangeAnswer::Unsatisfiable);
        assert_eq!(parse_range(None, 0), RangeAnswer::Whole);
    }

    /// A raw HTTP/1.1 exchange, so the test sees exactly what a player would. `request` is
    /// everything before the blank line that ends the header.
    fn exchange(port: u16, request: &str) -> (u16, String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        // A pool with no worker left accepts the connection and never answers: fail then,
        // rather than hang the suite.
        s.set_read_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        write!(s, "{request}\r\n").unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or_else(|| panic!("no header in {:?}", String::from_utf8_lossy(&raw)));
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status = head[9..12].parse().unwrap();
        (status, head, raw[split + 4..].to_vec())
    }

    fn request(method: &str, port: u16, path: &str, extra: &str) -> (u16, String, Vec<u8>) {
        exchange(
            port,
            &format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n"
            ),
        )
    }

    fn get(port: u16, path: &str, extra: &str) -> (u16, String, Vec<u8>) {
        request("GET", port, path, extra)
    }

    fn server_with(bytes: &[u8]) -> (crate::testutil::Fixture, MediaServer, i64) {
        let f = fixture(&[("clip.mp4", bytes), ("a.jpg", &jpeg(8, 8))]);
        f.add_photos();
        let id = f
            .ids()
            .into_iter()
            .find(|&id| f.engine.lib.item(id).unwrap().unwrap().kind == MediaKind::Video)
            .unwrap();
        let server = MediaServer::start(f.engine.clone()).unwrap();
        (f, server, id)
    }

    #[test]
    fn serves_a_video_whole_and_in_ranges() {
        let bytes: Vec<u8> = (0..3_000_000u32).map(|i| i as u8).collect();
        let (_f, server, id) = server_with(&bytes);
        assert_eq!(
            server.base_url(),
            format!("http://127.0.0.1:{}/{}", server.port, server.token)
        );
        let path = format!("/{}/video/{id}", server.token);
        let (status, head, body) = get(server.port, &path, "");
        assert_eq!(status, 200);
        assert_eq!(body, bytes);
        assert!(head.contains("Accept-Ranges: bytes"), "{head}");
        assert!(head.contains("Content-Type: video/mp4"), "{head}");
        assert!(head.contains("Cache-Control: no-store"), "{head}");
        assert!(
            head.contains(&format!("Access-Control-Allow-Origin: {}", app_origin())),
            "{head}"
        );
        // Open-ended: answered to the end of a file larger than any chunk size. This is the
        // spike's bug - a 1 MB cap broke every far seek.
        let (status, head, body) = get(server.port, &path, "Range: bytes=1000-\r\n");
        assert_eq!(status, 206);
        assert!(
            head.contains(&format!(
                "Content-Range: bytes 1000-{}/{}",
                bytes.len() - 1,
                bytes.len()
            )),
            "{head}"
        );
        assert!(
            head.contains(&format!("Content-Length: {}", bytes.len() - 1000)),
            "a player needs the length, not a chunked body: {head}"
        );
        assert_eq!(body, bytes[1000..]);
        let (status, head, _) = get(server.port, &path, "Range: bytes=9999999-\r\n");
        assert_eq!(status, 416);
        assert!(
            head.contains(&format!("Content-Range: bytes */{}", bytes.len())),
            "{head}"
        );
    }

    #[test]
    fn head_answers_the_length_with_no_body() {
        let bytes = vec![3u8; 100_000];
        let (_f, server, id) = server_with(&bytes);
        let path = format!("/{}/video/{id}", server.token);
        let (status, head, body) = request("HEAD", server.port, &path, "");
        assert_eq!((status, body.len()), (200, 0), "{head}");
        assert!(head.contains("Content-Length: 100000"), "{head}");
        let (status, head, body) = request("HEAD", server.port, &path, "Range: bytes=10-\r\n");
        assert_eq!((status, body.len()), (206, 0), "{head}");
        assert!(head.contains("Content-Length: 99990"), "{head}");
        assert!(
            head.contains("Content-Range: bytes 10-99999/100000"),
            "{head}"
        );
    }

    #[test]
    fn refuses_what_is_not_this_apps_video() {
        let (f, server, id) = server_with(b"video bytes");
        let photo = f.ids().into_iter().find(|&i| i != id).unwrap();
        let port = server.port;
        let token = &server.token;
        for path in [
            format!("/{}/video/{id}", "0".repeat(32)), // wrong token
            format!("/{}/video/{id}", &token[..31]),   // a prefix of the token
            format!("/{token}/video/{photo}"),         // a photo
            format!("/{token}/video/999999"),          // no such row
            format!("/{token}/../../etc/passwd"),      // not the route
            format!("/{token}/video/{id}/x"),          // not the route either
            format!("/{token}/video/{id}?x=1"),        // nor this
            format!("/{token}"),
            "/".to_owned(),
        ] {
            assert_eq!(get(port, &path, "").0, 404, "{path}");
        }
        let path = format!("/{token}/video/{id}");
        assert_eq!(get(port, &path, "").0, 200, "the route itself is served");
        assert_eq!(request("POST", port, &path, "").0, 404, "only GET and HEAD");
        assert_eq!(
            request("OPTIONS", port, &path, "").0,
            404,
            "only GET and HEAD"
        );
        // Right token, wrong Host: what a DNS-rebound web page sends.
        for host in [
            format!("Host: evil.example:{port}\r\n"),
            format!("Host: localhost:{port}\r\n"),
            "Host: 127.0.0.1\r\n".to_owned(),
            format!("Host: 127.0.0.1:{port}0\r\n"),
            String::new(),
            format!("Host: 127.0.0.1:{port}\r\nHost: evil.example:{port}\r\n"),
        ] {
            let (status, head, _) = exchange(
                port,
                &format!("GET {path} HTTP/1.1\r\n{host}Connection: close\r\n"),
            );
            assert_eq!(status, 404, "{host:?}: {head}");
        }
    }

    #[test]
    fn a_video_no_longer_on_disk_is_not_served() {
        let (f, server, id) = server_with(b"video bytes");
        let path = format!("/{}/video/{id}", server.token);
        assert_eq!(get(server.port, &path, "").0, 200);
        f.engine.lib.mark_missing(&[id], 1).unwrap();
        assert_eq!(get(server.port, &path, "").0, 404);
    }

    #[test]
    fn malformed_requests_leave_the_server_serving() {
        let (_f, server, id) = server_with(b"video bytes");
        let port = server.port;
        let path = format!("/{}/video/{id}", server.token);
        // Several rounds of each: a server that lost anything on one of them - a thread, a
        // lock - would stop answering before the last.
        for _ in 0..8 {
            for junk in [
                "NONSENSE\r\n".to_owned(),
                format!("GET {path} HTTP/1.1\r\nX-\u{e9}: 1\r\n"),
                format!("GET {path} HTTP/1.1\r\nConnection: close\r\n"),
                format!("BREW {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n"),
                format!(
                    "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nRange: bytes=-99999999999999999999999\r\nConnection: close\r\n"
                ),
                format!(
                    "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Big: {}\r\nConnection: close\r\n",
                    "a".repeat(200_000)
                ),
            ] {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                s.set_read_timeout(Some(std::time::Duration::from_secs(20)))
                    .unwrap();
                let _ = write!(s, "{junk}\r\n");
                // Every one of them is answered - by tiny_http or by `serve` - and closed.
                let mut sink = Vec::new();
                s.read_to_end(&mut sink)
                    .unwrap_or_else(|err| panic!("no answer to {junk:.60?}: {err}"));
            }
        }
        assert_eq!(get(port, &path, "").0, 200);
    }

    #[test]
    fn a_client_hanging_up_mid_body_leaves_the_server_serving() {
        let bytes = vec![7u8; 8_000_000];
        let (_f, server, id) = server_with(&bytes);
        let path = format!("/{}/video/{id}", server.token);
        // Several hang-ups in a row: each must end its request cleanly.
        for _ in 0..5 {
            let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
            write!(
                s,
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                server.port
            )
            .unwrap();
            let mut first = [0u8; 1024];
            s.read_exact(&mut first).unwrap();
            drop(s); // the player seeked elsewhere
        }
        let (status, _, body) = get(server.port, &path, "Range: bytes=0-9\r\n");
        assert_eq!((status, body.len()), (206, 10));
    }

    #[test]
    fn players_holding_connections_open_do_not_starve_the_next_request() {
        // Larger than the loopback socket buffers on both ends, so a response nobody reads
        // blocks its writer mid-body, which is what a paused `<video>` does.
        let bytes = vec![5u8; 48_000_000];
        let (_f, server, id) = server_with(&bytes);
        let path = format!("/{}/video/{id}", server.token);
        let held: Vec<TcpStream> = (0..10)
            .map(|_| {
                let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
                write!(
                    s,
                    "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                    server.port
                )
                .unwrap();
                s
            })
            .collect();
        // Give every held request time to reach its handler and fill its socket.
        std::thread::sleep(std::time::Duration::from_millis(500));
        let started = std::time::Instant::now();
        let (status, _, body) = get(server.port, &path, "Range: bytes=0-9\r\n");
        assert_eq!((status, body.len()), (206, 10));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "answered only after {:?}",
            started.elapsed()
        );
        drop(held);
    }

    #[test]
    fn tokens_compare_whole() {
        assert!(eq_constant_time("abcd", "abcd"));
        assert!(!eq_constant_time("abcd", "abce"));
        assert!(!eq_constant_time("abcd", "abc"));
        assert!(!eq_constant_time("", "a"));
    }
}
