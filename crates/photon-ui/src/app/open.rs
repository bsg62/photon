//! Reveal and Open, on the application's side: a photo, a folder, a watched folder or the
//! library shown in the system's file manager; a photo opened in the system's app for it;
//! the place a photo was taken opened in the browser. The paths are the library's, read
//! off the window's thread, and every call into the system goes off it too - a file on a
//! share that has stopped answering, or a file manager starting, can take seconds
//! (spec `2026-10-16-photon-native-platform-design.md`, decision 5).
//!
//! **Each asking is a thread of its own and none waits for another**: they do not share
//! anything a second press could spoil, and nothing on screen changes for them. What goes
//! wrong is said, in the system's words where it has any.

use super::App;
use photon_engine::{commands, engine::Engine, error::AppError};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::mpsc,
};

/// What is asked of the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opening {
    /// A photo, selected in its folder.
    RevealPhoto(i64),
    /// A photo, in the system's app for it.
    OpenPhoto(i64),
    /// A folder of the library, by its id.
    RevealFolder(i64),
    /// A watched folder, by its id.
    RevealWatched(i64),
    /// The library's own file.
    RevealLibrary,
    /// The place a photo was taken, on OpenStreetMap.
    Map(i64),
}

/// What the system is asked: a path to show, or a file or address to open.
enum Asked {
    Reveal(PathBuf),
    Open(OsString),
    /// A photo with no position: nothing to open.
    Nothing,
}

/// What is said of a path that is not there - a photo deleted since, a drive unplugged.
pub(crate) const NOT_THERE: &str = "It isn't there any more, or its drive is not connected";

fn asked(engine: &Engine, opening: Opening) -> Result<Asked, AppError> {
    Ok(match opening {
        Opening::RevealPhoto(id) => Asked::Reveal(commands::item_path(engine, id)?),
        Opening::OpenPhoto(id) => Asked::Open(commands::item_path(engine, id)?.into()),
        Opening::RevealFolder(id) => Asked::Reveal(commands::folder_path(engine, id)?),
        Opening::RevealWatched(id) => Asked::Reveal(commands::watched_path(engine, id)?),
        Opening::RevealLibrary => Asked::Reveal(commands::app_info(engine).library_path),
        Opening::Map(id) => match commands::item_map_url(engine, id)? {
            Some(url) => Asked::Open(url.into()),
            None => Asked::Nothing,
        },
    })
}

impl App {
    /// Asks the system for `opening`, off the window's thread.
    pub(super) fn open_with_system(&mut self, opening: Opening) {
        let (engine, platform, ctx) =
            (self.engine.clone(), self.platform.clone(), self.ctx.clone());
        let said = self.open_said.0.clone();
        let spawned = std::thread::Builder::new()
            .name("reveal".to_owned())
            .spawn(move || {
                let done = asked(&engine, opening)
                    .map_err(|err| err.message)
                    .and_then(|asked| match asked {
                        Asked::Reveal(path) => there(&path).and_then(|()| platform.reveal(&path)),
                        // A photo's file, asked about; the map's address is no path.
                        Asked::Open(target) if matches!(opening, Opening::OpenPhoto(_)) => {
                            there(Path::new(&target)).and_then(|()| platform.open(&target))
                        }
                        Asked::Open(target) => platform.open(&target),
                        Asked::Nothing => Ok(()),
                    });
                if let Err(err) = done {
                    let _ = said.send(err);
                    ctx.request_repaint();
                }
            });
        if let Err(err) = spawned {
            tracing::error!(%err, "the opener's thread could not start");
        }
    }

    /// What went wrong with an asking, said. Says whether anything was.
    pub(super) fn open_answers(&mut self, now: f64) -> bool {
        let mut answered = false;
        while let Ok(said) = self.open_said.1.try_recv() {
            answered = true;
            self.toasts.error(said, now);
        }
        answered
    }
}

/// Whether `path` is there, asked before the system is: the file manager on Linux answers
/// for any path, and `xdg-open` is let go of before it finds out, so a photo on a drive
/// that is unplugged - the likeliest thing of all to be revealed from Settings - was
/// revealed to nobody, with nothing said. As Tauri's opener asks it.
fn there(path: &Path) -> Result<(), String> {
    std::fs::metadata(path)
        .map(|_| ())
        .map_err(|err| format!("{NOT_THERE}: {} ({err}).", path.display()))
}

/// Where the askings say what went wrong.
pub(super) fn said() -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    mpsc::channel()
}
