//! `cargo run -p xtask -- screenshots`: the built UI, rendered by headless Chromium against a
//! pretend backend, in both themes.
//!
//! photon's conventions forbid launching the app to verify a change, and vitest has no layout
//! engine, so the look of the UI had no check at all short of a person. This renders the real
//! bundle without the app: `ui/dist` is served from here, `screenshots/mock.js` stands in for
//! Tauri's IPC, and thumbnails are generated gradients. It shows Chromium's rendering, not
//! WebKitGTK's or WKWebView's, so it replaces no item of the README's smoke checklist; it is
//! for seeing a change before a person does.
//!
//! The pure parts - routing, the index injection, Chromium's arguments - are tested. Running
//! Chromium is not: it is not on CI's runners, and a screenshot has no assertion to make.

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

const MOCK_JS: &str = include_str!("../screenshots/mock.js");

/// The script `ui/index.html` loads first. The mock is injected ahead of it, because
/// theme-boot.js is the first thing to run and `app-theme` calls the backend at import.
const BOOT_TAG: &str = r#"<script src="/theme-boot.js">"#;

/// How long a page may take. A page that never settles is a bug in the mock, not slowness.
const PAGE_TIMEOUT: Duration = Duration::from_secs(90);

pub struct Shot {
    pub name: &'static str,
    /// `theme`, `view` and `do`, as `mock.js` reads them.
    pub query: &'static str,
    /// The desktop's colour scheme. The theme is pinned by `query` as well, so a shot does
    /// not depend on how a given Chromium maps its flags to `prefers-color-scheme`.
    pub dark: bool,
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "main-light",
        query: "theme=light&do=select",
        dark: false,
    },
    Shot {
        name: "main-dark",
        query: "theme=dark&do=select",
        dark: true,
    },
    // Whether 120 and 224 are the right widths is a looking question, and this is how it
    // is looked at without launching the app.
    Shot {
        name: "grid-small",
        query: "theme=light&tile=small",
        dark: false,
    },
    Shot {
        name: "grid-large",
        query: "theme=light&tile=large",
        dark: false,
    },
    // The bookmark filled: a query the sidebar already holds, so the button is inert.
    Shot {
        name: "saved-search-light",
        query: "theme=light&do=savedsearch",
        dark: false,
    },
    Shot {
        name: "starred-light",
        query: "theme=light&view=starred",
        dark: false,
    },
    Shot {
        name: "menu-light",
        query: "theme=light&do=menu",
        dark: false,
    },
    Shot {
        name: "menu-dark",
        query: "theme=dark&do=menu",
        dark: true,
    },
    // The viewer is dark in both themes; the light shot is the one that proves it.
    Shot {
        name: "viewer-info-light",
        query: "theme=light&do=info",
        dark: false,
    },
    Shot {
        name: "viewer-crop-dark",
        query: "theme=dark&do=crop",
        dark: true,
    },
    Shot {
        name: "band-light",
        query: "theme=light&do=band",
        dark: false,
    },
    Shot {
        name: "export-light",
        query: "theme=light&do=export",
        dark: false,
    },
    Shot {
        name: "keyword-dark",
        query: "theme=dark&do=keyword",
        dark: true,
    },
    Shot {
        name: "settings-light",
        query: "theme=light&do=settings",
        dark: false,
    },
    Shot {
        name: "settings-dark",
        query: "theme=dark&do=settings",
        dark: true,
    },
    Shot {
        name: "appearance-dark",
        query: "theme=dark&do=appearance",
        dark: true,
    },
    // The Duplicates row in the sidebar (byte-identical files and look-alikes both counted
    // in `duplicateCount`), with the info panel open on a photo that has one of each kind.
    Shot {
        name: "duplicates-light",
        query: "theme=light&view=duplicates&do=info",
        dark: false,
    },
    // Three photos compared: a 2x2 with the fourth cell empty, on the dark ground compare
    // always uses regardless of theme - the light shot is the one that proves it.
    Shot {
        name: "compare-light",
        query: "theme=light&do=compare",
        dark: false,
    },
    Shot {
        name: "compare-dark",
        query: "theme=dark&do=compare",
        dark: true,
    },
];

#[derive(Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type,
            body: body.into(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: 404,
            content_type: "text/plain",
            body: b"not found".to_vec(),
        }
    }
}

/// The path of a `GET`, without its query; `None` for anything else.
pub fn request_path(request_line: &str) -> Option<&str> {
    let mut parts = request_line.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    Some(target.split(['?', '#']).next().unwrap_or(target))
}

/// Puts the mock ahead of theme-boot.js. An index without that tag is an error rather than
/// served as it is: without the mock every command rejects, and the result is ten pictures
/// of an empty window that look like a styling bug.
pub fn inject_mock(index_html: &str) -> Result<String, String> {
    if !index_html.contains(BOOT_TAG) {
        return Err(format!(
            "ui/dist/index.html has no `{BOOT_TAG}` to load the mock before"
        ));
    }
    Ok(index_html.replacen(
        BOOT_TAG,
        &format!(r#"<script src="/mock.js"></script>{BOOT_TAG}"#),
        1,
    ))
}

/// A thumbnail or a full image: a gradient whose hue follows the item id, so neighbouring
/// tiles differ and a given photo looks the same in the grid and in the viewer.
pub fn placeholder_svg(id: u64) -> String {
    let hue = (id * 47) % 360;
    let end = (hue + 50) % 360;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="400" viewBox="0 0 600 400" preserveAspectRatio="none"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl({hue} 50% 60%)"/><stop offset="1" stop-color="hsl({end} 55% 28%)"/></linearGradient></defs><rect width="600" height="400" fill="url(#g)"/></svg>"#
    )
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

/// What the server answers for `path`. `dist` is `ui/dist`.
pub fn respond(path: &str, dist: &Path) -> Response {
    // `photon://` media, which the UI asks for over http when it thinks it is on Windows.
    if let Some(rest) = path
        .strip_prefix("/thumb/")
        .or_else(|| path.strip_prefix("/image/"))
    {
        let id = rest
            .split('/')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        return Response::ok("image/svg+xml", placeholder_svg(id));
    }
    if path == "/mock.js" {
        return Response::ok("text/javascript", MOCK_JS);
    }
    // Bound to loopback and short-lived, but a server that reads files refuses to leave its
    // root all the same.
    if path.split('/').any(|segment| segment == "..") {
        return Response::not_found();
    }
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    let file = dist.join(relative);
    let Ok(body) = std::fs::read(&file) else {
        return Response::not_found();
    };
    if relative == "index.html" {
        return match inject_mock(&String::from_utf8_lossy(&body)) {
            Ok(html) => Response::ok("text/html", html),
            Err(why) => Response {
                status: 500,
                content_type: "text/plain",
                body: why.into_bytes(),
            },
        };
    }
    Response::ok(content_type(&file), body)
}

/// Chromium's arguments for one shot.
pub fn chromium_args(shot: &Shot, port: u16, out_dir: &Path) -> Vec<String> {
    let mut args = vec![
        "--headless".to_owned(),
        "--disable-gpu".to_owned(),
        "--hide-scrollbars".to_owned(),
        "--window-size=1280,800".to_owned(),
        // Lets mock.js's timers and the app's first page of thumbnails run before the capture.
        "--virtual-time-budget=5000".to_owned(),
        // `mediaUrl` serves media from http://photon.localhost on Windows and from the
        // photon:// scheme elsewhere, which no plain browser can load. Claiming Windows and
        // mapping that host here is what gives the tiles their pictures.
        "--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) photon-screenshots".to_owned(),
        format!("--host-resolver-rules=MAP photon.localhost 127.0.0.1:{port}"),
    ];
    if shot.dark {
        args.push("--force-dark-mode".to_owned());
        args.push("--blink-settings=preferredColorScheme=0".to_owned());
    } else {
        args.push("--blink-settings=preferredColorScheme=1".to_owned());
    }
    args.push(format!(
        "--screenshot={}",
        out_dir.join(format!("{}.png", shot.name)).display()
    ));
    args.push(format!("http://127.0.0.1:{port}/?{}", shot.query));
    args
}

fn serve(stream: TcpStream, dist: &Path) {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let response = match request_path(&request_line) {
        Some(path) => respond(path, dist),
        None => Response::not_found(),
    };
    let head = format!(
        "HTTP/1.1 {} \r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    let mut stream = &stream;
    // A browser that has gone away is not this server's problem.
    let _ = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&response.body));
}

fn find_chromium() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("CHROMIUM") {
        return Some(PathBuf::from(explicit));
    }
    let names = [
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
        "google-chrome",
        "chrome",
    ];
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// `--flag <value>` from the command line.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).map(String::as_str)
}

fn capture(chromium: &Path, shot: &Shot, port: u16, out_dir: &Path) -> Result<(), String> {
    let mut child = Command::new(chromium)
        .args(chromium_args(shot, port, out_dir))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", chromium.display()))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("chromium exited with {status}")),
            Ok(None) if started.elapsed() > PAGE_TIMEOUT => {
                let _ = child.kill();
                return Err(format!("no screenshot after {}s", PAGE_TIMEOUT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("cannot wait for chromium: {e}")),
        }
    }
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let Some(chromium) = find_chromium() else {
        eprintln!("no Chromium found: install chromium or chrome, or set CHROMIUM to its path");
        return ExitCode::FAILURE;
    };
    let out_dir =
        flag(args, "--out").map_or_else(|| root.join("target").join("screenshots"), PathBuf::from);
    let only = flag(args, "--only");
    let shots: Vec<&Shot> = SHOTS
        .iter()
        .filter(|s| only.is_none_or(|name| s.name == name))
        .collect();
    if shots.is_empty() {
        let names: Vec<&str> = SHOTS.iter().map(|s| s.name).collect();
        eprintln!(
            "no shot called {:?}; expected one of: {}",
            only.unwrap_or_default(),
            names.join(", ")
        );
        return ExitCode::FAILURE;
    }

    if !args.iter().any(|a| a == "--no-build") {
        // `npm` is a .cmd shim on Windows, which `Command` does not resolve by itself.
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        let built = Command::new(npm)
            .args(["run", "build", "-w", "ui"])
            .current_dir(root)
            .status();
        if !built.is_ok_and(|status| status.success()) {
            eprintln!("`npm run build -w ui` failed");
            return ExitCode::FAILURE;
        }
    }
    let dist = root.join("ui").join("dist");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }

    // Port 0: whatever is free. Loopback only.
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("cannot listen on loopback: {e}");
            return ExitCode::FAILURE;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or_default();
    let served = dist.clone();
    // Detached: it ends with the process. A thread per connection, since Chromium opens
    // several at once and each is answered and closed.
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let dist = served.clone();
            std::thread::spawn(move || serve(stream, &dist));
        }
    });

    let mut failed = false;
    for shot in shots {
        match capture(&chromium, shot, port, &out_dir) {
            Ok(()) => println!("{}", out_dir.join(format!("{}.png", shot.name)).display()),
            Err(why) => {
                eprintln!("{}: {why}", shot.name);
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dist(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "photon-xtask-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (name, body) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    #[test]
    fn a_request_line_gives_its_path_without_the_query() {
        assert_eq!(
            request_path("GET /assets/a.js?v=1 HTTP/1.1\r\n"),
            Some("/assets/a.js")
        );
        assert_eq!(
            request_path("GET /?theme=dark&do=info HTTP/1.1\r\n"),
            Some("/")
        );
        assert_eq!(request_path("POST / HTTP/1.1\r\n"), None);
        assert_eq!(request_path(""), None);
    }

    #[test]
    fn the_mock_is_loaded_before_theme_boot() {
        let html = r#"<head><title>photon</title><script src="/theme-boot.js"></script></head>"#;
        let out = inject_mock(html).unwrap();
        let mock = out.find(r#"<script src="/mock.js"></script>"#).unwrap();
        let boot = out.find(BOOT_TAG).unwrap();
        assert!(mock < boot);
        assert_eq!(out.matches("/mock.js").count(), 1);
    }

    #[test]
    fn an_index_with_nowhere_to_put_the_mock_is_an_error() {
        assert!(inject_mock("<head><title>photon</title></head>").is_err());
    }

    #[test]
    fn media_is_a_gradient_that_follows_the_item_id() {
        let dist = temp_dist(&[]);
        let thumb = respond("/thumb/12/grid/k11", &dist);
        assert_eq!((thumb.status, thumb.content_type), (200, "image/svg+xml"));
        assert_eq!(thumb, respond("/image/12", &dist));
        assert_ne!(thumb.body, respond("/thumb/13/grid/k12", &dist).body);
    }

    #[test]
    fn the_index_is_served_with_the_mock_and_assets_as_they_are() {
        let dist = temp_dist(&[
            ("index.html", r#"<script src="/theme-boot.js"></script>"#),
            ("assets/app.css", "body{}"),
        ]);
        let index = respond("/", &dist);
        assert_eq!((index.status, index.content_type), (200, "text/html"));
        assert!(String::from_utf8(index.body).unwrap().contains("/mock.js"));
        assert_eq!(
            respond("/assets/app.css", &dist),
            Response::ok("text/css", "body{}")
        );
        assert_eq!(respond("/assets/missing.js", &dist).status, 404);
        let mock = respond("/mock.js", &dist);
        assert_eq!(mock.content_type, "text/javascript");
        assert!(
            String::from_utf8(mock.body)
                .unwrap()
                .contains("__TAURI_INTERNALS__")
        );
    }

    #[test]
    fn an_index_the_mock_cannot_be_put_into_is_a_500_not_a_blank_app() {
        let dist = temp_dist(&[("index.html", "<head></head>")]);
        assert_eq!(respond("/", &dist).status, 500);
    }

    #[test]
    fn the_server_does_not_leave_its_root() {
        let dist = temp_dist(&[("index.html", BOOT_TAG), ("inner/x.js", "1")]);
        std::fs::write(
            dist.parent().unwrap().join("photon-xtask-outside.txt"),
            "secret",
        )
        .unwrap();
        assert_eq!(respond("/inner/x.js", &dist).status, 200);
        assert_eq!(respond("/../photon-xtask-outside.txt", &dist).status, 404);
        assert_eq!(
            respond("/inner/../../photon-xtask-outside.txt", &dist).status,
            404
        );
    }

    #[test]
    fn chromium_is_told_the_scheme_the_port_and_where_to_write() {
        let out = Path::new("/tmp/shots");
        let dark = chromium_args(
            &Shot {
                name: "a",
                query: "theme=dark&do=info",
                dark: true,
            },
            4321,
            out,
        );
        let light = chromium_args(
            &Shot {
                name: "b",
                query: "theme=light",
                dark: false,
            },
            4321,
            out,
        );
        assert!(dark.contains(&"--force-dark-mode".to_owned()));
        assert!(!light.contains(&"--force-dark-mode".to_owned()));
        assert!(
            dark.contains(&"--host-resolver-rules=MAP photon.localhost 127.0.0.1:4321".to_owned())
        );
        // Joined here as the code joins it: the separator is `\` on Windows.
        assert!(dark.contains(&format!("--screenshot={}", out.join("a.png").display())));
        assert_eq!(
            dark.last().unwrap(),
            "http://127.0.0.1:4321/?theme=dark&do=info"
        );
        // Without a Windows user agent the UI asks for photon:// URLs, which nothing serves.
        assert!(
            light
                .iter()
                .any(|a| a.starts_with("--user-agent=") && a.contains("Windows"))
        );
    }

    #[test]
    fn shot_names_are_unique_and_safe_as_file_names() {
        let mut names: Vec<&str> = SHOTS.iter().map(|s| s.name).collect();
        assert!(
            names
                .iter()
                .all(|n| n.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
        );
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SHOTS.len());
    }

    /// The names inside `const <name> = <open> … <close>` in mock.js: object keys at four
    /// spaces' indent, or quoted strings.
    fn mock_list(name: &str, open: char, close: &str) -> Vec<String> {
        let start = MOCK_JS
            .find(&format!("const {name} = {open}"))
            .expect("list in mock.js");
        let body = &MOCK_JS[start..];
        let body = &body[..body.find(close).expect("end of list")];
        if open == '{' {
            body.lines()
                .filter_map(|l| l.strip_prefix("    ")?.split_once(": ").map(|(k, _)| k))
                .filter(|k| k.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
                .map(str::to_owned)
                .collect()
        } else {
            body.split('\'')
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
                .collect()
        }
    }

    #[test]
    fn the_mock_knows_every_command_the_ui_can_send() {
        let api = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/api.ts"),
        )
        .unwrap();
        let sent: Vec<&str> = api
            .split("invoke<")
            .skip(1)
            .filter_map(|rest| rest.split_once(">('")?.1.split('\'').next())
            .collect();
        assert!(sent.len() > 30, "api.ts was not parsed: {sent:?}");

        let canned = mock_list("canned", '{', "\n  };");
        let silent = mock_list("SILENT", '[', "];");
        assert!(
            canned.contains(&"grid_rows".to_owned()) && silent.contains(&"set_star".to_owned())
        );
        let unknown: Vec<&&str> = sent
            .iter()
            .filter(|c| !canned.iter().chain(&silent).any(|k| k == *c))
            .collect();
        // A command the mock does not answer resolves to null: a screenshot of whatever the
        // UI makes of that, which reads as a styling bug. Give it an answer in `canned`, or
        // list it in SILENT if nothing draws its result.
        assert!(unknown.is_empty(), "mock.js has no entry for {unknown:?}");
    }
}
