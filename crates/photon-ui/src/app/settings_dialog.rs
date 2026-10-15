//! Settings, on the application's side: opening it, reading what it shows off the
//! window's thread, and doing what is asked in it. The dialog itself is `settings.rs` and
//! `settings_view.rs`; this is where what it asks changes anything, as `app.rs` is for
//! everything else.

use super::{App, Unwritten, Write, Wrote};
use crate::{
    dialogs::Dialog,
    settings::{About, Asked, MEMORY_POLL_MS, Memory, Section, Seen, Settings, Stored},
    theme,
};
use eframe::egui;
use photon_core::library::ThemeChoice;
use photon_engine::{commands, engine::Engine};
use std::sync::Arc;

/// A setting to store, on the queue of writes.
pub(super) enum SettingWrite {
    /// Applied at the press, and kept for the session if the library will not store it:
    /// going back would answer a disk error with a flash of the whole window.
    Theme(ThemeChoice),
    Interval(i64),
    Shuffle(bool),
    Distance(i64),
    FaceDetection(bool),
}

/// What the library made of one: an interval and a distance are clamped, and the
/// dialog shows what was stored.
pub(super) enum SettingWrote {
    Interval(i64),
    Shuffle(bool),
    Distance(i64),
    Stored,
}

/// Makes `write`, on the queue's thread.
pub(super) fn store(engine: &Arc<Engine>, write: SettingWrite) -> Result<Wrote, Unwritten> {
    let refused = |interval: bool| {
        move |err: photon_engine::error::AppError| Unwritten::Setting {
            interval,
            said: err.message,
        }
    };
    let wrote = match write {
        SettingWrite::Theme(choice) => commands::set_theme(engine, choice)
            .map(|()| SettingWrote::Stored)
            .map_err(refused(false))?,
        SettingWrite::Interval(seconds) => commands::set_slideshow_interval(engine, seconds)
            .map(SettingWrote::Interval)
            .map_err(refused(true))?,
        SettingWrite::Shuffle(shuffle) => commands::set_slideshow_shuffle(engine, shuffle)
            .map(|()| SettingWrote::Shuffle(shuffle))
            .map_err(refused(false))?,
        SettingWrite::Distance(distance) => commands::set_similar_distance(engine, distance)
            .map(SettingWrote::Distance)
            .map_err(refused(false))?,
        SettingWrite::FaceDetection(on) => commands::set_face_detection(engine, on)
            .map(|()| SettingWrote::Stored)
            .map_err(refused(false))?,
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
/// process table. A read that fails leaves the row out, rather than a message every two
/// seconds.
pub(super) fn memory() -> Option<Memory> {
    commands::memory_usage().ok().map(|usage| Memory {
        bytes: usage.bytes,
        processes: usage.processes,
    })
}

/// How many people the user has named: what switching face detection off deletes.
pub(super) fn named(engine: &Engine) -> Result<i64, String> {
    commands::face_data_summary(engine)
        .map(|summary| summary.named_people)
        .map_err(|err| err.message)
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
        self.memory_asked = None;
        self.open_dialog(Dialog::Settings(Settings::new(section, about)));
    }

    /// Before Settings is drawn: what it shows that the application holds, and photon's
    /// memory every two seconds while About is shown.
    pub(super) fn settings_frame(&mut self, ctx: &egui::Context, now_ms: f64) {
        let Some(Dialog::Settings(settings)) = &mut self.dialog else {
            return;
        };
        settings.seen = Seen {
            theme: self.theme,
            size: self.size,
            find_faces: self.engine.face_detection(),
            faces_line: self.scans.faces_line(),
        };
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
        if let Some(memory) = self.memory_read.answer() {
            answered = true;
            if let Some(Dialog::Settings(settings)) = &mut self.dialog {
                settings.memory = memory.ok().flatten().or(settings.memory);
            }
        }
        if let Some(named) = self.face_count.answer() {
            answered = true;
            if let Some(Dialog::Settings(settings)) = &mut self.dialog {
                match named.map_err(|_| "photon could not count the people named.".to_owned()) {
                    Ok(Ok(named)) => {
                        if settings.named(named) {
                            self.push_setting(SettingWrite::FaceDetection(false));
                        }
                    }
                    Ok(Err(said)) | Err(said) => {
                        settings.uncounted();
                        self.toasts.error(said, now_ms);
                    }
                }
            }
        }
        answered
    }

    /// What the library made of a setting written.
    pub(super) fn setting_written(&mut self, wrote: SettingWrote) {
        let Some(Dialog::Settings(settings)) = &mut self.dialog else {
            return;
        };
        match wrote {
            SettingWrote::Interval(seconds) => settings.interval_stored(seconds),
            SettingWrote::Shuffle(shuffle) => settings.shuffle_stored(shuffle),
            SettingWrote::Distance(distance) => settings.distance_stored(distance),
            SettingWrote::Stored => {}
        }
    }

    /// A setting the library would not store: the interval's field shows what it holds.
    pub(super) fn setting_refused(&mut self, interval: bool) {
        if interval && let Some(Dialog::Settings(settings)) = &mut self.dialog {
            settings.interval_refused();
        }
    }

    fn push_setting(&mut self, write: SettingWrite) {
        self.writes.push(Write::Setting(write));
    }

    /// Does what was asked in Settings.
    pub(super) fn settings_asked(&mut self, ctx: &egui::Context, asked: Vec<Asked>) {
        for asked in asked {
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
                        self.push_setting(SettingWrite::Interval(seconds));
                    }
                }
                Asked::Shuffle(shuffle) => self.push_setting(SettingWrite::Shuffle(shuffle)),
                Asked::Distance(distance) => self.push_setting(SettingWrite::Distance(distance)),
                Asked::FindFaces(true) => self.push_setting(SettingWrite::FaceDetection(true)),
                // Off deletes the people the user named: the library is asked how many
                // there are first, and with any the row asks.
                Asked::FindFaces(false) => {
                    if settings.ask_off() {
                        self.face_count.ask(());
                    }
                }
                Asked::SwitchOff => {
                    settings.answered();
                    self.push_setting(SettingWrite::FaceDetection(false));
                }
                Asked::KeepFaces => settings.answered(),
            }
        }
    }

    /// Settings closed: an interval typed and not yet stored is stored, as leaving its
    /// field would have stored it - Escape closes the dialog from inside the field.
    pub(super) fn settings_closed(&mut self, mut settings: Settings) {
        if let Some(seconds) = settings.commit_interval() {
            self.push_setting(SettingWrite::Interval(seconds));
        }
    }
}
