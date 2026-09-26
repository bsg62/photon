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

use crate::engine::Engine;
use photon_core::{Error, thumbs::ThumbSize};
use std::{path::Path, time::Duration};
use tauri::http::{Response, StatusCode, header};

pub const THUMB_TIMEOUT: Duration = Duration::from_secs(30);

pub fn handle(engine: &Engine, path: &str) -> Response<Vec<u8>> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match parts.as_slice() {
        ["thumb", id, size, rest @ ..] => thumb(engine, id, size, rest.first().copied()),
        ["image", id] => image(engine, id, true),
        ["image", id, "uncropped"] => image(engine, id, false),
        _ => text(StatusCode::NOT_FOUND, "not found"),
    }
}

fn thumb(engine: &Engine, id: &str, size: &str, url_key: Option<&str>) -> Response<Vec<u8>> {
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
    if let Some(key) = url_key
        && let Ok(bytes) = std::fs::read(engine.thumbs.path_for(key, size))
    {
        return ok(bytes, "image/webp", FOREVER);
    }
    match engine.thumbs.request(id, size, THUMB_TIMEOUT) {
        Ok(file) => match std::fs::read(&file) {
            Ok(bytes) => {
                // `request` serves the photo's *current* thumbnail whatever key was asked
                // for, and keys can recur - "Original", or a fourth quarter turn, returns a
                // photo to a key it has had before. Only a file that is the URL key's own may
                // be kept: a request still carrying the original's key while the row holds an
                // edit would otherwise pin the edited picture under the original's URL for a
                // year.
                let own = url_key.is_some_and(|key| engine.thumbs.path_for(key, size) == file);
                ok(bytes, "image/webp", if own { FOREVER } else { "no-store" })
            }
            Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
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
fn parse_key(key: &str) -> Option<u64> {
    u64::from_str_radix(key, 16)
        .ok()
        .filter(|&parsed| photon_core::grid::hex_key(parsed) == key)
}

/// One full-size render at a time. A render holds a whole decoded photo (150 MB and up
/// for 24 MP) on a protocol thread, outside the thumbnail pool whose `MAX_WORKERS` is what
/// bounds decode memory; flicking through a run of edited photos would otherwise start one
/// per photo passed.
///
/// An export takes the same lock: a full-size render is a full-size render whoever asked
/// for it, and an export of a hundred edited photos beside a viewer flicking through them
/// would otherwise be two at once.
pub(crate) static RENDERING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

fn image(engine: &Engine, id: &str, cropped: bool) -> Response<Vec<u8>> {
    let Ok(id) = id.parse::<i64>() else {
        return text(StatusCode::BAD_REQUEST, "bad id");
    };
    let item = match engine.lib.item(id) {
        Ok(Some(item)) if item.missing_since.is_none() => item,
        Ok(_) => return text(StatusCode::NOT_FOUND, "not found"),
        Err(err) => return text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let edit = if cropped {
        item.edit
    } else {
        item.edit.without_crop()
    };
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
        ) {
            Ok((bytes, mime)) => ok(bytes, mime, "no-cache"),
            Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                text(StatusCode::NOT_FOUND, "not found")
            }
            Err(Error::Io(err)) => text(StatusCode::SERVICE_UNAVAILABLE, &err.to_string()),
            Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        };
    }
    match std::fs::read(&item.path) {
        Ok(bytes) => ok(bytes, mime_for(Path::new(&item.path)), "no-cache"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            text(StatusCode::NOT_FOUND, "not found")
        }
        Err(err) => text(StatusCode::SERVICE_UNAVAILABLE, &err.to_string()),
    }
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
        let r = handle(&f.engine, &format!("/thumb/{id}/grid/{key}"));
        assert_eq!(r.status(), 200);
        assert_eq!(header(&r, "content-type"), "image/webp");
        assert!(header(&r, "cache-control").contains("immutable"));
        assert!(!r.body().is_empty());
        assert_eq!(
            handle(&f.engine, &format!("/thumb/{id}/preview/x")).status(),
            200
        );
    }

    #[test]
    fn serves_originals_with_their_mime_type() {
        let img = jpeg(64, 32);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let r = handle(&f.engine, &format!("/image/{}", f.ids()[0]));
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
            handle(&f.engine, &format!("/thumb/{id}/grid/{original}")).status(),
            200
        );
        crate::commands::rotate_item(&f.engine, id, true).unwrap();

        // A tile that has not refetched yet still asks under the original's key, whose
        // thumbnail is still cached: it gets the original picture, which is what that URL
        // names, so it may keep it - "Original" brings that key back to that picture.
        let stale = handle(&f.engine, &format!("/thumb/{id}/grid/{original}"));
        assert_eq!(dims(&stale), (40, 20));
        assert!(header(&stale, "cache-control").contains("immutable"));
        let fresh = handle(
            &f.engine,
            &format!("/thumb/{id}/grid/{}", current_key(&f, id)),
        );
        assert_eq!(dims(&fresh), (20, 40));
        assert!(header(&fresh, "cache-control").contains("immutable"));

        // Once the original's thumbnail is collected, the same stale URL can only be
        // answered with the photo's current picture - which it must not keep.
        std::fs::remove_file(cached_file(&f, &original)).unwrap();
        let collected = handle(&f.engine, &format!("/thumb/{id}/grid/{original}"));
        assert_eq!(dims(&collected), (20, 40));
        assert_eq!(header(&collected, "cache-control"), "no-store");
        let keyless = handle(&f.engine, &format!("/thumb/{id}/grid"));
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
        let r = handle(&f.engine, &format!("/thumb/{id}/grid/{key}"));
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
            handle(&f.engine, &format!("/thumb/{id}/grid/{key}")).status(),
            200
        );
        let r = handle(&f.engine, &format!("/thumb/9999/grid/{key}"));
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
            handle(&f.engine, &format!("/thumb/{id}/grid/{key}")).status(),
            200
        );
        let r = handle(
            &f.engine,
            &format!("/thumb/{id}/grid/{}", key.to_uppercase()),
        );
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
            let r = handle(&f.engine, path);
            assert_eq!(r.status(), 200, "{path}");
            let picture = image::load_from_memory(r.body()).unwrap();
            (picture.width(), picture.height())
        };
        assert_eq!(handle(&f.engine, &format!("/image/{id}")).body(), &img);

        // Turned clockwise (20x40), then the top half of that.
        crate::commands::set_item_edit(&f.engine, id, 1, Some([0, 0, 65535, 32768])).unwrap();
        assert_eq!(dims(&format!("/image/{id}")), (20, 20));
        assert_eq!(
            dims(&format!("/image/{id}/uncropped")),
            (20, 40),
            "the crop tool draws on the whole turned picture"
        );
        let r = handle(&f.engine, &format!("/image/{id}"));
        assert_eq!(header(&r, "content-type"), "image/jpeg");
        assert_eq!(header(&r, "cache-control"), "no-cache");
    }

    #[test]
    fn maps_errors_to_status_codes() {
        let f = fixture(&[("bad.jpg", b"garbage")]);
        f.add_photos();
        let bad = f.ids()[0];
        assert_eq!(
            handle(&f.engine, &format!("/thumb/{bad}/grid/k")).status(),
            422
        );
        assert_eq!(handle(&f.engine, "/thumb/9999/grid/k").status(), 404);
        assert_eq!(handle(&f.engine, "/image/9999").status(), 404);
        assert_eq!(handle(&f.engine, "/thumb/abc/grid/k").status(), 400);
        assert_eq!(handle(&f.engine, "/thumb/1/huge/k").status(), 400);
        assert_eq!(handle(&f.engine, "/nope").status(), 404);
    }
}
