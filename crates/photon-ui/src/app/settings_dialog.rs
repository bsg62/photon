//! Settings, on the application's side: opening it, reading what it shows off the
//! window's thread, and doing what is asked in it. The dialog itself is `settings.rs` and
//! `settings_view.rs`; this is where what it asks changes anything, as `app.rs` is for
//! everything else.

use super::{App, Unwritten, Write, Wrote};
use crate::{
    dialogs::Dialog,
    export_dialog,
    settings::{
        About, Asked, Counted, MEMORY_POLL_MS, Memory, Question, Section, Seen, Settings, Stored,
    },
    tag_manager::{Committed, Rename},
    theme, watched_list,
};
use eframe::egui;
use photon_core::library::{TagRule, ThemeChoice, WatchedFolder};
use photon_engine::{commands, engine::Engine};
use std::{collections::HashMap, path::Path, sync::Arc};

/// What Settings reads again at every change of the library's data: each watched
/// folder's count of photos, and the changes made to keywords.
pub(super) struct Lists {
    counts: HashMap<i64, i64>,
    rules: Vec<TagRule>,
}

pub(super) fn lists(engine: &Engine) -> Result<Lists, String> {
    let said = |err: photon_engine::error::AppError| err.message;
    let counts = (commands::watched_folder_stats(engine)
        .map_err(said)?
        .into_iter())
    .map(|stats| (stats.watched_id, stats.photo_count))
    .collect();
    let rules = commands::list_tag_rules(engine).map_err(said)?;
    Ok(Lists { counts, rules })
}

/// Whether the folder whose path is `text` can be offered to be watched: the folder, with
/// a `~` written out, or what is said under the field - the export's field's rules
/// (`check_dest`), the text as it stands first, since a folder's name can end in a space
/// and a dropped one is put in the field as the system gave it. Whether photon will watch
/// it - inside a folder already watched, say - the library says when it is added.
pub(super) fn check_folder(text: &str, home: Option<&Path>) -> Result<String, String> {
    let given = Path::new(text);
    let path = if given.is_absolute() && given.is_dir() {
        given.to_owned()
    } else {
        match export_dialog::typed_path(text, home).unwrap_or(Err(export_dialog::NOT_WHOLE)) {
            Ok(path) => path,
            Err(why) => return Err(why.to_owned()),
        }
    };
    (path.to_str())
        .filter(|_| path.is_dir())
        .map(str::to_owned)
        .ok_or_else(|| export_dialog::NO_FOLDER.to_owned())
}

/// What of the dialog a setting the library refused was sent from, and waits for it.
pub(super) enum Refused {
    /// Nothing waits: the library's words are a message.
    Said,
    /// The interval's field, of the opening numbered so.
    Interval(u64),
    /// The field a folder to add is typed in, by the number it sent the folder under.
    Add { sent: u64, opening: u64 },
    /// The keyword's field, by the number it sent the name under.
    Rename { sent: u64, opening: u64 },
    /// A folder's removal: its row is a row again.
    Remove(i64),
}

/// A setting to store, on the queue of writes.
pub(super) enum SettingWrite {
    /// Applied at the press, and kept for the session if the library will not store it:
    /// going back would answer a disk error with a flash of the whole window.
    Theme(ThemeChoice),
    /// With the opening of Settings it was sent from.
    Interval {
        seconds: i64,
        opening: u64,
    },
    Shuffle(bool),
    Distance(i64),
    FaceDetection(bool),
    /// A watched folder taken out of the library: its files stay where they are.
    RemoveFolder(i64),
    /// A folder to watch, from the field it was typed in, and the number it was sent
    /// under.
    AddFolder {
        path: String,
        sent: u64,
        opening: u64,
    },
    /// A keyword renamed, removed or brought back: rules in the library, which never
    /// touch the photos' own keywords.
    RenameTag {
        rename: Rename,
        opening: u64,
    },
    HideTag(String),
    RestoreTag(String),
}

/// What the library made of one: an interval and a distance are clamped, and the
/// dialog shows what was stored.
pub(super) enum SettingWrote {
    Interval {
        seconds: i64,
        opening: u64,
    },
    Shuffle(bool),
    Distance(i64),
    Stored,
    /// A watched folder removed: the folder list is read again.
    FolderRemoved(i64),
    FolderAdded {
        folder: WatchedFolder,
        sent: u64,
        opening: u64,
    },
    TagRenamed {
        sent: u64,
        opening: u64,
    },
    /// A keyword's rule made or taken back: the lists are read again.
    TagRules,
}

/// Makes `write`, on the queue's thread.
pub(super) fn store(engine: &Arc<Engine>, write: SettingWrite) -> Result<Wrote, Unwritten> {
    let refused = |refused: Refused| {
        move |err: photon_engine::error::AppError| Unwritten::Setting {
            refused,
            said: err.message,
        }
    };
    let wrote = match write {
        SettingWrite::Theme(choice) => commands::set_theme(engine, choice)
            .map(|()| SettingWrote::Stored)
            .map_err(refused(Refused::Said))?,
        SettingWrite::Interval { seconds, opening } => {
            commands::set_slideshow_interval(engine, seconds)
                .map(|seconds| SettingWrote::Interval { seconds, opening })
                .map_err(refused(Refused::Interval(opening)))?
        }
        SettingWrite::Shuffle(shuffle) => commands::set_slideshow_shuffle(engine, shuffle)
            .map(|()| SettingWrote::Shuffle(shuffle))
            .map_err(refused(Refused::Said))?,
        SettingWrite::Distance(distance) => commands::set_similar_distance(engine, distance)
            .map(SettingWrote::Distance)
            .map_err(refused(Refused::Said))?,
        SettingWrite::FaceDetection(on) => commands::set_face_detection(engine, on)
            .map(|()| SettingWrote::Stored)
            .map_err(refused(Refused::Said))?,
        SettingWrite::RemoveFolder(id) => commands::remove_folder(engine, id)
            .map(|()| SettingWrote::FolderRemoved(id))
            .map_err(refused(Refused::Remove(id)))?,
        SettingWrite::AddFolder {
            path,
            sent,
            opening,
        } => commands::add_folder(engine, &path)
            .map(|folder| SettingWrote::FolderAdded {
                folder,
                sent,
                opening,
            })
            .map_err(refused(Refused::Add { sent, opening }))?,
        SettingWrite::RenameTag { rename, opening } => {
            let sent = rename.sent;
            commands::rename_tag(engine, &rename.from, &rename.to)
                .map(|()| SettingWrote::TagRenamed { sent, opening })
                .map_err(refused(Refused::Rename { sent, opening }))?
        }
        SettingWrite::HideTag(tag) => commands::hide_tag(engine, &tag)
            .map(|()| SettingWrote::TagRules)
            .map_err(refused(Refused::Said))?,
        SettingWrite::RestoreTag(tag) => commands::restore_tag_rule(engine, &tag)
            .map(|()| SettingWrote::TagRules)
            .map_err(refused(Refused::Said))?,
    };
    Ok(Wrote::Setting(wrote))
}

/// What the dialog reads as it opens: none of it is shown before it has been read.
pub(super) fn read(engine: &Engine) -> Result<Stored, String> {
    let said = |err: photon_engine::error::AppError| err.message;
    Ok(Stored {
        interval: commands::slideshow_interval(engine).map_err(said)?,
        shuffle: commands::slideshow_shuffle(engine).map_err(said)?,
        distance: commands::similar_distance(engine).map_err(said)?,
    })
}

/// photon's memory, read off the window's thread: on Linux each read walks the whole
/// process table. A read that fails says nothing, rather than a message every two
/// seconds: the figure last read stands, and before one there is no row.
pub(super) fn memory() -> Option<Memory> {
    commands::memory_usage().ok().map(|usage| Memory {
        bytes: usage.bytes,
        processes: usage.processes,
    })
}

/// How many people the user has named: what switching face detection off deletes.
pub(super) fn named(engine: &Engine, opening: u64) -> (u64, Result<i64, String>) {
    let named = commands::face_data_summary(engine)
        .map(|summary| summary.named_people)
        .map_err(|err| err.message);
    (opening, named)
}

impl App {
    /// Opens Settings at `section`.
    pub(super) fn open_settings(&mut self, section: Section) {
        let info = commands::app_info(&self.engine);
        let about = About {
            version: info.version.to_owned(),
            library: info.library_path.display().to_string(),
            licence: info.licence.to_owned(),
        };
        self.settings_read.ask(());
        self.settings_lists.ask(());
        self.memory_asked = None;
        self.settings_opened += 1;
        let mut settings = Settings::new(section, about);
        settings.opening = self.settings_opened;
        self.open_dialog(Dialog::Settings(Box::new(settings)));
    }

    /// A theme pinned against the desktop's is put on the title bar again whenever the
    /// system says the window's theme has changed. winit on Windows does not keep the
    /// theme it is told: at every `WM_SETTINGCHANGE` - which Windows broadcasts for many
    /// changes, the wheel's lines among them - it puts the system's back on the title
    /// bar, and says so as a new theme (read in winit 0.30.13's source, not seen run).
    /// With `System` there is nothing to keep.
    pub(super) fn keep_title_bar(&mut self, ctx: &egui::Context) {
        let system = ctx.input(|input| input.raw.system_theme);
        if system != self.system_theme {
            let first = self.system_theme.is_none();
            self.system_theme = system;
            // The launch has put it already (`App::new`).
            if !first && self.theme != ThemeChoice::System {
                theme::apply::put(ctx, self.theme);
            }
        }
    }

    /// Before Settings is drawn: what it shows that the application holds, and photon's
    /// memory every two seconds while About is shown.
    pub(super) fn settings_frame(&mut self, ctx: &egui::Context, now_ms: f64) {
        let Some(Dialog::Settings(settings)) = &mut self.dialog else {
            return;
        };
        let scans = &self.scans;
        let folders = (self.watched.iter())
            .map(|folder| {
                let count = settings.counts.get(&folder.id).copied();
                let mut row = watched_list::row(
                    folder,
                    scans.seen_by(folder.id),
                    scans.degraded(folder.id),
                    count,
                );
                row.going = self.removing.iter().any(|(going, _)| *going == folder.id);
                row
            })
            .collect();
        settings.seen = Seen {
            theme: self.theme,
            size: self.size,
            find_faces: self.engine.face_detection(),
            faces_line: self.scans.faces_line(),
            folders,
        };
        // Statistics are counted when the section is first shown; a count that failed
        // is asked again when it is left and come back to.
        if settings.section == Section::Statistics {
            if settings.stats == Counted::NotAsked {
                settings.stats = Counted::Counting;
                self.stats_read.ask(settings.opening);
            }
        } else if settings.stats == Counted::Failed {
            settings.stats = Counted::NotAsked;
        }
        if settings.section == Section::About {
            let due = self
                .memory_asked
                .is_none_or(|asked| now_ms - asked >= MEMORY_POLL_MS);
            if due {
                self.memory_asked = Some(now_ms);
                self.memory_read.ask(());
            }
            let asked = self.memory_asked.unwrap_or(now_ms);
            let left = (asked + MEMORY_POLL_MS - now_ms).max(0.0);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left / 1000.0));
        }
    }

    /// What the tasks Settings asked have answered. Whether anything was.
    pub(super) fn settings_answers(&mut self, now_ms: f64) -> bool {
        let mut answered = false;
        if let Some(read) = self.settings_read.answer() {
            answered = true;
            match (read, &mut self.dialog) {
                (Ok(Ok(stored)), Some(Dialog::Settings(settings))) => settings.read(stored),
                (Ok(Err(said)), Some(Dialog::Settings(_))) => self.toasts.error(said, now_ms),
                (Err(_), Some(Dialog::Settings(_))) => {
                    self.toasts
                        .error("photon could not read its settings.", now_ms);
                }
                // Read for a dialog closed since: one opened again has asked again.
                _ => {}
            }
        }
        if let Some(read) = self.settings_lists.answer() {
            answered = true;
            match (read, &mut self.dialog) {
                (Ok(Ok(lists)), Some(Dialog::Settings(settings))) => {
                    settings.counts = lists.counts;
                    settings.rules = Some(lists.rules);
                }
                (Ok(Err(said)), Some(Dialog::Settings(_))) => self.toasts.error(said, now_ms),
                _ => {}
            }
        }
        if let Some(answer) = self.adding_check.answer() {
            answered = true;
            if let (Ok((asked, answer)), Some(Dialog::Settings(settings))) =
                (answer, &mut self.dialog)
                && let Some(adding) = &mut settings.adding
            {
                adding.checked(&asked, answer);
            }
        }
        if let Some(answer) = self.stats_read.answer() {
            answered = true;
            if let Some(Dialog::Settings(settings)) = &mut self.dialog {
                match answer {
                    Ok((opening, Ok(counted))) if opening == settings.opening => {
                        settings.stats = Counted::Counted(Box::new(counted));
                    }
                    Ok((opening, Err(said))) if opening == settings.opening => {
                        settings.stats = Counted::Failed;
                        self.toasts.error(said, now_ms);
                    }
                    Err(_) => settings.stats = Counted::Failed,
                    // Counted for a Settings closed since.
                    Ok(_) => {}
                }
            }
        }
        if let Some(memory) = self.memory_read.answer() {
            answered = true;
            if let Some(Dialog::Settings(settings)) = &mut self.dialog {
                settings.memory = memory.ok().flatten().or(settings.memory);
            }
        }
        if let Some(answer) = self.face_count.answer() {
            answered = true;
            let open = match &mut self.dialog {
                Some(Dialog::Settings(settings)) => Some(settings),
                _ => None,
            };
            match answer.map_err(|_| "photon could not count the people named.".to_owned()) {
                Ok((opening, Ok(named))) => match open {
                    // Asked in the Settings on screen: with people named, its row asks.
                    Some(settings) if settings.opening == opening => {
                        if settings.named(named) {
                            self.push_setting(SettingWrite::FaceDetection(false));
                        }
                    }
                    // Asked in a Settings closed since. The box was unticked, and with
                    // nobody named there is nothing to lose and nothing to ask: it is
                    // switched off, as the Svelte dialog's own switch went on after it
                    // closed. With people named the question cannot be asked where it
                    // was, nor in another Settings, which nobody unticked: nothing.
                    _ => {
                        if named == 0 {
                            self.push_setting(SettingWrite::FaceDetection(false));
                        }
                    }
                },
                Ok((opening, Err(said))) => {
                    if let Some(settings) = open
                        && settings.opening == opening
                    {
                        settings.uncounted();
                    }
                    self.toasts.error(said, now_ms);
                }
                Err(said) => {
                    if let Some(settings) = open {
                        settings.uncounted();
                    }
                    self.toasts.error(said, now_ms);
                }
            }
        }
        answered
    }

    /// What the library made of a setting written.
    pub(super) fn setting_written(&mut self, wrote: SettingWrote) {
        match wrote {
            // The folder list is read again, as after a drop; and Settings' counts.
            SettingWrote::FolderRemoved(id) => {
                // Its row goes when the list read after this answer arrives.
                for (going, answered) in &mut self.removing {
                    if *going == id {
                        *answered = true;
                    }
                }
                self.folder_list.ask(());
                self.settings_lists.ask(());
                return;
            }
            // Watched, and in the list on hand at once, counting as being scanned until
            // its scan has been heard from - an empty library must not say meanwhile that
            // nothing is watched - as a folder dropped on the window is.
            SettingWrote::FolderAdded {
                folder,
                sent,
                opening,
            } => {
                self.scans.added(folder.id);
                if !self.watched.iter().any(|watched| watched.id == folder.id) {
                    self.watched.push(folder.clone());
                }
                // Nothing is said: its row says it, in the list the field stands under, as
                // the Svelte dialog's picker says nothing.
                if let Some(Dialog::Settings(settings)) = &mut self.dialog
                    && settings.opening == opening
                    && (settings.adding.as_ref()).is_some_and(|adding| adding.waits_for(sent))
                {
                    settings.adding = None;
                }
                self.settings_lists.ask(());
                return;
            }
            // The sidebar lists keywords too, and the engine announces a data change only
            // where a grid was rebuilt.
            SettingWrote::TagRules => {
                self.collecting.ask(());
                self.settings_lists.ask(());
                return;
            }
            SettingWrote::TagRenamed { sent, opening } => {
                self.collecting.ask(());
                self.settings_lists.ask(());
                if let Some(Dialog::Settings(settings)) = &mut self.dialog
                    && settings.opening == opening
                {
                    settings.renamer.stored(sent);
                }
                return;
            }
            _ => {}
        }
        let Some(Dialog::Settings(settings)) = &mut self.dialog else {
            return;
        };
        match wrote {
            SettingWrote::Interval { seconds, opening } => {
                settings.interval_stored(seconds, opening)
            }
            SettingWrote::Shuffle(shuffle) => settings.shuffle_stored(shuffle),
            SettingWrote::Distance(distance) => settings.distance_stored(distance),
            SettingWrote::Stored
            | SettingWrote::FolderRemoved(_)
            | SettingWrote::FolderAdded { .. }
            | SettingWrote::TagRenamed { .. }
            | SettingWrote::TagRules => {}
        }
    }

    /// A setting the library would not store. The interval's field shows what the
    /// library holds; a folder's field and a keyword's keep what was typed, the library's
    /// words under them, where they still wait for this answer. Answers the words to say
    /// as a message, where nothing says them.
    pub(super) fn setting_refused(&mut self, refused: Refused, said: String) -> Option<String> {
        let settings = match &mut self.dialog {
            Some(Dialog::Settings(settings)) => Some(settings),
            _ => None,
        };
        match (refused, settings) {
            (Refused::Interval(opening), Some(settings)) => {
                settings.interval_refused(opening);
                Some(said)
            }
            (Refused::Add { sent, opening }, Some(settings))
                if settings.opening == opening
                    && (settings.adding.as_ref()).is_some_and(|adding| adding.waits_for(sent)) =>
            {
                if let Some(adding) = &mut settings.adding {
                    adding.refused(sent, said);
                }
                None
            }
            (Refused::Rename { sent, opening }, Some(settings)) if settings.opening == opening => {
                settings.renamer.refused(sent, said);
                None
            }
            (Refused::Remove(id), _) => {
                self.removing.retain(|(going, _)| *going != id);
                Some(said)
            }
            _ => Some(said),
        }
    }

    fn push_setting(&mut self, write: SettingWrite) {
        self.writes.push(Write::Setting(write));
    }

    /// Does what was asked in Settings.
    pub(super) fn settings_asked(&mut self, ctx: &egui::Context, asked: Vec<Asked>) {
        for asked in asked {
            if asked == Asked::Close {
                if let Some(Dialog::Settings(settings)) = self.dialog.take() {
                    self.settings_closed(*settings);
                }
                return;
            }
            // A row of Statistics: Settings closes and the row is searched for, as a
            // sidebar row's search is - the search box told first.
            if let Asked::Search(query) = asked {
                if let Some(Dialog::Settings(settings)) = self.dialog.take() {
                    self.settings_closed(*settings);
                }
                self.go(crate::nav::Step::Search(query));
                return;
            }
            let Some(Dialog::Settings(settings)) = &mut self.dialog else {
                return;
            };
            match asked {
                Asked::Section(section) => {
                    settings.section = section;
                    // About reads the memory as it is shown, not two seconds later.
                    self.memory_asked = None;
                }
                // Put on the window at once, and stored after.
                Asked::Theme(choice) => {
                    self.theme = choice;
                    theme::apply::put(ctx, choice);
                    self.push_setting(SettingWrite::Theme(choice));
                }
                // The top bar's own control, again.
                Asked::Size(size) => {
                    self.size = size;
                    self.writes.push(Write::TileSize(size));
                }
                Asked::Interval { text, store } => {
                    settings.type_interval(&text);
                    if store && let Some(seconds) = settings.commit_interval() {
                        let opening = settings.opening;
                        self.push_setting(SettingWrite::Interval { seconds, opening });
                    }
                }
                Asked::Shuffle(shuffle) => self.push_setting(SettingWrite::Shuffle(shuffle)),
                Asked::Distance(distance) => self.push_setting(SettingWrite::Distance(distance)),
                Asked::FindFaces(true) => self.push_setting(SettingWrite::FaceDetection(true)),
                // Off deletes the people the user named: the library is asked how many
                // there are first, and with any the row asks.
                Asked::FindFaces(false) => {
                    if settings.ask_off() {
                        self.face_count.ask(settings.opening);
                    }
                }
                // The question in a row answered: what it asked about is done, or nothing.
                Asked::Yes => {
                    let opening = settings.opening;
                    match settings.answered() {
                        Some(Question::SwitchOff(_)) => {
                            self.push_setting(SettingWrite::FaceDetection(false));
                        }
                        Some(Question::RemoveFolder { id, .. }) => {
                            self.removing.push((id, false));
                            self.push_setting(SettingWrite::RemoveFolder(id));
                        }
                        Some(Question::RemoveTag(tag)) => {
                            self.push_setting(SettingWrite::HideTag(tag));
                        }
                        Some(Question::Rename(_)) => {
                            if let Some(rename) = settings.renamer.agreed() {
                                self.push_setting(SettingWrite::RenameTag { rename, opening });
                            }
                        }
                        None => {}
                    }
                }
                Asked::No => {
                    if let Some(Question::Rename(_)) = settings.answered() {
                        settings.renamer.declined();
                    }
                }
                Asked::Rescan(watched) => {
                    self.writes.push(Write::Rescan { watched });
                }
                Asked::RemoveFolder { id, path } => {
                    settings.ask(Question::RemoveFolder { id, path });
                }
                Asked::AddOpen => {
                    settings.adding = Some(Default::default());
                }
                Asked::AddPath(text) => {
                    if let Some(ask) =
                        (settings.adding.as_mut()).and_then(|adding| adding.type_in(text))
                    {
                        self.adding_check.ask(ask);
                    }
                }
                Asked::AddSend => {
                    let opening = settings.opening;
                    if let Some((path, sent)) =
                        settings.adding.as_mut().and_then(|adding| adding.send())
                    {
                        self.push_setting(SettingWrite::AddFolder {
                            path,
                            sent,
                            opening,
                        });
                    }
                }
                Asked::AddClose => {
                    settings.adding = None;
                }
                Asked::Filter(text) => {
                    settings.filter = text;
                }
                Asked::RenameStart(tag) => {
                    settings.let_go_of_question();
                    settings.renamer.start(&tag);
                }
                Asked::RenameType(text) => settings.renamer.type_in(&text),
                Asked::RenameCancel => settings.renamer.cancel(),
                Asked::RenameCommit => {
                    let tags = &self.held.collections().tags;
                    let rules = settings.rules.as_deref().unwrap_or_default();
                    match settings.renamer.commit(tags, rules) {
                        Committed::Nothing => {}
                        Committed::Asks(words) => settings.ask(Question::Rename(words)),
                        Committed::Store(rename) => {
                            let opening = settings.opening;
                            self.push_setting(SettingWrite::RenameTag { rename, opening });
                        }
                    }
                }
                Asked::RemoveTag(tag) => settings.ask(Question::RemoveTag(tag)),
                Asked::Restore(tag) => self.push_setting(SettingWrite::RestoreTag(tag)),
                // (Taken before the dialog is borrowed, above.)
                Asked::Close | Asked::Search(_) => {}
            }
        }
    }

    /// Settings closed: an interval typed and not yet stored is stored, as leaving its
    /// field would have stored it - Escape closes the dialog from inside the field.
    pub(super) fn settings_closed(&mut self, mut settings: Settings) {
        if let Some(seconds) = settings.commit_interval() {
            let opening = settings.opening;
            self.push_setting(SettingWrite::Interval { seconds, opening });
        }
    }
}
