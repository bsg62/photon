//! Copy photo, on the application's side: from the grid's Ctrl+C with one photo selected,
//! the tile menu's "Copy photo" and the viewer's Ctrl+C. The picture as shown - the edit
//! applied, at most 2,560 pixels a side - is made by the engine (`commands::copy_picture`,
//! under `RENDERING`) and put on the clipboard by the platform, on a thread of its own:
//! the decode takes a second or two, and the clipboard's write is not cheap either.
//!
//! **One copy at a time, and the last photo asked for is the one on the clipboard**: a
//! second press for the photo on its way is dropped, as the Svelte store drops it -
//! another full-size decode of the same photo, for nothing - and a press for another photo
//! waits for it and goes after it, the latest such press alone. Dropped, Ctrl+C on the
//! next photo of the viewer a moment after the last said "Photo copied" of the first, and
//! the first was what was pasted. **What is done is said** ("Photo copied"): a copy changes nothing on screen, and without a
//! word a copy cannot be told from a key that missed.

use super::App;
use photon_engine::commands;
use std::sync::mpsc;

/// What is said when a copy is done: the Svelte store's words.
pub(crate) const COPIED: &str = "Photo copied";

impl App {
    /// Puts the photo `id` on the clipboard, unless a copy is on its way.
    pub(super) fn copy_photo(&mut self, id: i64) {
        if let Some((busy, _)) = &self.copying {
            if *busy != id {
                self.next_copy = Some(id);
            }
            return;
        }
        let (send, answer) = mpsc::channel();
        let (engine, platform, ctx) =
            (self.engine.clone(), self.platform.clone(), self.ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("copy photo".to_owned())
            .spawn(move || {
                let copied = commands::copy_picture(&engine, id)
                    .map_err(|err| err.message)
                    .and_then(|picture| {
                        let (width, height) = picture.dimensions();
                        platform.copy_image(width as usize, height as usize, picture.into_raw())
                    });
                let _ = send.send(copied);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.copying = Some((id, answer)),
            Err(err) => tracing::error!(%err, "the copy's thread could not start"),
        }
    }

    /// The copy's answer, once it has one: said. Says whether it answered.
    pub(super) fn copied(&mut self, now: f64) -> bool {
        let Some((_, copying)) = &self.copying else {
            return false;
        };
        let answer = match copying.try_recv() {
            Ok(answer) => answer,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Couldn't put the photo on the clipboard.".to_owned())
            }
        };
        self.copying = None;
        match answer {
            Ok(()) => self.toasts.done(COPIED, now),
            Err(said) => self.toasts.error(said, now),
        }
        if let Some(next) = self.next_copy.take() {
            self.copy_photo(next);
        }
        true
    }
}
