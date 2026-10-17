//! The People page, on the application's side: opening and leaving it, reading it off the
//! window's thread, its crops, and doing what is pressed on it. The page itself is
//! `people_page.rs` and `people_view.rs`.
//!
//! **The page is not a view of the grid** (`mainPage` in the Svelte UI): it has no rows, so
//! the views' queue, `Nav` and the selection know nothing of it. It is drawn over the
//! grid's area, the sidebar and the top bar staying, and the grid is not drawn under it -
//! it keeps its place, which is a number of its own, so coming back finds it where it was.

use super::{App, MenuOf, View, Write};
use crate::{
    dialogs::{Confirm, Dialog},
    nav::{Step, Target},
    people_page::{
        Ask, FaceAction, FacesWrite, Loaded, MORE, More, PeoplePage, STALE_GROUP, STRIP, group_in,
    },
    people_view::PeopleAction,
    settings::Section,
};
use eframe::egui;
use photon_core::library::PageFace;
use photon_engine::{commands, engine::Engine, error::AppError};

/// What "Show more" fetched: what was asked, and the faces or why not.
pub(super) type Fetched = (More, Result<Vec<PageFace>, String>);

pub(super) fn more(engine: &Engine, more: More) -> Fetched {
    let faces = commands::person_faces(engine, more.group, more.filter, more.offset, more.limit)
        .map_err(|err| err.message);
    (more, faces)
}

/// What a write of the page's that the library refused is said as: one naming a group or
/// person that is gone in words the user can act on - a grouping run emptied and deleted
/// it, another write merged it away, and the library's "that is not a named person" reads
/// as the user's mistake - and anything else as the library put it.
pub(super) fn said(err: AppError) -> String {
    if err.kind == "notAPerson" {
        STALE_GROUP.to_owned()
    } else {
        err.message
    }
}

/// Writes faces of the page: an action of a strip's bar, or single faces named - the first
/// one's group named, and the others' merged into the person that made.
pub(super) fn write_faces(
    engine: &std::sync::Arc<Engine>,
    write: FacesWrite,
) -> Result<(), AppError> {
    match write {
        FacesWrite::Act { ids, action } => match action {
            FaceAction::Confirm => commands::confirm_faces(engine, &ids),
            FaceAction::Reject => commands::reject_faces(engine, &ids),
            FaceAction::Ignore => commands::ignore_faces(engine, &ids, true),
            FaceAction::Unignore => commands::ignore_faces(engine, &ids, false),
        },
        FacesWrite::NameSingles { groups, name, .. } => {
            let Some((first, rest)) = groups.split_first() else {
                return Ok(());
            };
            let person = commands::name_person(engine, *first, &name)?;
            for group in rest {
                commands::merge_people(engine, *group, person)?;
            }
            Ok(())
        }
    }
}

/// The page as the library answered a read: the number it was asked under, the page with
/// what "Show more" had loaded fetched again, and what could not be fetched.
pub(super) type Read = (u64, Result<Loaded, String>, Vec<String>);

/// Reads the page, then each strip's faces "Show more" had loaded, after the strip's
/// first ones in the page just read, a page of `MORE` at a time until a short page says
/// there are no more. A strip whose fetch fails is left out - it folds back to its first
/// faces - and said; the page itself still loads.
pub(super) fn read(engine: &Engine, ask: Ask) -> Read {
    let page = match commands::people_page(engine, STRIP) {
        Ok(page) => page,
        Err(err) => return (ask.number, Err(err.message), Vec::new()),
    };
    let mut kept = std::collections::HashMap::new();
    let mut failed = Vec::new();
    for (strip, want) in ask.wants {
        let Some(group) = group_in(&page, strip) else {
            continue;
        };
        let mut got = Vec::new();
        let fetched = loop {
            if got.len() >= want {
                break Ok(());
            }
            let limit = MORE.min(want - got.len());
            let offset = group.faces.len() + got.len();
            match commands::person_faces(engine, group.id, strip.filter(), offset, limit) {
                Ok(faces) => {
                    let short = faces.len() < limit;
                    got.extend(faces);
                    if short {
                        break Ok(());
                    }
                }
                Err(err) => break Err(err.message),
            }
        };
        match fetched {
            Ok(()) => {
                kept.insert(strip, got);
            }
            Err(said) => failed.push(said),
        }
    }
    (ask.number, Ok(Loaded { page, kept }), failed)
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

    /// How far the page is scrolled, and whether its scrollbar is held: for a test.
    pub fn people_scrolled(&self) -> (f32, bool) {
        self.people_view.scrolled()
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
        self.people_openings += 1;
        let mut model = PeoplePage::new(self.people_openings);
        let ask = model.reload(now);
        self.people_read.ask(ask);
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
                (Ok((number, Ok(loaded), failed)), Some(model)) => {
                    model.loaded(number, loaded);
                    for said in failed {
                        self.toasts.error(said, now);
                    }
                }
                (Ok((_, Err(said), _)), Some(_)) => self.toasts.error(said, now),
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
                && let Some(ask) = model.changed(now)
            {
                self.people_read.ask(ask);
            }
            if let Some(ask) = model.poll(now) {
                self.people_read.ask(ask);
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
            let ask = model.reload(now);
            self.people_read.ask(ask);
        }
    }

    /// Faces the page wrote, or the library refused: put back if refused, and the page
    /// read again either way.
    pub(super) fn faces_written(&mut self, ids: &[i64], written: bool, now: f64) {
        if let Some(model) = &mut self.page {
            model.faces_done(ids, written);
        }
        self.read_people(now);
    }

    /// The page read again, after one of its writes: at once.
    pub(super) fn read_people(&mut self, now: f64) {
        if let Some(model) = &mut self.page {
            let ask = model.reload(now);
            self.people_read.ask(ask);
        }
    }

    /// The pages "Show more" fetched.
    pub(super) fn more_answers(&mut self, now: f64) -> bool {
        let mut answered = false;
        for (_, answer) in self.people_more.answers() {
            answered = true;
            match (answer, &mut self.page) {
                (Ok((more, Ok(faces))), Some(model)) => model.more_landed(more, faces),
                (Ok((more, Err(said))), Some(model)) => {
                    model.more_failed(more);
                    self.toasts.error(said, now);
                }
                _ => {}
            }
        }
        answered
    }

    /// Merges the person "Merge into…" was opened on into `into`, once asked.
    pub(super) fn merge_into(&mut self, of: MenuOf, into: i64) {
        let (MenuOf::Merge(from), Some(model)) = (of, &self.page) else {
            return;
        };
        let (Some(from_name), Some(into_name)) = (model.name_of(from), model.name_of(into)) else {
            return;
        };
        let confirm = Confirm::Merge {
            from,
            from_name: from_name.to_owned(),
            into,
            into_name: into_name.to_owned(),
        };
        self.open_dialog(Dialog::Confirm(confirm));
    }

    /// Does what was pressed on the page.
    pub(super) fn people_act(&mut self, ctx: &egui::Context, action: PeopleAction) {
        let Some(model) = &mut self.page else {
            return;
        };
        match action {
            PeopleAction::Name { group, name } => {
                model.naming(group);
                self.write(Write::NameGroup { group, name });
            }
            PeopleAction::Open(id) => self.open_face(id),
            PeopleAction::OpenSettings => self.open_settings(Section::People),
            PeopleAction::Toggle { strip, face } => model.toggle(strip, face),
            PeopleAction::Clear => model.clear_selection(),
            PeopleAction::Act { strip, action } => {
                if let Some(write) = model.act(strip, action) {
                    self.write(Write::Faces(write));
                }
            }
            PeopleAction::NameSingles(typed) => {
                if let Some(write) = model.name_singles(&typed) {
                    self.write(Write::Faces(write));
                }
            }
            PeopleAction::ConfirmAll(person) => {
                if let Some(write) = model.confirm_all(person) {
                    self.write(Write::Faces(write));
                }
            }
            PeopleAction::ShowMore(strip) => {
                if let Some(more) = model.show_more(strip) {
                    self.people_more.push(more);
                }
            }
            PeopleAction::ShowFewer(strip) => model.show_fewer(strip),
            PeopleAction::Rename { person, name } => {
                if let Some(name) = model.rename(person, &name) {
                    self.write(Write::Rename { person, name });
                }
            }
            PeopleAction::Merge { from, at } => self.open_menu(ctx, at, MenuOf::Merge(from)),
            PeopleAction::Delete(person) => {
                if let Some(name) = model.name_of(person) {
                    let confirm = Confirm::Person {
                        id: person,
                        name: name.to_owned(),
                    };
                    self.open_dialog(Dialog::Confirm(confirm));
                }
            }
            PeopleAction::Ignore { group, ignored } => {
                model.naming(group);
                self.write(Write::GroupIgnored { group, ignored });
            }
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

    /// Opens the photo a jump to it found in the grid now on screen, or says it is gone -
    /// while the page it was asked from is still open: left meanwhile for another view, the
    /// viewer would open over a grid the user went elsewhere from.
    pub(super) fn take_open(&mut self, now: f64) {
        if let Some(id) = self.opening.take()
            && self.page.is_some()
        {
            if self.index.position_of(id).is_some() {
                self.open_viewer(id);
            } else {
                self.toasts.done(PHOTO_GONE, now);
            }
        }
    }
}
