//! The `photon://` URI scheme: thumbnails and original images for the webview.
//!
//! - `/thumb/<id>/<grid|preview>/<thumbKey>`: WebP, built on demand, cached forever
//!   (`thumbKey` changes when the file does).
//! - `/image/<id>`: the original file.

use crate::engine::Engine;
use photon_core::{Error, thumbs::ThumbSize};
use std::{path::Path, time::Duration};
use tauri::http::{Response, StatusCode, header};

pub const THUMB_TIMEOUT: Duration = Duration::from_secs(30);

pub fn handle(engine: &Engine, path: &str) -> Response<Vec<u8>> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match parts.as_slice() {
        ["thumb", id, size, ..] => thumb(engine, id, size),
        ["image", id] => image(engine, id),
        _ => text(StatusCode::NOT_FOUND, "not found"),
    }
}

fn thumb(engine: &Engine, id: &str, size: &str) -> Response<Vec<u8>> {
    let Ok(id) = id.parse::<i64>() else {
        return text(StatusCode::BAD_REQUEST, "bad id");
    };
    let size = match size {
        "grid" => ThumbSize::Grid,
        "preview" => ThumbSize::Preview,
        _ => return text(StatusCode::BAD_REQUEST, "bad size"),
    };
    match engine.thumbs.request(id, size, THUMB_TIMEOUT) {
        Ok(file) => match std::fs::read(&file) {
            Ok(bytes) => ok(bytes, "image/webp", "public, max-age=31536000, immutable"),
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

fn image(engine: &Engine, id: &str) -> Response<Vec<u8>> {
    let Ok(id) = id.parse::<i64>() else {
        return text(StatusCode::BAD_REQUEST, "bad id");
    };
    let item = match engine.lib.item(id) {
        Ok(Some(item)) if item.missing_since.is_none() => item,
        Ok(_) => return text(StatusCode::NOT_FOUND, "not found"),
        Err(err) => return text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
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
        let r = handle(&f.engine, &format!("/thumb/{id}/grid/0123456789abcdef"));
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
