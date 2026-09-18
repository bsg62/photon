//! Small pieces of UI state that have to outlive the process.
//!
//! Three things so far — the folder the grid was last showing, how long a slideshow holds
//! each photo, and whether the thumbnail cache needs collecting — but the table is generic because a column per setting would mean
//! a migration per setting.

use super::Library;
use crate::Result;
use rusqlite::Connection;
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
}
