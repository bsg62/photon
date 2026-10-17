//! The thumbnails the grid draws: the loader's results uploaded as textures, within a
//! frame's budget, and the grid's wants handed back to the loader.

use super::{
    loader::{LoadError, Loader, Want},
    textures::{DEFAULT_LIMIT, Pixels, TexKey, Textures, UPLOADS_PER_FRAME},
};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::{collections::HashSet, time::Duration};

pub struct Thumbs {
    textures: Textures<TextureHandle>,
    loader: Loader,
    /// Whether a thumbnail that arrives late fades in: the grid's do, the viewer's
    /// previews do not.
    fades: bool,
}

/// Bytes of preview kept as textures. A preview is at most 1600 pixels on its longer
/// side, about seven megabytes: the photo on screen and two on either side of it, and
/// room for the ones just left.
pub const PREVIEW_LIMIT: usize = 96 << 20;

impl Thumbs {
    /// The grid's thumbnails.
    pub fn new(loader: Loader) -> Self {
        Self {
            textures: Textures::new(DEFAULT_LIMIT),
            loader,
            fades: true,
        }
    }

    /// The viewer's previews: a store of its own, which holds whatever its `loader`'s
    /// source makes (`EngineThumbs::preview`), under the keys the grid's are held under
    /// in theirs.
    pub fn previews(loader: Loader) -> Self {
        Self {
            textures: Textures::new(PREVIEW_LIMIT),
            loader,
            fades: false,
        }
    }

    /// Whether a late thumbnail fades in: the Settings switch for animations.
    pub fn set_fades(&mut self, fades: bool) {
        self.fades = fades;
    }

    /// Once per frame, before the tiles are drawn: takes what the loader has finished,
    /// uploads within the budget, and tells the loader what is wanted now - `wanted`, most
    /// wanted first, or nothing at all while `defer` holds, since a tile that will be gone
    /// before it settles must not cost a render. Answers whether any of them is still on
    /// its way: wanted and not yet a texture, a failure or a thing put off. `still` is
    /// whether the grid is at rest, the one place a late thumbnail fades in.
    pub fn frame(
        &mut self,
        ctx: &egui::Context,
        wanted: &[Want],
        defer: bool,
        still: bool,
    ) -> bool {
        self.textures.begin_frame();
        let keys: Vec<TexKey> = wanted.iter().map(|want| TexKey::grid(want.key)).collect();
        let wanted_keys: HashSet<TexKey> = keys.iter().copied().collect();
        let now = ctx.input(|input| input.time);

        for loaded in self.loader.poll() {
            let key = TexKey::grid(loaded.key);
            match loaded.result {
                Ok(pixels) => {
                    self.textures.arrived(key, now);
                    self.textures.offer(key, pixels, &wanted_keys);
                }
                Err(LoadError::Failed(_)) => self.textures.fail(key),
                Err(LoadError::Unavailable) => self.textures.put_off(key, now),
            }
        }
        let fade = (now, self.fades && still);
        let more = (self.textures).upload(UPLOADS_PER_FRAME, &wanted_keys, fade, |key, pixels| {
            upload(ctx, key, pixels)
        });
        if more || self.textures.fades(now) {
            ctx.request_repaint();
        }

        // A still grid draws no frame by itself: without this one a thumbnail that was not
        // there the first time would never be asked for again.
        if let Some(at) = self.textures.next_retry(&wanted_keys) {
            ctx.request_repaint_after(Duration::from_secs_f64((at - now).max(0.0)));
        }

        let missing_keys = self.textures.missing(&keys, now);
        if !defer {
            self.textures.asking(&missing_keys, &wanted_keys, now);
        }
        let missing: HashSet<u64> = missing_keys.into_iter().map(|key| key.key).collect();
        self.loader.want(if defer {
            Vec::new()
        } else {
            wanted
                .iter()
                .filter(|want| missing.contains(&want.key))
                .copied()
                .collect()
        });
        more || !missing.is_empty()
    }

    /// The texture of the picture cached under `key`, if it has been uploaded.
    pub fn texture(&mut self, key: u64) -> Option<&TextureHandle> {
        self.textures.get(TexKey::grid(key))
    }

    /// How much of the thumbnail under `key` is seen at `now`: less while it fades in.
    pub fn shown(&self, key: u64, now: f64) -> f32 {
        self.textures.shown(TexKey::grid(key), now)
    }

    /// Whether the picture cached under `key` is a texture, without counting it as drawn.
    pub fn holds(&self, key: u64) -> bool {
        self.textures.holds(TexKey::grid(key))
    }

    /// Whether the picture under `key` could not be made.
    pub fn failed(&self, key: u64) -> bool {
        self.textures.failed(TexKey::grid(key))
    }

    /// Whether the picture under `key` was not to be had when last asked for, and has not
    /// arrived since: not built in time, a video with no poster, a photo whose drive is
    /// not there. It is asked for again; meanwhile the tile shows its mark.
    pub fn troubled(&self, key: u64) -> bool {
        self.textures.troubled(TexKey::grid(key))
    }

    /// Bytes of texture held.
    pub fn bytes(&self) -> usize {
        self.textures.bytes()
    }

    /// Lets go of every texture: the viewer has closed. What the loader finishes after
    /// this is dropped at the next `frame` that does not want it.
    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

fn upload(ctx: &egui::Context, key: TexKey, pixels: &Pixels) -> TextureHandle {
    ctx.load_texture(
        format!("thumb-{:016x}", key.key),
        ColorImage::from_rgba_unmultiplied(
            [pixels.width as usize, pixels.height as usize],
            &pixels.rgba,
        ),
        TextureOptions::LINEAR,
    )
}
