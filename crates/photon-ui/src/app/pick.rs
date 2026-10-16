//! The system's folder picker, on the application's side: asked for by "Choose…" beside
//! the export's destination and Settings' folder field, its answer put into the field that
//! asked and asked about as a typed path is; and by "Add folder…" - Settings' and the
//! empty library's - whose folder is watched at once, as the Tauri photon's is
//! (`add-folder.ts`). Those opened Settings' field a path is typed in first, and the
//! picker over it: steps the Tauri photon does not have, which the first person to run
//! the window said. The field is what is left where no picker can be opened.
//!
//! **The picker runs on a thread of its own**: a portal answers when the user does, and a
//! window that stopped drawing meanwhile would be taken by the system for one that does
//! not answer. **One at a time, and its answer is for the field that last asked for it**,
//! by the number of the dialog's opening: a press while a picker is open - a picker that
//! is not tied to the window, which a portal's on Wayland and Windows' with no owner are
//! not, can be behind it - opens no second one and takes the open one for the field it is
//! made in. Taken for nothing, the old one's answer went to a dialog that had closed, and
//! the new "Choose…" did nothing at all. A folder chosen for a dialog that has closed since
//! goes nowhere: put into another, it would be a destination nobody chose. "Choose…" is
//! never dimmed for it: dimmed, a button gives the keyboard up (egui lets go of what senses
//! no press), and after the picker nothing in the dialog had it.

use super::App;
use crate::{
    dialogs::Dialog,
    platform::{NO_PICKER, Picked},
};
use std::sync::mpsc;

/// Which field asked for a folder, or none: a folder to watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PickFor {
    Export,
    AddFolder,
    /// "Add folder…": the folder chosen is watched.
    Watch,
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
        PickFor::AddFolder | PickFor::Watch => "Add a folder to photon",
    }
}

impl App {
    /// Opens the system's folder picker for the field `of` of the dialog on screen, unless
    /// one is open already.
    pub(super) fn pick_folder(&mut self, of: PickFor) {
        if let Some(picking) = &mut self.picking {
            picking.of = of;
            picking.opening = self.dialog_openings;
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
        // A folder to watch is watched whatever is on screen by then: it was asked for
        // by the user, for nothing but itself.
        if of != PickFor::Watch && opening != self.dialog_openings {
            return true;
        }
        let folder = match answer {
            Picked::Folder(folder) => folder,
            Picked::Cancelled => return true,
            Picked::Unavailable => {
                self.toasts.error(NO_PICKER.to_owned(), now);
                // Where no picker opens, the field a path is typed in is the way left.
                if of == PickFor::Watch {
                    self.type_a_folder();
                }
                return true;
            }
        };
        // A path that is not Unicode is put in the field as near as it can be, and is
        // then said to be no folder, as it would be if it were typed.
        let text = (folder.into_os_string().into_string())
            .unwrap_or_else(|os| os.to_string_lossy().into_owned());
        if of == PickFor::Watch {
            // Said as a folder dropped on the window is: "Watching “…”", or why not.
            self.writes.push(super::Write::Watch { paths: vec![text] });
            return true;
        }
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

    /// Settings, at its folders, with the field a path is typed in open.
    pub(super) fn type_a_folder(&mut self) {
        if !matches!(&self.dialog, Some(Dialog::Settings(_))) {
            self.open_settings(crate::settings::Section::Folders);
        }
        if let Some(Dialog::Settings(settings)) = &mut self.dialog {
            settings.section = crate::settings::Section::Folders;
            if settings.adding.is_none() {
                settings.adding = Some(Default::default());
            }
        }
    }
}
