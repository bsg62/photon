//! The People page, on the application's side: opening and leaving it, reading it off the
//! window's thread, its crops, and doing what is pressed on it. The page itself is
//! `people_page.rs` and `people_view.rs`.
//!
//! **The page is not a view of the grid** (`mainPage` in the Svelte UI): it has no rows, so
//! the views' queue, `Nav` and the selection know nothing of it. It is drawn over the
//! grid's area, the sidebar and the top bar staying, and the grid is not drawn under it -
//! it keeps its place, which is a number of its own, so coming back finds it where it was.

use super::{App, View, Write};
use crate::{
    nav::{Step, Target},
    people_page::{PeoplePage, STRIP},
    people_view::PeopleAction,
    settings::Section,
};
use eframe::egui;
use photon_core::library::PeoplePage as Page;
use photon_engine::{commands, engine::Engine};

/// The page as the library answered a read: the number it was asked under, and the page.
pub(super) type Read = (u64, Result<Page, String>);

pub(super) fn read(engine: &Engine, number: u64) -> Read {
    let page = commands::people_page(engine, STRIP).map_err(|err| err.message);
    (number, page)
}

/// What is said when a face's photo has left the library since the page was read.
pub(crate) const PHOTO_GONE: &str = "That photo is no longer in the library.";

impl App {
    /// Whether the People page is on screen.
    pub fn people_open(&self) -> bool {
        self.page.is_some()
    }

    /// The page's model, while it is open: for a test.
    pub fn people_page(&self) -> Option<&PeoplePage> {
        self.page.as_ref()
    }

    /// Whether the page holds the crop of the face `face`, under its photo's key `key`.
    pub fn people_crop(&self, face: i64, key: &str) -> bool {
        self.people_view.holds_crop(&(face, key.to_owned()))
    }

    /// Opens the People page, from the label beside People's fold in the sidebar. The list
    /// under the fold opens too, so that the names are beside the page that makes them. A
    /// search half typed is dropped, or it would land behind the page and switch the grid
    /// where nobody is looking.
    pub(super) fn open_people(&mut self, now: f64) {
        self.search.cancel();
        if !self.layout.open.people {
            self.layout.open.people = true;
            self.layout_store.ask(self.layout);
        }
        if self.page.is_some() {
            return;
        }
        let mut model = PeoplePage::default();
        let number = model.reload(now);
        self.people_read.ask(number);
        self.page = Some(model);
    }

    /// Leaves the page for the grid: a row of the sidebar, a folder, typing in the search
    /// box, a link of the viewer that leaves it. Its crops are let go of.
    pub(super) fn leave_people(&mut self) {
        if self.page.take().is_some() {
            self.people_view.clear();
            self.face_crops.clear();
        }
    }

    /// What the page's tasks have answered, and the read a library change put off once
    /// it is due. Says whether anything was answered.
    pub(super) fn people_answers(&mut self, ctx: &egui::Context, now: f64) -> bool {
        let mut answered = false;
        if let Some(answer) = self.people_read.answer() {
            answered = true;
            match (answer, &mut self.page) {
                (Ok((number, Ok(page))), Some(model)) => model.loaded(number, page),
                (Ok((_, Err(said))), Some(_)) => self.toasts.error(said, now),
                _ => {}
            }
        }
        let made = self.face_crops.answers();
        if !made.is_empty() {
            answered = true;
            if self.page.is_some() {
                self.people_view.took(ctx, made);
            }
        }
        if let Some(model) = &mut self.page {
            if std::mem::take(&mut self.people_changed)
                && let Some(number) = model.changed(now)
            {
                self.people_read.ask(number);
            }
            if let Some(number) = model.poll(now) {
                self.people_read.ask(number);
            }
            if let Some(due) = model.due() {
                let wait = ((due - now) / 1000.0).max(0.0);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            }
        }
        self.people_changed = false;
        answered
    }

    /// The page's group is named, or the write refused: it is put back, and the page read
    /// again either way - a refusal can mean the page is stale.
    pub(super) fn group_named(&mut self, group: i64, written: bool, now: f64) {
        if let Some(model) = &mut self.page {
            model.named(group, written);
            let number = model.reload(now);
            self.people_read.ask(number);
        }
    }

    /// Does what was pressed on the page.
    pub(super) fn people_act(&mut self, action: PeopleAction) {
        match action {
            PeopleAction::Name { group, name } => {
                if let Some(model) = &mut self.page {
                    model.naming(group);
                }
                self.writes.push(Write::NameGroup { group, name });
            }
            PeopleAction::Open(id) => self.open_face(id),
            PeopleAction::OpenSettings => self.open_settings(Section::People),
        }
    }

    /// A face's photo, in the viewer over the page: where the grid on screen holds it, or
    /// in All photos, which the grid is switched to only when its view lacks the photo -
    /// the page shows no hidden photo's face, so All holds every one it can show unless
    /// the photo has gone since the page was read. The page stays, so closing the viewer
    /// lands back on it.
    fn open_face(&mut self, id: i64) {
        if !self.nav.busy() && self.index.position_of(id).is_some() {
            self.open_viewer(id);
            return;
        }
        if self.nav.target().view != View::All {
            self.ask_step(Step::View(View::All));
        }
        // Looked up in the grid of the step on its way, once it lands, or in the grid on
        // screen when none is.
        if let Some(to) = self.nav.jump(Target::Open(id)) {
            self.arrive(to);
        }
    }

    /// Opens the photo a jump to it found in the grid now on screen, or says it is gone.
    pub(super) fn take_open(&mut self, now: f64) {
        if let Some(id) = self.opening.take() {
            if self.index.position_of(id).is_some() {
                self.open_viewer(id);
            } else {
                self.toasts.done(PHOTO_GONE, now);
            }
        }
    }
}
