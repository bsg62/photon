//! The engine's events on their way to the UI thread.
//!
//! The engine reports from its own threads - a scan, a rebuild, a pass. Each report is
//! put on a channel and the UI is asked for a frame, in which it takes them. The channel
//! has no bound and the UI empties it every frame, so a sender never waits.

use eframe::egui;
use photon_engine::events::{
    Events, ExportProgress, FaceProgress, FolderStatus, LibraryChanged, ScanProgressEvent,
};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Library(LibraryChanged),
    Scan(ScanProgressEvent),
    Folder(FolderStatus),
    Export(ExportProgress),
    Face(FaceProgress),
}

pub struct UiEvents {
    events: Sender<Event>,
    ctx: egui::Context,
}

impl UiEvents {
    pub fn new(ctx: egui::Context) -> (Self, Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        (Self { events, ctx }, receiver)
    }

    fn send(&self, event: Event) {
        // A UI that has gone has nobody to draw for.
        if self.events.send(event).is_ok() {
            self.ctx.request_repaint();
        }
    }
}

impl Events for UiEvents {
    fn library_changed(&self, event: LibraryChanged) {
        self.send(Event::Library(event));
    }
    fn scan_progress(&self, event: ScanProgressEvent) {
        self.send(Event::Scan(event));
    }
    fn folder_status(&self, event: FolderStatus) {
        self.send(Event::Folder(event));
    }
    fn export_progress(&self, event: ExportProgress) {
        self.send(Event::Export(event));
    }
    fn face_progress(&self, event: FaceProgress) {
        self.send(Event::Face(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    /// A context that counts how often it is asked for a frame.
    fn counting() -> (egui::Context, Arc<AtomicUsize>) {
        let ctx = egui::Context::default();
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = asked.clone();
        ctx.set_request_repaint_callback(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        (ctx, asked)
    }

    #[test]
    fn an_event_reaches_the_ui_thread_and_asks_for_a_frame() {
        let (ctx, asked) = counting();
        let (events, receiver) = UiEvents::new(ctx);
        let changed = LibraryChanged {
            version: 3,
            len: 10,
            data_changed: true,
        };
        events.library_changed(changed);
        assert_eq!(receiver.try_recv(), Ok(Event::Library(changed)));
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_event_for_a_ui_that_has_gone_is_dropped() {
        let (ctx, asked) = counting();
        let (events, receiver) = UiEvents::new(ctx);
        drop(receiver);
        events.folder_status(FolderStatus {
            watched_id: 1,
            online: true,
            degraded: false,
        });
        assert_eq!(asked.load(Ordering::SeqCst), 0);
    }
}
