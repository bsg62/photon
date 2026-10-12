//! The engine as the place thumbnails come from.

use super::{
    loader::{Building, LoadError, ThumbSource, Want},
    textures::Pixels,
};
use photon_core::thumbs::{ThumbService, ThumbSize};
use photon_engine::engine::Engine;
use std::sync::Arc;

/// The engine's thumbnails of one size: the grid's, or the previews the viewer shows
/// until a photo's full picture is made.
pub struct EngineThumbs {
    engine: Arc<Engine>,
    size: ThumbSize,
}

impl EngineThumbs {
    pub fn grid(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            size: ThumbSize::Grid,
        }
    }

    pub fn preview(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            size: ThumbSize::Preview,
        }
    }
}

impl ThumbSource for EngineThumbs {
    fn cached(&self, key: u64) -> Option<Pixels> {
        let image = self.engine.thumbs.decoded(key, self.size).ok()?;
        Some(pixels(image.width(), image.height(), image.into_raw()))
    }

    fn build(&self, want: Want) -> Building<'_> {
        Box::pin(async move {
            let (thumbs, size) = (&self.engine.thumbs, self.size);
            let path = thumbs
                .request_async(want.id, size)
                .await
                .map_err(load_error)?;
            // `request_async` answers with the photo's thumbnail as it is now. If the photo
            // has been edited since the grid named it, that is another picture than the
            // one `want.key` names, and keys recur ("Original", a fourth quarter turn):
            // stored under this key it would be drawn for the other picture when the photo
            // comes back to it. The grid asks under the new key once it has the new index.
            if path != thumbs.path_for(want.key, size) {
                return Err(LoadError::Unavailable);
            }
            let image = ThumbService::decode_file(&path).map_err(load_error)?;
            Ok(pixels(image.width(), image.height(), image.into_raw()))
        })
    }
}

fn pixels(width: u32, height: u32, rgba: Vec<u8>) -> Pixels {
    Pixels {
        width,
        height,
        rgba,
    }
}

/// A thumbnail that cannot be made is a failure; everything else - not built in time, a
/// photo gone since the grid named it, a cache file that will not read - may be there next
/// time.
fn load_error(err: photon_core::Error) -> LoadError {
    match err {
        photon_core::Error::ThumbFailed(message) => LoadError::Failed(message),
        _ => LoadError::Unavailable,
    }
}
