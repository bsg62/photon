//! The system's folder picker, on the application's side: asked for by "Choose…" beside
//! the export's destination and Settings' folder field, and by the empty library's "Add
//! folder…"; its answer put into the field that asked, and asked about as a typed path is.
//!
//! **The picker runs on a thread of its own**: a portal answers when the user does, and a
//! window that stopped drawing meanwhile would be taken by the system for one that does
//! not answer. One at a time: the "Choose…" of a field whose picker is open takes no
//! second press. **The folder goes only into the dialog that asked**, by the number of its
//! opening: closed meanwhile, or another opened in its place, the answer is dropped - put
//! into a dialog the user had not asked it for, it would be a destination nobody chose.

use super::App;
use crate::{
    dialogs::Dialog,
    platform::{NO_PICKER, Picked},
};
use std::sync::mpsc;

/// Which field asked for a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PickFor {
    Export,
    AddFolder,
}

/// The picker that is open.
pub(super) struct Picking {
    of: PickFor,
    /// The opening of the dialog that asked (`App::dialog_openings`).
    opening: u64,
    answer: mpsc::Receiver<Picked>,
}

/// The picker's title, the Tauri photon's words.
fn title(of: PickFor) -> &'static str {
    match of {
        PickFor::Export => "Export copies to…",
        PickFor::AddFolder => "Add a folder to photon",
    }
}

impl App {
    /// Opens the system's folder picker for the field `of` of the dialog on screen, unless
    /// one is open already.
    pub(super) fn pick_folder(&mut self, of: PickFor) {
        if self.picking.is_some() {
            return;
        }
        let (send, answer) = mpsc::channel();
        let (platform, ctx) = (self.platform.clone(), self.ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("folder picker".to_owned())
            .spawn(move || {
                let _ = send.send(platform.pick_folder(title(of)));
                ctx.request_repaint();
            });
        if let Err(err) = spawned {
            tracing::error!(%err, "the folder picker's thread could not start");
            return;
        }
        self.picking = Some(Picking {
            of,
            opening: self.dialog_openings,
            answer,
        });
        self.set_choosing(of, true);
    }

    /// The picker's answer, once it has given one: the folder put in the field that asked
    /// for it, while that dialog is still the one on screen. Says whether it answered.
    pub(super) fn picked(&mut self, now: f64) -> bool {
        let Some(picking) = &self.picking else {
            return false;
        };
        let answer = match picking.answer.try_recv() {
            Ok(answer) => answer,
            Err(mpsc::TryRecvError::Empty) => return false,
            // The thread ended without an answer - the picker panicked: no picker.
            Err(mpsc::TryRecvError::Disconnected) => Picked::Unavailable,
        };
        let Some(Picking { of, opening, .. }) = self.picking.take() else {
            return false;
        };
        if opening != self.dialog_openings {
            return true;
        }
        self.set_choosing(of, false);
        let folder = match answer {
            Picked::Folder(folder) => folder,
            Picked::Cancelled => return true,
            Picked::Unavailable => {
                self.toasts.error(NO_PICKER.to_owned(), now);
                return true;
            }
        };
        // A path that is not Unicode is put in the field as near as it can be, and is
        // then said to be no folder, as it would be if it were typed.
        let text = (folder.into_os_string().into_string())
            .unwrap_or_else(|os| os.to_string_lossy().into_owned());
        match (of, &mut self.dialog) {
            (PickFor::Export, Some(Dialog::Export(export))) => {
                if let Some(ask) = export.type_in(text) {
                    self.checking.ask(ask);
                }
            }
            (PickFor::AddFolder, Some(Dialog::Settings(settings))) => {
                if let Some(adding) = &mut settings.adding
                    && let Some(ask) = adding.type_in(text)
                {
                    self.adding_check.ask(ask);
                }
            }
            _ => {}
        }
        true
    }

    /// Marks the field `of` of the dialog on screen as waiting for the picker, or not.
    fn set_choosing(&mut self, of: PickFor, choosing: bool) {
        match (of, &mut self.dialog) {
            (PickFor::Export, Some(Dialog::Export(export))) => export.choosing = choosing,
            (PickFor::AddFolder, Some(Dialog::Settings(settings))) => {
                if let Some(adding) = &mut settings.adding {
                    adding.choosing = choosing;
                }
            }
            _ => {}
        }
    }
}
