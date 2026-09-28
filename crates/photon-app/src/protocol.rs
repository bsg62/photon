//! The `photon://` URI scheme: thumbnails and original images for the webview.
//!
//! - `/thumb/<id>/<grid|preview>/<thumbKey>`: WebP, built on demand, cached forever
//!   (`thumbKey` changes when the file or its edit does). A thumbnail already cached under
//!   the key is served from the key alone; the id is looked up only to build one.
//! - `/image/<id>`: the original file - or, for a photo with an edit, the edited picture
//!   rendered on the fly (`photon_core::edit::render_full`). The file itself is never what
//!   an edited photo shows.
//! - `/image/<id>/uncropped`: the same without the crop, which is what the crop tool draws
//!   its rectangle on.
//!
//! Both image routes are `no-cache` with an `ETag` naming the picture they serve, so the
//! webview revalidates rather than refetches: a photo revisited, or reached after its
//! neighbour preload, is a 304 with no file read and no render.

use crate::engine::Engine;
use photon_core::{Error, thumbs::ThumbSize};
use std::{future::Future, path::Path, sync::Arc, time::Duration};
use tauri::http::{Response, StatusCode, header};

pub const THUMB_TIMEOUT: Duration = Duration::from_secs(30);

/// Answers one request, on the async runtime. `if_none_match` is the request's
/// `If-None-Match` header, if it sent one; only the image routes read it (a thumbnail is
/// `immutable` and never revalidated).
///
/// Disk work - a file read, a render - runs on the blocking pool, for as long as it takes
/// and no longer. Waiting for a thumbnail that is not built yet does not: it is awaited
/// here (`ThumbService::request_async`), holding no thread. Run whole inside
/// `spawn_blocking`, every such wait held one of Tokio's 512 blocking threads for up to
/// `THUMB_TIMEOUT`, and once a fast scroll had parked that many, cached thumbnails,
/// full-size images and every blocking IPC command queued behind waits that were only
/// going to time out.
pub async fn handle(
    engine: Arc<Engine>,
    path: String,
    if_none_match: Option<String>,
) -> Response<Vec<u8>> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let cropped = match parts.as_slice() {
        ["thumb", id, size, rest @ ..] => {
            return thumb(engine, id, size, rest.first().copied()).await;
        }
        ["image", _] => true,
        ["image", _, "uncropped"] => false,
        _ => return text(StatusCode::NOT_FOUND, "not found"),
    };
    let id = parts[1].to_owned();
    off_thread(move || image(&engine, &id, cropped, if_none_match.as_deref()))
        .await
        .unwrap_or_else(|response| *response)
}

/// `respond`'s response, or a 500 if it panics - awaited as a task of its own, so its
/// panic ends in the `JoinError` read here rather than in the task that holds the
/// responder.
///
/// The scheme handler hands each request a responder that must be called exactly once.
/// Awaited in the same task as that call, a panic anywhere in `handle` - outside
/// `off_thread`, which already turns a panic in disk work into a 500 - unwound past it:
/// the request was never answered, and the `<img>` waited with neither `onload` nor
/// `onerror`, so a tile stayed blank with nothing to retry it.
pub async fn answered(
    respond: impl Future<Output = Response<Vec<u8>>> + Send + 'static,
) -> Response<Vec<u8>> {
    match tokio::spawn(respond).await {
        Ok(response) => response,
        Err(err) => {
            tracing::error!(%err, "a photon:// request handler failed");
            text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string())
        }
    }
}

/// Runs `work` on the blocking pool of the runtime serving the request - Tauri's, in the
/// app. A panic in it is a 500, as it would be anywhere else.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Box<Response<Vec<u8>>>> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| Box::new(text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string())))
}

async fn thumb(
    engine: Arc<Engine>,
    id: &str,
    size: &str,
    url_key: Option<&str>,
) -> Response<Vec<u8>> {
    let Ok(id) = id.parse::<i64>() else {
        return text(StatusCode::BAD_REQUEST, "bad id");
    };
    let size = match size {
        "grid" => ThumbSize::Grid,
        "preview" => ThumbSize::Preview,
        _ => return text(StatusCode::BAD_REQUEST, "bad size"),
    };
    let url_key = url_key.and_then(parse_key);
    // The key names the picture, so a thumbnail already cached under it is the answer
    // without asking the database anything - this is every tile of a scroll through a
    // library whose thumbnails exist. A read that fails (not built yet, or collected since)
    // falls through to the item's own lookup below.
    if let Some(key) = url_key {
        let cached = engine.thumbs.path_for(key, size);
        match off_thread(move || std::fs::read(cached)).await {
            Ok(Ok(bytes)) => return ok(bytes, "image/webp", FOREVER),
            Ok(Err(_)) => {}
            Err(response) => return *response,
        }
    }
    // Dropping the request at the deadline is what gives up the wait: `ThumbQueue::wait`
    // lifts its `Neighbour` floor as it goes.
    let requested = tokio::time::timeout(THUMB_TIMEOUT, engine.thumbs.request_async(id, size))
        .await
        .unwrap_or(Err(Error::ThumbTimeout(id)));
    match requested {
        Ok(file) => match off_thread({
            let file = file.clone();
            move || std::fs::read(file)
        })
        .await
        {
            Err(response) => *response,
            Ok(Ok(bytes)) => {
                // `request` serves the photo's *current* thumbnail whatever key was asked
                // for, and keys can recur - "Original", or a fourth quarter turn, returns a
                // photo to a key it has had before. Only a file that is the URL key's own may
                // be kept: a request still carrying the original's key while the row holds an
                // edit would otherwise pin the edited picture under the original's URL for a
                // year.
                let own = url_key.is_some_and(|key| engine.thumbs.path_for(key, size) == file);
                ok(bytes, "image/webp", if own { FOREVER } else { "no-store" })
            }
            Ok(Err(err)) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        },
        Err(Error::NotFound(_)) => text(StatusCode::NOT_FOUND, "not found"),
        Err(Error::ThumbFailed(message)) => text(StatusCode::UNPROCESSABLE_ENTITY, &message),
        Err(Error::ThumbTimeout(_) | Error::ThumbUnavailable(_)) => {
            text(StatusCode::SERVICE_UNAVAILABLE, "thumbnail not ready")
        }
        Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

/// How long the webview may keep a thumbnail served under its own key: forever, since that
/// is what the key is for - it changes whenever the picture does.
const FOREVER: &str = "public, max-age=31536000, immutable";

/// The URL's thumbnail key, only in the exact spelling `hex_key` gives it. A looser parse
/// would read `+1` as key 1 and cache key 1's picture under a URL the UI never asks for.
pub(crate) fn parse_key(key: &str) -> Option<u64> {
    u64::from_str_radix(key, 16)
        .ok()
        .filter(|&parsed| photon_core::grid::hex_key(parsed) == key)
}

/// One full-size render at a time. A render holds a whole decoded photo (about 100 MB at its
/// peak for 24 MP) on a protocol thread, outside the thumbnail pool whose `MAX_WORKERS` is what
/// bounds decode memory; flicking through a run of edited photos would otherwise start one
/// per photo passed.
///
/// An export takes the same lock: a full-size render is a full-size render whoever asked
/// for it, and an export of a hundred edited photos beside a viewer flicking through them
/// would otherwise be two at once.
pub(crate) static RENDERING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

fn image(
    engine: &Engine,
    id: &str,
    cropped: bool,
    if_none_match: Option<&str>,
) -> Response<Vec<u8>> {
    let Ok(id) = id.parse::<i64>() else {
        return text(StatusCode::BAD_REQUEST, "bad id");
    };
    let item = match engine.lib.item(id) {
        Ok(Some(item)) if item.missing_since.is_none() => item,
        Ok(_) => return text(StatusCode::NOT_FOUND, "not found"),
        Err(err) => return text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    // A video is served by `media_server.rs`, streamed; this handler reads the whole file
    // into memory, which for a video is gigabytes.
    if item.kind != photon_core::media::MediaKind::Image {
        return text(StatusCode::NOT_FOUND, "not found");
    }
    let edit = if cropped {
        item.edit
    } else {
        item.edit.without_crop()
    };
    // Checked before the file is opened or `RENDERING` taken: sparing both is the point.
    let etag = image_etag(&item, edit, cropped);
    if if_none_match.is_some_and(|header| etag_matches(header, &etag)) {
        return not_modified(&etag);
    }
    if !edit.is_identity() {
        // `no-cache`, like the original: the URL does not change with the edit, so the
        // webview must ask again. The UI adds the thumbnail key as a query for the same
        // reason - an `<img>` given the URL it already has does not refetch at all.
        let _one_at_a_time = RENDERING.lock();
        return match photon_core::edit::render_full(
            Path::new(&item.path),
            item.orientation,
            edit,
            photon_core::edit::FULL_QUALITY,
            photon_core::edit::Chroma::Half,
        ) {
            Ok((bytes, mime)) => with_etag(ok(bytes, mime, "no-cache"), &etag),
            Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                text(StatusCode::NOT_FOUND, "not found")
            }
            Err(Error::Io(err)) => text(StatusCode::SERVICE_UNAVAILABLE, &err.to_string()),
            Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        };
    }
    match std::fs::read(&item.path) {
        Ok(bytes) => with_etag(
            ok(bytes, mime_for(Path::new(&item.path)), "no-cache"),
            &etag,
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            text(StatusCode::NOT_FOUND, "not found")
        }
        Err(err) => text(StatusCode::SERVICE_UNAVAILABLE, &err.to_string()),
    }
}

/// The validator of the picture an image route serves: the thumbnail key of `edit` over the
/// file's fingerprint - `item.thumb_key()` itself for `/image`, the uncropped edit's key for
/// `/uncropped`. A key changes whenever the file or the edit does, so it is exactly "the
/// same picture".
///
/// `/uncropped` is marked apart. Its key is the one `/image` carries for the same photo
/// once the crop is removed, and a photo with no crop has one key for both; the webview
/// only revalidates a URL with that URL's own tag, but a tag that could name a picture
/// served under the other route is one cache-keying change away from serving it there.
fn image_etag(
    item: &photon_core::library::Item,
    edit: photon_core::edit::Edit,
    cropped: bool,
) -> String {
    let key = edit.thumb_key(photon_core::media::fingerprint(
        &item.path,
        item.size,
        item.mtime_ms,
    ));
    let hex = photon_core::grid::hex_key(key);
    if cropped {
        format!("\"{hex}\"")
    } else {
        format!("\"{hex}-uncropped\"")
    }
}

/// Whether an `If-None-Match` header names `etag`: a comma-separated list of tags, each
/// possibly weak (`W/`, which the weak comparison `If-None-Match` uses ignores), or `*`.
fn etag_matches(header: &str, etag: &str) -> bool {
    header
        .split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag)
}

fn not_modified(etag: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Vec::new())
        .expect("static response parts are valid")
}

fn with_etag(mut response: Response<Vec<u8>>, etag: &str) -> Response<Vec<u8>> {
    response.headers_mut().insert(
        header::ETAG,
        etag.parse()
            .expect("a quoted hex key is a valid header value"),
    );
    response
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg" | "jpe") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
}

fn ok(body: Vec<u8>, content_type: &str, cache: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .expect("static response parts are valid")
}

fn text(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(message.as_bytes().to_vec())
        .expect("static response parts are valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, jpeg};
    use photon_core::media::MediaKind;

    /// One request, as the webview makes it, from a plain thread.
    fn call(engine: &Arc<Engine>, path: &str, if_none_match: Option<&str>) -> Response<Vec<u8>> {
        tauri::async_runtime::block_on(handle(
            engine.clone(),
            path.to_owned(),
            if_none_match.map(str::to_owned),
        ))
    }

    /// A handler that panics is still answered: the responder is called with a 500 rather
    /// than dropped, which left the webview's request pending forever.
    #[test]
    fn a_panicking_handler_is_answered_with_a_500() {
        let response = tauri::async_runtime::block_on(answered(async {
            panic!("a handler bug");
        }));
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let response = tauri::async_runtime::block_on(answered(async {
            text(StatusCode::NOT_FOUND, "not found")
        }));
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "a response passes through"
        );
    }

    fn get(engine: &Arc<Engine>, path: &str) -> Response<Vec<u8>> {
        call(engine, path, None)
    }

    fn header<'a>(r: &'a Response<Vec<u8>>, name: &str) -> &'a str {
        r.headers().get(name).unwrap().to_str().unwrap()
    }

    #[test]
    fn serves_thumbnails_with_immutable_caching() {
        let img = jpeg(64, 32);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let key = crate::commands::viewer_item(&f.engine, id)
            .unwrap()
            .thumb_key;
        let r = get(&f.engine, &format!("/thumb/{id}/grid/{key}"));
        assert_eq!(r.status(), 200);
        assert_eq!(header(&r, "content-type"), "image/webp");
        assert!(header(&r, "cache-control").contains("immutable"));
        assert!(!r.body().is_empty());
        assert_eq!(
            get(&f.engine, &format!("/thumb/{id}/preview/x")).status(),
            200
        );
    }

    #[test]
    fn serves_originals_with_their_mime_type() {
        let img = jpeg(64, 32);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let r = get(&f.engine, &format!("/image/{}", f.ids()[0]));
        assert_eq!(r.status(), 200);
        assert_eq!(header(&r, "content-type"), "image/jpeg");
        assert_eq!(r.body(), &img);
    }

    /// The unedited AVIF is served as itself: the webview decodes it natively where it can,
    /// and the viewer keeps the preview where it cannot.
    #[test]
    fn serves_avif_as_avif() {
        assert_eq!(mime_for(Path::new("a.avif")), "image/avif");
        assert_eq!(mime_for(Path::new("B.AVIF")), "image/avif");
    }

    fn dims(r: &Response<Vec<u8>>) -> (u32, u32) {
        let picture = image::load_from_memory(r.body()).unwrap();
        (picture.width(), picture.height())
    }

    fn current_key(f: &crate::testutil::Fixture, id: i64) -> String {
        crate::commands::viewer_item(&f.engine, id)
            .unwrap()
            .thumb_key
    }

    fn cached_file(f: &crate::testutil::Fixture, key: &str) -> std::path::PathBuf {
        f.engine
            .thumbs
            .path_for(u64::from_str_radix(key, 16).unwrap(), ThumbSize::Grid)
    }

    /// A key names one picture, so a request is answered with the picture its URL names -
    /// and only a response that *is* that picture may be kept.
    #[test]
    fn a_thumbnail_is_cached_forever_only_under_its_own_key() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        let original = current_key(&f, id);
        assert_eq!(
            get(&f.engine, &format!("/thumb/{id}/grid/{original}")).status(),
            200
        );
        crate::commands::rotate_item(&f.engine, id, true).unwrap();

        // A tile that has not refetched yet still asks under the original's key, whose
        // thumbnail is still cached: it gets the original picture, which is what that URL
        // names, so it may keep it - "Original" brings that key back to that picture.
        let stale = get(&f.engine, &format!("/thumb/{id}/grid/{original}"));
        assert_eq!(dims(&stale), (40, 20));
        assert!(header(&stale, "cache-control").contains("immutable"));
        let fresh = get(
            &f.engine,
            &format!("/thumb/{id}/grid/{}", current_key(&f, id)),
        );
        assert_eq!(dims(&fresh), (20, 40));
        assert!(header(&fresh, "cache-control").contains("immutable"));

        // Once the original's thumbnail is collected, the same stale URL can only be
        // answered with the photo's current picture - which it must not keep.
        std::fs::remove_file(cached_file(&f, &original)).unwrap();
        let collected = get(&f.engine, &format!("/thumb/{id}/grid/{original}"));
        assert_eq!(dims(&collected), (20, 40));
        assert_eq!(header(&collected, "cache-control"), "no-store");
        let keyless = get(&f.engine, &format!("/thumb/{id}/grid"));
        assert_eq!(header(&keyless, "cache-control"), "no-store");
    }

    /// A thumbnail built on request, under the key the URL asked for, is that key's own.
    #[test]
    fn a_thumbnail_built_on_request_under_the_asked_key_is_kept() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        let key = current_key(&f, id);
        let _ = std::fs::remove_file(cached_file(&f, &key));
        let r = get(&f.engine, &format!("/thumb/{id}/grid/{key}"));
        assert_eq!(r.status(), 200);
        assert!(header(&r, "cache-control").contains("immutable"));
    }

    /// A cached thumbnail is served from its key alone: the id is only looked up to build
    /// one, so an id the library does not hold does not stop it.
    #[test]
    fn a_cached_thumbnail_is_served_without_looking_the_photo_up() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        let key = current_key(&f, id);
        assert_eq!(
            get(&f.engine, &format!("/thumb/{id}/grid/{key}")).status(),
            200
        );
        let r = get(&f.engine, &format!("/thumb/9999/grid/{key}"));
        assert_eq!(r.status(), 200);
        assert!(header(&r, "cache-control").contains("immutable"));
    }

    /// Only the spelling the UI is given is a key: anything else is looked up by id, and
    /// is not kept under a URL the UI never asks for.
    #[test]
    fn a_key_in_any_other_spelling_is_not_a_key() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        let key = current_key(&f, id);
        assert_eq!(
            get(&f.engine, &format!("/thumb/{id}/grid/{key}")).status(),
            200
        );
        // `+` always differs from the key's own spelling and `from_str_radix` accepts it;
        // upper case would not differ for a key that happens to be all digits.
        let r = get(&f.engine, &format!("/thumb/{id}/grid/+{key}"));
        assert_eq!(r.status(), 200);
        assert_eq!(header(&r, "cache-control"), "no-store");
    }

    #[test]
    fn an_edited_photo_is_served_as_the_edit_not_as_the_file() {
        let img = jpeg(40, 20);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let dims = |path: &str| {
            let r = get(&f.engine, path);
            assert_eq!(r.status(), 200, "{path}");
            let picture = image::load_from_memory(r.body()).unwrap();
            (picture.width(), picture.height())
        };
        assert_eq!(get(&f.engine, &format!("/image/{id}")).body(), &img);

        // Turned clockwise (20x40), then the top half of that.
        crate::commands::set_item_edit(&f.engine, id, 1, Some([0, 0, 65535, 32768])).unwrap();
        assert_eq!(dims(&format!("/image/{id}")), (20, 20));
        assert_eq!(
            dims(&format!("/image/{id}/uncropped")),
            (20, 40),
            "the crop tool draws on the whole turned picture"
        );
        let r = get(&f.engine, &format!("/image/{id}"));
        assert_eq!(header(&r, "content-type"), "image/jpeg");
        assert_eq!(header(&r, "cache-control"), "no-cache");
    }

    fn etag(r: &Response<Vec<u8>>) -> String {
        header(r, "etag").to_owned()
    }

    fn revalidate(f: &crate::testutil::Fixture, path: &str, tag: &str) -> Response<Vec<u8>> {
        call(&f.engine, path, Some(tag))
    }

    /// A revisit is answered from the webview's cache: the tag names the picture - the
    /// photo's thumbnail key - and a request that already holds it is a 304 without the
    /// file being read. The file is gone here, so a read would have been a 404.
    #[test]
    fn an_original_the_webview_holds_is_not_read_again() {
        let img = jpeg(40, 20);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let path = format!("/image/{id}");
        let first = get(&f.engine, &path);
        let tag = etag(&first);
        assert_eq!(tag, format!("\"{}\"", current_key(&f, id)));
        assert_eq!(header(&first, "cache-control"), "no-cache");

        std::fs::remove_file(f.photos.join("a.jpg")).unwrap();
        let again = revalidate(&f, &path, &tag);
        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
        assert!(again.body().is_empty());
        assert_eq!(etag(&again), tag);
        assert_eq!(header(&again, "cache-control"), "no-cache");

        // Any other tag is a stale copy, and gets the file - which is gone.
        assert_eq!(revalidate(&f, &path, "\"0000000000000000\"").status(), 404);
    }

    /// The header's other spellings: a list, a weak tag, and `*`.
    #[test]
    fn if_none_match_is_read_as_a_list_of_possibly_weak_tags() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let path = format!("/image/{}", f.ids()[0]);
        let tag = etag(&get(&f.engine, &path));
        for header in [
            format!("\"other\", {tag}"),
            format!("W/{tag}"),
            "*".to_owned(),
        ] {
            assert_eq!(revalidate(&f, &path, &header).status(), 304, "{header}");
        }
        assert_eq!(revalidate(&f, &path, "\"other\"").status(), 200);
    }

    /// An edited photo is rendered under `RENDERING`, one at a time; revalidating the
    /// picture the webview already has must neither render it again nor queue for that
    /// lock behind someone else's render.
    #[test]
    fn a_held_render_is_revalidated_without_rendering_again() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        crate::commands::set_item_edit(&f.engine, id, 1, Some([0, 0, 65535, 32768])).unwrap();
        let cropped = format!("/image/{id}");
        let uncropped = format!("/image/{id}/uncropped");
        let cropped_tag = etag(&get(&f.engine, &cropped));
        let uncropped_tag = etag(&get(&f.engine, &uncropped));
        assert_eq!(cropped_tag, format!("\"{}\"", current_key(&f, id)));
        assert_ne!(
            cropped_tag, uncropped_tag,
            "the two routes serve different pictures"
        );

        let held = RENDERING.lock();
        let (tx, rx) = std::sync::mpsc::channel();
        let engine = f.engine.clone();
        let requests = [
            (cropped.clone(), cropped_tag.clone()),
            (uncropped.clone(), uncropped_tag.clone()),
        ];
        std::thread::spawn(move || {
            let statuses: Vec<_> = requests
                .iter()
                .map(|(path, tag)| call(&engine, path, Some(tag)).status())
                .collect();
            let _ = tx.send(statuses);
        });
        let statuses = rx.recv_timeout(std::time::Duration::from_secs(10));
        drop(held);
        assert_eq!(
            statuses.expect("a revalidation waited for the render lock"),
            [StatusCode::NOT_MODIFIED, StatusCode::NOT_MODIFIED]
        );

        // Each route's tag is its own: the other route's names a different picture.
        assert_eq!(revalidate(&f, &cropped, &uncropped_tag).status(), 200);
        assert_eq!(revalidate(&f, &uncropped, &cropped_tag).status(), 200);
    }

    /// The routes' tags never coincide, even for a photo only turned, whose two pictures
    /// have one key; and `/uncropped`'s names the uncropped picture, so changing only the
    /// crop leaves the one the crop tool holds valid.
    #[test]
    fn the_uncropped_route_is_tagged_by_its_own_picture() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        let cropped = format!("/image/{id}");
        let uncropped = format!("/image/{id}/uncropped");

        crate::commands::set_item_edit(&f.engine, id, 1, None).unwrap();
        let turned = current_key(&f, id);
        assert_eq!(etag(&get(&f.engine, &cropped)), format!("\"{turned}\""));
        assert_eq!(
            etag(&get(&f.engine, &uncropped)),
            format!("\"{turned}-uncropped\"")
        );

        let tag = etag(&get(&f.engine, &uncropped));
        crate::commands::set_item_edit(&f.engine, id, 1, Some([0, 0, 65535, 32768])).unwrap();
        assert_eq!(revalidate(&f, &uncropped, &tag).status(), 304);
        assert_eq!(
            revalidate(&f, &cropped, &format!("\"{turned}\"")).status(),
            200
        );
    }

    /// An edit changes the picture, so the tag the webview holds from before it no longer
    /// matches, and the new picture is sent.
    #[test]
    fn an_edit_invalidates_the_held_picture() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        let path = format!("/image/{id}");
        let before = etag(&get(&f.engine, &path));
        crate::commands::rotate_item(&f.engine, id, true).unwrap();
        let after = revalidate(&f, &path, &before);
        assert_eq!(after.status(), 200);
        assert_eq!(dims(&after), (20, 40));
        assert_ne!(etag(&after), before);
    }

    /// A thumbnail that is not built yet is waited for without a blocking thread. Here the
    /// runtime has one, and the wait is a video frame no webview will ever draw: parked on
    /// that thread, it would hold the full-size image behind it for all of `THUMB_TIMEOUT`.
    #[test]
    fn a_thumbnail_wait_holds_no_blocking_thread() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20)), ("clip.mp4", &vec![0u8; 4096])]);
        f.add_photos();
        let kind = |id: i64| f.engine.lib.item(id).unwrap().unwrap().kind;
        let ids = f.ids();
        let clip = *ids
            .iter()
            .find(|&&id| kind(id) == MediaKind::Video)
            .unwrap();
        let photo = *ids
            .iter()
            .find(|&&id| kind(id) == MediaKind::Image)
            .unwrap();
        crate::commands::video_session_start(&f.engine, true).unwrap();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_time()
            .build()
            .unwrap();
        let engine = f.engine.clone();
        runtime.block_on(async move {
            let waiting = tokio::spawn(handle(engine.clone(), format!("/thumb/{clip}/grid"), None));
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(!waiting.is_finished(), "nothing draws the video's frame");
            let image = tokio::time::timeout(
                Duration::from_secs(5),
                handle(engine, format!("/image/{photo}"), None),
            )
            .await
            .expect("the full-size image queued behind a thumbnail wait");
            assert_eq!(image.status(), 200);
            waiting.abort();
        });
        runtime.shutdown_timeout(Duration::ZERO);
    }

    #[test]
    fn maps_errors_to_status_codes() {
        let f = fixture(&[("bad.jpg", b"garbage")]);
        f.add_photos();
        let bad = f.ids()[0];
        assert_eq!(
            get(&f.engine, &format!("/thumb/{bad}/grid/k")).status(),
            422
        );
        assert_eq!(get(&f.engine, "/thumb/9999/grid/k").status(), 404);
        assert_eq!(get(&f.engine, "/image/9999").status(), 404);
        assert_eq!(get(&f.engine, "/thumb/abc/grid/k").status(), 400);
        assert_eq!(get(&f.engine, "/thumb/1/huge/k").status(), 400);
        assert_eq!(get(&f.engine, "/nope").status(), 404);
    }

    #[test]
    fn the_full_image_route_refuses_a_video() {
        let f = fixture(&[("clip.mp4", &vec![0u8; 4096])]);
        f.add_photos();
        let id = f.ids()[0];
        assert_eq!(
            get(&f.engine, &format!("image/{id}")).status(),
            StatusCode::NOT_FOUND
        );
    }
}
