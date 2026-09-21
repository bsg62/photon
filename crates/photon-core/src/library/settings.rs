//! Small pieces of UI state that have to outlive the process.
//!
//! Four things so far — the folder the grid was last showing, how long a slideshow holds
//! each photo, whether the thumbnail cache needs collecting, and which colour scheme the UI
//! uses — but the table is generic because a column per setting would mean
//! a migration per setting.

use super::Library;
use crate::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The folder whose section was at the top of the grid when photon last closed.
const LAST_FOLDER: &str = "last_folder";

/// How long a slideshow shows each photo, in whole seconds.
const SLIDESHOW_INTERVAL_S: &str = "slideshow_interval_s";
/// What a slideshow uses until the user says otherwise.
pub const SLIDESHOW_INTERVAL_DEFAULT_S: i64 = 4;
/// The shortest and longest interval accepted. Under a second a full-size photo may not
/// have decoded before it is replaced; past a minute it reads as stuck.
pub const SLIDESHOW_INTERVAL_RANGE_S: std::ops::RangeInclusive<i64> = 1..=60;

/// Which colour scheme the UI uses.
const THEME: &str = "theme";

/// Whether an export writes edited photos as they are shown. Remembered because it is a
/// choice about how the user works, not about one export.
const EXPORT_APPLY_EDITS: &str = "export_apply_edits";

/// How large the grid draws its tiles. Stored as the step's name rather than its pixel
/// width, so changing what "Large" measures does not have to migrate anyone's setting.
const GRID_TILE: &str = "grid_tile";

/// How far apart two perceptual hashes may be and still count as the same picture:
/// 0 off, 3 conservative, 6 loose. Stored as the distance itself rather than a name,
/// because the distance is what the pass uses and a name would need a second table to
/// interpret it.
const SIMILAR_DISTANCE: &str = "similar_distance";
/// Conservative: the distance at which grouping has complete recall. Derived from that
/// constant rather than written as 3 beside it - the default *is* that fact, and two copies
/// of it could be changed apart.
pub const SIMILAR_DISTANCE_DEFAULT: i64 = crate::similar::EXACT_RECALL_DISTANCE as i64;
/// Off, conservative, loose. Clamped rather than refused, both ways.
pub const SIMILAR_DISTANCE_RANGE: std::ops::RangeInclusive<i64> = 0..=6;

/// The user's colour scheme: the desktop's, or one of the two pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Anything unrecognised is `System`: the table is plain text a newer photon may have
    /// written, and following the desktop is the one answer that is never wrong.
    fn parse(stored: &str) -> Self {
        match stored {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

/// How large the grid draws its tiles. The widths themselves live in the UI
/// (`ui/src/lib/layout.ts`), because they are a layout fact, not a stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GridTile {
    Small,
    #[default]
    Medium,
    Large,
}

impl GridTile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    fn parse(stored: &str) -> Self {
        match stored {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }
}

/// Bumped by every write that can leave a thumbnail with no item: a purge, a replaced row
/// (its fingerprint changes with the file), a removed watched folder. Compared against
/// [`THUMB_GC_CLEAN_EPOCH`] to decide whether the cache walk is worth doing.
const THUMB_GC_EPOCH: &str = "thumb_gc_epoch";
/// The value of [`THUMB_GC_EPOCH`] the last completed collection was started against.
const THUMB_GC_CLEAN_EPOCH: &str = "thumb_gc_clean_epoch";
/// When the last collection finished, in milliseconds since the epoch.
const THUMB_GC_AT: &str = "thumb_gc_at";

/// Writes a setting on `conn`, replacing any previous value. A free function over the
/// connection so a caller inside its own transaction can use it too.
fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

/// Marks the thumbnail cache as possibly holding garbage. Takes the connection rather than
/// `&Library` so the callers that orphan thumbnails can do it inside their own transaction:
/// a purge that committed without its bump would leave garbage nothing will ever look for.
pub(crate) fn bump_thumb_gc_epoch(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, '1')
         ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
        [THUMB_GC_EPOCH],
    )?;
    Ok(())
}

fn clamp_interval(seconds: i64) -> i64 {
    seconds.clamp(
        *SLIDESHOW_INTERVAL_RANGE_S.start(),
        *SLIDESHOW_INTERVAL_RANGE_S.end(),
    )
}

fn clamp_distance(distance: i64) -> i64 {
    distance.clamp(
        *SIMILAR_DISTANCE_RANGE.start(),
        *SIMILAR_DISTANCE_RANGE.end(),
    )
}

impl Library {
    /// Reads a setting, or `None` when it has never been written.
    fn setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.reader()?;
        let value = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .ok();
        Ok(value)
    }

    /// Writes a setting, replacing any previous value.
    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        set_setting(&self.writer(), key, value)?;
        Ok(())
    }

    /// The folder to scroll back to on launch, or `None` when there is nothing to restore.
    ///
    /// A folder that no longer exists is reported as `None` rather than handed to the UI to
    /// fail on: a watched folder can be removed, or a directory deleted, between two runs,
    /// and the stored id then points at a row that has been cascaded away. The check is a
    /// join here rather than a lookup in the caller because only the database knows.
    pub fn last_folder(&self) -> Result<Option<i64>> {
        let Some(id) = self.setting_i64(LAST_FOLDER)? else {
            return Ok(None);
        };
        let conn = self.reader()?;
        let exists = conn
            .query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(()))
            .is_ok();
        Ok(exists.then_some(id))
    }

    /// Records the folder the grid is showing, for the next launch.
    pub fn set_last_folder(&self, folder_id: i64) -> Result<()> {
        self.set_setting(LAST_FOLDER, &folder_id.to_string())
    }

    /// Seconds a slideshow holds each photo. The default when never set, and clamped on
    /// the way out as well as on the way in: the table is plain text a newer or older
    /// photon may have written, and a zero here would spin the slideshow.
    pub fn slideshow_interval_s(&self) -> Result<i64> {
        let stored = self
            .setting_i64(SLIDESHOW_INTERVAL_S)?
            .unwrap_or(SLIDESHOW_INTERVAL_DEFAULT_S);
        Ok(clamp_interval(stored))
    }

    /// Stores the slideshow interval, clamped, and returns what was stored so the caller
    /// can show the value in force rather than the one asked for.
    pub fn set_slideshow_interval_s(&self, seconds: i64) -> Result<i64> {
        let seconds = clamp_interval(seconds);
        self.set_setting(SLIDESHOW_INTERVAL_S, &seconds.to_string())?;
        Ok(seconds)
    }

    /// Whether an export renders edits into the copies. True until the user says otherwise:
    /// an untouched photo is a byte copy either way, so this only decides what happens to
    /// photos the user has deliberately edited - and there, what they see is what they
    /// asked for.
    pub fn export_apply_edits(&self) -> Result<bool> {
        Ok(self
            .setting(EXPORT_APPLY_EDITS)?
            .is_none_or(|stored| stored == "1"))
    }

    pub fn set_export_apply_edits(&self, apply: bool) -> Result<()> {
        self.set_setting(EXPORT_APPLY_EDITS, if apply { "1" } else { "0" })
    }

    /// The colour scheme the user chose; `System` when never set.
    pub fn theme(&self) -> Result<ThemeChoice> {
        Ok(self
            .setting(THEME)?
            .map_or(ThemeChoice::System, |stored| ThemeChoice::parse(&stored)))
    }

    /// Stores the colour scheme.
    pub fn set_theme(&self, choice: ThemeChoice) -> Result<()> {
        self.set_setting(THEME, choice.as_str())
    }

    /// How large the grid draws its tiles; `Medium` when never set.
    pub fn grid_tile(&self) -> Result<GridTile> {
        Ok(self
            .setting(GRID_TILE)?
            .map_or(GridTile::Medium, |stored| GridTile::parse(&stored)))
    }

    /// Stores the grid's tile size.
    pub fn set_grid_tile(&self, tile: GridTile) -> Result<()> {
        self.set_setting(GRID_TILE, tile.as_str())
    }

    /// How far apart two perceptual hashes may be and still be grouped, in force right now.
    /// Conservative when never set, and clamped on the way out as well as on the way in:
    /// the table is plain text an older or newer photon may have written.
    pub fn similar_distance(&self) -> Result<i64> {
        let stored = self
            .setting_i64(SIMILAR_DISTANCE)?
            .unwrap_or(SIMILAR_DISTANCE_DEFAULT);
        Ok(clamp_distance(stored))
    }

    /// Stores the look-alike distance, clamped, and returns what was stored so the caller
    /// can show the value in force rather than the one asked for.
    pub fn set_similar_distance(&self, distance: i64) -> Result<i64> {
        let distance = clamp_distance(distance);
        self.set_setting(SIMILAR_DISTANCE, &distance.to_string())?;
        Ok(distance)
    }

    fn setting_i64(&self, key: &str) -> Result<Option<i64>> {
        Ok(self.setting(key)?.and_then(|v| v.parse().ok()))
    }

    /// Whether the thumbnail cache is worth walking, and if so the epoch to report back to
    /// [`Library::thumb_gc_done`] once the walk is over.
    ///
    /// Due when something has orphaned a thumbnail since the last collection, when there
    /// has never been one, or when the last one is older than `max_age`. The age rule
    /// exists for the one kind of garbage no write can announce: a temp file left by a
    /// process killed mid-write. Without it the walk over every cached file, two per photo,
    /// ran on every launch to find, almost always, nothing.
    pub fn thumb_gc_due(&self, now_ms: i64, max_age: Duration) -> Result<Option<i64>> {
        let epoch = self.setting_i64(THUMB_GC_EPOCH)?.unwrap_or(0);
        let clean = self.setting_i64(THUMB_GC_CLEAN_EPOCH)?;
        let at = self.setting_i64(THUMB_GC_AT)?;
        let stale = match (clean, at) {
            (Some(clean), Some(at)) => {
                clean != epoch || now_ms.saturating_sub(at) > max_age.as_millis() as i64
            }
            _ => true,
        };
        Ok(stale.then_some(epoch))
    }

    /// Records a finished collection that was started against `epoch`. Garbage made while
    /// the walk was running bumped the epoch past this value, so the next `thumb_gc_due`
    /// still reports it rather than believing the cache clean.
    pub fn thumb_gc_done(&self, epoch: i64, now_ms: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        for (key, value) in [(THUMB_GC_CLEAN_EPOCH, epoch), (THUMB_GC_AT, now_ms)] {
            set_setting(&tx, key, &value.to_string())?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);

    #[test]
    fn the_slideshow_interval_defaults_persists_and_is_clamped_both_ways() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.slideshow_interval_s().unwrap(), 4);
        assert_eq!(lib.set_slideshow_interval_s(9).unwrap(), 9);
        assert_eq!(lib.slideshow_interval_s().unwrap(), 9);
        assert_eq!(lib.set_slideshow_interval_s(0).unwrap(), 1);
        assert_eq!(lib.set_slideshow_interval_s(3600).unwrap(), 60);
        assert_eq!(lib.slideshow_interval_s().unwrap(), 60);
        // A value written by something other than the setter is clamped on read.
        lib.set_setting(SLIDESHOW_INTERVAL_S, "0").unwrap();
        assert_eq!(lib.slideshow_interval_s().unwrap(), 1);
    }

    #[test]
    fn the_similar_distance_defaults_persists_and_is_clamped_both_ways() {
        let (_dir, lib) = temp_library();
        assert_eq!(
            lib.similar_distance().unwrap(),
            3,
            "conservative is the default"
        );
        assert_eq!(lib.set_similar_distance(6).unwrap(), 6);
        assert_eq!(lib.similar_distance().unwrap(), 6);
        assert_eq!(
            lib.set_similar_distance(0).unwrap(),
            0,
            "off is a real choice"
        );
        assert_eq!(lib.set_similar_distance(99).unwrap(), 6, "clamped to loose");
        assert_eq!(lib.set_similar_distance(-1).unwrap(), 0, "clamped to off");
        // Written by something other than the setter - clamped on read, as the table is
        // plain text an older or newer photon may have written.
        lib.set_setting(SIMILAR_DISTANCE, "40").unwrap();
        assert_eq!(lib.similar_distance().unwrap(), 6);
    }

    #[test]
    fn the_theme_defaults_to_system_and_round_trips() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.theme().unwrap(), ThemeChoice::System);
        for choice in [ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::System] {
            lib.set_theme(choice).unwrap();
            assert_eq!(lib.theme().unwrap(), choice);
        }
    }

    #[test]
    fn a_theme_this_photon_does_not_know_reads_as_system() {
        let (_dir, lib) = temp_library();
        lib.set_theme(ThemeChoice::Dark).unwrap();
        // Written by a newer photon, or by hand.
        lib.set_setting(THEME, "sepia").unwrap();
        assert_eq!(lib.theme().unwrap(), ThemeChoice::System);
    }

    #[test]
    fn a_theme_choice_crosses_ipc_in_lowercase() {
        assert_eq!(
            serde_json::to_string(&ThemeChoice::System).unwrap(),
            "\"system\""
        );
        assert_eq!(
            serde_json::from_str::<ThemeChoice>("\"dark\"").unwrap(),
            ThemeChoice::Dark
        );
    }

    #[test]
    fn a_fresh_library_is_due_for_thumbnail_gc() {
        let (_dir, lib) = temp_library();
        assert!(lib.thumb_gc_due(1_000, WEEK).unwrap().is_some());
    }

    /// Each of the four writes that can orphan a thumbnail makes the next collection due,
    /// and nothing else does: a collection with nothing to find is the walk this exists to
    /// avoid.
    #[test]
    fn gc_is_due_again_only_after_a_write_that_can_orphan_a_thumbnail() {
        let (_dir, lib) = temp_library();
        let (watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        let settle = |lib: &Library| {
            let epoch = lib.thumb_gc_due(1_000, WEEK).unwrap().unwrap();
            lib.thumb_gc_done(epoch, 1_000).unwrap();
            assert_eq!(lib.thumb_gc_due(1_000, WEEK).unwrap(), None);
        };
        settle(&lib);

        lib.mark_missing(&[ids[0]], 5).unwrap();
        lib.set_ratings(&[(ids[1], 1)]).unwrap();
        assert_eq!(
            lib.thumb_gc_due(1_000, WEEK).unwrap(),
            None,
            "marking missing and rating leave every thumbnail attached to its item"
        );

        lib.purge_items(&[ids[0]]).unwrap();
        assert!(lib.thumb_gc_due(1_000, WEEK).unwrap().is_some(), "purge");
        settle(&lib);

        lib.update_items(&[(ids[1], new_item(folder, "/p/b.jpg", 2))])
            .unwrap();
        assert!(lib.thumb_gc_due(1_000, WEEK).unwrap().is_some(), "replace");
        settle(&lib);

        let turned = crate::edit::Edit::new(1, None).unwrap();
        lib.set_item_edit(ids[1], turned).unwrap();
        assert!(lib.thumb_gc_due(1_000, WEEK).unwrap().is_some(), "an edit");
        settle(&lib);
        lib.set_item_edit(ids[1], turned).unwrap();
        assert_eq!(
            lib.thumb_gc_due(1_000, WEEK).unwrap(),
            None,
            "the same edit again orphans nothing"
        );

        lib.remove_watched_folder(watched).unwrap();
        assert!(
            lib.thumb_gc_due(1_000, WEEK).unwrap().is_some(),
            "removing a watched folder"
        );
    }

    #[test]
    fn garbage_made_while_a_collection_runs_keeps_the_next_one_due() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        let epoch = lib.thumb_gc_due(1_000, WEEK).unwrap().unwrap();

        // A scan purges something after the walk read its fingerprints.
        lib.purge_items(&ids).unwrap();
        lib.thumb_gc_done(epoch, 1_000).unwrap();

        assert!(lib.thumb_gc_due(1_000, WEEK).unwrap().is_some());
    }

    #[test]
    fn gc_is_due_again_once_the_last_one_is_older_than_max_age() {
        let (_dir, lib) = temp_library();
        let epoch = lib.thumb_gc_due(1_000, WEEK).unwrap().unwrap();
        lib.thumb_gc_done(epoch, 1_000).unwrap();
        let week_ms = WEEK.as_millis() as i64;
        assert_eq!(lib.thumb_gc_due(1_000 + week_ms, WEEK).unwrap(), None);
        assert!(lib.thumb_gc_due(1_001 + week_ms, WEEK).unwrap().is_some());
    }

    #[test]
    fn the_last_folder_survives_a_reopen() {
        let (dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.set_last_folder(folder).unwrap();
        drop(lib);

        // Reopening the same file is the whole point: this is state for the *next* run.
        let reopened = crate::library::Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(reopened.last_folder().unwrap(), Some(folder));
    }

    #[test]
    fn a_folder_that_no_longer_exists_is_not_restored() {
        // A watched folder removed between two runs takes its folder rows with it. The
        // stored id then points at nothing, and the UI must land at the top of the grid
        // rather than ask the grid for an offset that cannot exist.
        let (_dir, lib) = temp_library();
        let (watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.set_last_folder(folder).unwrap();

        lib.remove_watched_folder(watched).unwrap();

        assert_eq!(lib.last_folder().unwrap(), None);
    }

    #[test]
    fn there_is_nothing_to_restore_in_a_fresh_library() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.last_folder().unwrap(), None);
    }

    #[test]
    fn writing_the_last_folder_again_replaces_it() {
        let (_dir, lib) = temp_library();
        let (watched, first) = seed_folder(&lib, Path::new("/p"));
        let second = lib.upsert_folder(watched, Some(first), "/p/b", 1).unwrap();

        lib.set_last_folder(first).unwrap();
        lib.set_last_folder(second).unwrap();

        assert_eq!(lib.last_folder().unwrap(), Some(second));
    }

    #[test]
    fn grid_tile_defaults_to_medium() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.grid_tile().unwrap(), GridTile::Medium);
    }

    #[test]
    fn grid_tile_round_trips() {
        let (_dir, lib) = temp_library();
        for tile in [GridTile::Small, GridTile::Large, GridTile::Medium] {
            lib.set_grid_tile(tile).unwrap();
            assert_eq!(lib.grid_tile().unwrap(), tile, "{tile:?}");
        }
    }

    /// The table is plain text an older or newer photon may have written, so an
    /// unrecognised step falls back rather than erroring - the same rule
    /// `ThemeChoice::parse` follows for an unknown theme.
    #[test]
    fn an_unknown_grid_tile_falls_back_to_medium() {
        let (_dir, lib) = temp_library();
        lib.set_setting(GRID_TILE, "enormous").unwrap();
        assert_eq!(lib.grid_tile().unwrap(), GridTile::Medium);
    }
}
