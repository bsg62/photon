//! Remembers which photos a thumbnail worker was decoding when photon died, so a photo
//! that kills the process is not decoded again on every launch.
//!
//! `catch_unwind` in `service.rs` contains a decoder that panics, but three ways of dying
//! get past it:
//! - a panic inside rav1d, which cannot unwind out of its `extern "C"` entry points and
//!   aborts (see `avif/av1.rs`);
//! - an allocation failure, which aborts without unwinding;
//! - the OOM killer.
//!
//! Each leaves the photo `Pending`, so the next launch queues it again and dies again, with
//! nothing naming the file. A marker file per in-flight decode survives all three, where a
//! panic hook would see only the first.
//!
//! A marker is turned into a **death record**, under its own directory, the moment `recover`
//! sees it - not left in place and re-read on every later launch. The first version of this
//! guard kept counting by adding one to the marker itself at every `recover`, which blamed a
//! photo that had only ever died once: a photo on an offline drive (never queued, so its
//! marker - if it somehow had one - would sit untouched forever), a quick relaunch, an
//! orphaned marker (`process` used to return before clearing one for a missing, purged or
//! already-failed item), and SQLite id reuse (`items.id` is `INTEGER PRIMARY KEY` without
//! `AUTOINCREMENT`, so a library rebuild can hand a healthy photo a dead one's id) would all
//! gain a false death at every subsequent launch, forever, with nothing to reset the count. A
//! record is written once per real death and the marker that produced it is deleted in the
//! same pass, so a marker can never outlive the launch after the death that left it, and a
//! record is keyed by the photo's `thumb_key()` so a reused id, a changed file, or a fresh
//! edit never inherits someone else's deaths.
//!
//! That second version still let `begin` write a marker before its photo's decode actually
//! held `service.rs`'s exclusive decode lock, so every photo merely *waiting* for a suspect's
//! turn already had a marker on disk - a second suspect, never actually decoding, could be
//! blamed for the first one's death. `begin` is only ever called after the lock is held, and
//! `Marker::resolve` now tells the guard whether the decode actually decided the photo's fate
//! before it drops, so a transient failure (the drive dropped out, not a death) does not erase
//! a real death recorded earlier.

use crate::grid::hex_key;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// How many deaths with a photo in flight make it the suspect. One is not enough: quitting,
/// a power cut or an unrelated crash while a photo happens to be decoding would blame it.
pub(crate) const DEATHS_TO_FAIL: u32 = 2;

/// Shown where the photo's thumbnail would be, like any other decode failure.
pub(crate) const CRASH_MESSAGE: &str = "photon closed unexpectedly while reading this photo";

/// Extension of a temp file mid atomic-write. `recover` and `disarm` skip it when listing
/// their directory: a write not yet renamed into place is not yet a marker or a record, and
/// treating it as either would race the rename that is about to replace it.
const TMP_EXT: &str = "tmp";

/// One exclusive lock file per cache root, held for the whole life of the `InFlight` that
/// acquires it. Double-clicking a launcher starts a second instance pointed at the same
/// library; without this, either instance's `recover()` would turn the *other's* live
/// markers into deaths and delete them out from under its still-running decode, and either's
/// `disarm()` would sweep the other's markers on its own clean quit. Named outside
/// `in-flight/` itself so neither `recover` nor `disarm`'s directory listing ever has to
/// know to skip it.
const LOCK_FILE: &str = "in-flight.lock";

pub(crate) struct InFlight {
    markers: PathBuf,
    records: PathBuf,
    /// Set by `disarm` on a clean quit. `begin` then writes no marker: a deliberate exit
    /// killed nothing, so nothing should be left for the next launch to misread as a death.
    closing: AtomicBool,
    /// Held for as long as `self` lives, once acquired - dropping it releases the OS lock.
    /// Never read; it exists to keep the lock alive, nothing else.
    _lock: Option<File>,
    /// Whether `_lock` was actually acquired. A second instance on the same cache root runs
    /// with this `false`: `recover` reads nothing, `begin` writes no marker, `disarm` sweeps
    /// nothing, `clear` removes no record, and a `Marker` it hands out removes neither its
    /// marker nor its record on drop - every one of those paths, if it exists, belongs to
    /// whichever instance does hold the guard, so this instance can neither corrupt it nor be
    /// corrupted by it. It still decodes thumbnails - just without the crash-loop guard.
    guarded: bool,
}

impl InFlight {
    pub(crate) fn new(cache_root: &Path) -> Self {
        let lock = acquire_lock(cache_root);
        let guarded = lock.is_some();
        if !guarded {
            tracing::warn!(
                "another photon instance already guards this thumbnail cache; \
                 running without the crash-loop guard"
            );
        }
        Self {
            markers: cache_root.join("in-flight"),
            records: cache_root.join("deaths"),
            closing: AtomicBool::new(false),
            _lock: lock,
            guarded,
        }
    }

    /// Turns every marker a previous run left behind into a counted death, then deletes the
    /// marker. Called once, before any worker starts: a marker still present then was being
    /// decoded when the process ended. A death only counts against the record it matches -
    /// same key, meaning the same photo as it stood at that death, not merely the same id -
    /// so it carries forward correctly and it starts over at one otherwise.
    ///
    /// A marker that cannot be read, or whose content is not the 16 hex characters `begin`
    /// writes (a torn write, if the power went mid-write despite the rename), still counts as
    /// a death: `marker_key` reports it as the empty key. `deaths` can never match the empty
    /// key against a real photo's `hex_key(...)`, so that death can never fail anything - it
    /// is exactly as harmless as a marker with no photo behind it, which is the only thing it
    /// could actually be. Deleting it either way is the point: nothing may wedge
    /// `in-flight/` forever.
    ///
    /// The marker is deleted *before* the record is written, not after: a death between the
    /// two steps then loses this one count (an extra life the two-strike rule tolerates, and
    /// no worse than a marker recover never got to see at all) rather than leaving the marker
    /// behind for the *next* launch to count again on top of the record this launch already
    /// wrote - which would double it, and risk a permanent false failure. Getting the crash
    /// window wrong in the direction that only ever loses a count is the one that matters.
    ///
    /// Also clears any `.tmp` file left in either directory: nothing writes here before this
    /// runs, so a `.tmp` name still present is a write a previous run's own death interrupted
    /// mid-rename, not a real marker or record - and unlike a real marker, it names nothing
    /// `deaths` could ever match, so leaving it in place would only wedge disk space forever.
    pub(crate) fn recover(&self) {
        if !self.guarded {
            // Not this instance's markers to judge - see `guarded`'s own comment.
            return;
        }
        clear_tmp(&self.records);
        let Ok(entries) = fs::read_dir(&self.markers) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if is_tmp(&path) {
                remove(&path);
                continue;
            }
            let Some(id) = id_of(&path) else { continue };
            let key = marker_key(&path);
            let count = match self.read_record(id) {
                Some((previous, recorded_key)) if recorded_key == key => previous + 1,
                _ => 1,
            };
            remove(&path);
            self.write_record(id, count, &key);
        }
    }

    /// Deaths recorded against `id` while its content matched `key`. Reports 0 for a record
    /// left by a different photo now living at the same id (id reuse), or by an earlier
    /// version of the same photo (a changed file, a fresh edit) - either way `key` no longer
    /// matches what killed photon, so it must not blame what is there now.
    pub(crate) fn deaths(&self, id: i64, key: u64) -> u32 {
        let key = hex_key(key);
        match self.read_record(id) {
            Some((count, recorded_key)) if recorded_key == key => count,
            _ => 0,
        }
    }

    /// Marks `id` in flight, under `key`, until the returned guard drops. Writes nothing when
    /// `guarded` is false (a second instance on this cache root - see the field's own
    /// comment) or once `disarm` has run, and - since a worker can be caught between reading
    /// `closing` and
    /// finishing the write - checks again immediately after: if `disarm` set the flag while
    /// this write was in flight, the marker just written is removed again right away, rather
    /// than leaving it for `disarm`'s own sweep (already run by then) or a worker that, in
    /// production, is never actually joined before `process::exit` to rely on.
    ///
    /// `closing` is `SeqCst`, not `Release`/`Acquire`, on every store and load: this is a
    /// store-buffering (Dekker) pattern - `disarm` stores the flag and then lists the
    /// directory, `begin` writes the marker and then reads the flag - and `Release`/`Acquire`
    /// alone does not rule out *both* sides observing only the other's old value (no store
    /// there yet, no marker there yet), which would let a marker survive a concurrent
    /// `disarm` uncaught by either. `SeqCst` puts every access to `closing` into one global
    /// order shared by both threads, so whichever of `disarm`'s store and this second load
    /// happens second in that order is guaranteed to observe the other: if this load reads
    /// `true`, `disarm` already ran and this call removes what it just wrote; if it reads
    /// `false`, `disarm` has not stored yet, so its later sweep - which does not depend on
    /// this check at all - still finds the marker already on disk by the time it runs.
    /// Best-effort throughout: a cache that cannot be written loses the guard, not the
    /// thumbnail.
    pub(crate) fn begin(&self, id: i64, key: u64) -> Marker {
        let marker = self.marker_path(id);
        if self.guarded && !self.closing.load(Ordering::SeqCst) {
            match write_atomic(&self.markers, &marker, &hex_key(key)) {
                Ok(()) if self.closing.load(Ordering::SeqCst) => remove(&marker),
                Ok(()) => {}
                Err(err) => tracing::warn!(%err, ?marker, "could not mark a photo in flight"),
            }
        }
        Marker {
            marker,
            record: self.record_path(id),
            decided: false,
            // Only an instance that holds the lock ever wrote (or could write) anything for
            // this id - see `guarded`'s own doc. An unguarded `Marker` must not remove either
            // path on drop: the marker and the record at this id, if they exist at all, are
            // the guarded instance's still-live decode and its death history, not this
            // instance's to erase, and it never counted this attempt either way.
            guarded: self.guarded,
        }
    }

    /// Forgets `id`'s death record. Called once its photo has either been failed for its
    /// deaths or has just survived a fresh decode (`Marker::drop` does the latter case too;
    /// this is `process`'s own call for the former). Safe even if the item underneath has
    /// changed since: `deaths` already refuses a record whose key does not match the item
    /// currently at `id`, so a record left behind for the old key could never have blamed the
    /// new one anyway - clearing it early is hygiene, not correctness.
    pub(crate) fn clear(&self, id: i64) {
        if !self.guarded {
            // Not this instance's record to erase - see `guarded`'s own comment. It never
            // counts a death either way, so a record at this id belongs to whichever instance
            // does hold the lock.
            return;
        }
        remove(&self.record_path(id));
    }

    /// Disarms the guard for a clean quit: no marker is written from here on (`begin` checks
    /// `closing` both before writing and again right after, so a write racing this call
    /// either never starts or is undone immediately - see its own comment), and every marker
    /// on disk right now is removed, because photon chose to stop - nothing killed it.
    ///
    /// This has to be the one actually doing the removing, not merely trusting the worker to
    /// clean up after itself: in production, `ThumbService::close` is followed by tauri's own
    /// `process::exit`, with the worker threads never joined first (`RunEvent::Exit` ->
    /// `Engine::shutdown` -> `close()`), so a worker cannot be relied on to run its job to
    /// completion and drop its `Marker` the way it would on an ordinary, crash-free exit.
    pub(crate) fn disarm(&self) {
        // SeqCst to pair with `begin`'s SeqCst loads - see `begin`'s own comment for why
        // `Release` alone would not be enough here.
        self.closing.store(true, Ordering::SeqCst);
        if !self.guarded {
            // Nothing on disk under `markers` is this instance's to sweep - it could belong
            // to whichever instance does hold the lock, still decoding.
            return;
        }
        let Ok(entries) = fs::read_dir(&self.markers) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_tmp(&path) {
                remove(&path);
            }
        }
    }

    fn marker_path(&self, id: i64) -> PathBuf {
        self.markers.join(id.to_string())
    }

    fn record_path(&self, id: i64) -> PathBuf {
        self.records.join(id.to_string())
    }

    /// Parses a death record's `"<count> <key hex>"`. `None` for a missing or unreadable
    /// file, or one whose count is not a plain integer - the same "not there" answer either
    /// way, since a torn write here is no more informative than no record at all.
    fn read_record(&self, id: i64) -> Option<(u32, String)> {
        let text = fs::read_to_string(self.record_path(id)).ok()?;
        let mut parts = text.trim().splitn(2, ' ');
        let count = parts.next()?.parse().ok()?;
        Some((count, parts.next().unwrap_or_default().to_string()))
    }

    fn write_record(&self, id: i64, count: u32, key: &str) {
        let path = self.record_path(id);
        if let Err(err) = write_atomic(&self.records, &path, &format!("{count} {key}")) {
            tracing::warn!(%err, ?path, "could not record a death against a photo");
        }
    }
}

/// Removes its photo's marker when dropped - the decode finished without taking photon down
/// with it, whatever the outcome, so there is nothing left for the next launch to misread as a
/// leftover death. An abort or the OOM killer runs no `Drop` at all, which is the one case this
/// guard exists to survive: the marker stays, for `recover` to find. Only when `guarded` is
/// true, though: a `Marker` from an unguarded `InFlight` wrote no marker in `begin` and must
/// remove nothing on drop either - the path at this id, if one exists, is the guarded
/// instance's own live decode, not this one's to delete out from under it.
///
/// The death *record* is a different question: it is only cleared when `resolve` was told the
/// photo's fate was actually decided this attempt (rendered, or explicitly failed - a caught
/// panic counts, since `process` marks the item `Failed` for it). A transient failure - the
/// source unreachable, the cache unwritable - decides nothing and leaves the item `Pending`
/// for a retry, so the record has to survive it: a suspect whose drive merely dropped out for
/// a moment must not have its earlier death quietly forgotten by a retry that never even
/// reached the decoder. `resolve` defaults to `false` (kept) precisely because forgetting a
/// real death is the wrong direction to fail in - the two-strike rule already tolerates the
/// occasional extra life a kept record costs an innocent photo.
pub(crate) struct Marker {
    marker: PathBuf,
    record: PathBuf,
    decided: bool,
    /// Whether the `InFlight` that made this guard holds the cache root's lock. `false` for a
    /// second instance on the same root (see `InFlight::guarded`): it wrote no marker for this
    /// id and never counts a death, so on drop it must not remove either path - both, if they
    /// exist, belong to whichever instance does hold the lock.
    guarded: bool,
}

impl Marker {
    /// Tells the guard how this decode ended, before it drops: `true` once `process` knows
    /// the photo's fate either way (`Ok`, or a caught panic - both explicit `Failed` writes go
    /// through `set_thumb_state_if_unchanged` on their own), `false` for a transient error that
    /// leaves the item `Pending`. Consumes the guard, so `Drop` - and the removals above -
    /// run immediately.
    pub(crate) fn resolve(mut self, decided: bool) {
        self.decided = decided;
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if !self.guarded {
            return;
        }
        remove(&self.marker);
        if self.decided {
            remove(&self.record);
        }
    }
}

/// The key a marker file names, or the empty string if it is missing, unreadable, or not the
/// 16 lowercase hex characters `begin` writes. `deaths` can never match the empty string
/// against a real `hex_key(...)` (which is always exactly 16 hex characters), so folding
/// every unreadable case into one sentinel is safe: it is counted as a death - the marker
/// existing at all means something was in flight - but that death can never be pinned on any
/// actual photo.
fn marker_key(path: &Path) -> String {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| text.len() == 16 && text.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_default()
}

fn id_of(path: &Path) -> Option<i64> {
    path.file_name()?.to_str()?.parse().ok()
}

/// Opens (creating if needed) and locks [`LOCK_FILE`] under `cache_root`, exclusively and
/// without blocking. `None` means another instance already holds it - `TryLockError::WouldBlock`
/// on all three platforms per `std::fs::File::try_lock`'s docs - or the attempt failed some
/// other way (the cache root could not be created, or `TryLockError::Error`, an I/O error);
/// either way the caller falls back to running unguarded rather than risk treating a live
/// decode of another instance as this one's to judge.
fn acquire_lock(cache_root: &Path) -> Option<File> {
    if let Err(err) = fs::create_dir_all(cache_root) {
        tracing::warn!(%err, ?cache_root, "could not create the thumbnail cache root");
        return None;
    }
    let path = cache_root.join(LOCK_FILE);
    let file = match File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(err) => {
            tracing::warn!(%err, ?path, "could not open the in-flight lock file");
            return None;
        }
    };
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(std::fs::TryLockError::WouldBlock) => None,
        Err(std::fs::TryLockError::Error(err)) => {
            tracing::warn!(%err, ?path, "could not lock the in-flight lock file");
            None
        }
    }
}

fn is_tmp(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some(TMP_EXT)
}

/// Removes every `.tmp` entry directly in `dir`. Only `recover` calls this, and only on the
/// records directory (the markers directory's own `.tmp` entries are cleared inline, in the
/// same pass that turns its real markers into records) - both are safe only because `recover`
/// runs once, before any worker exists to be mid-write.
fn clear_tmp(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_tmp(&path) {
            remove(&path);
        }
    }
}

/// Writes `content` to `dest` through a temp file in the same directory, then renames it into
/// place, so `recover`/`disarm`'s own directory listing - or a concurrent `deaths`/`recover`
/// read of the same path - never sees a half-written marker or record. `fs::write` alone
/// truncates before it writes, so a process killed mid-write (the same kind of death this
/// whole guard exists for) would otherwise leave an empty file where a real count was.
fn write_atomic(dir: &Path, dest: &Path, content: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dest.with_extension(TMP_EXT);
    fs::write(&tmp, content)?;
    fs::rename(&tmp, dest)
}

fn remove(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        // Windows can fail this while another handle (an antivirus scan, say) has the file
        // open. Best-effort is enough: at worst that one file becomes one false death at the
        // next launch, which the two-strike rule already absorbs.
        Err(err) => tracing::warn!(%err, ?path, "could not remove an in-flight file"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_counts_each_death_once() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        let key = 0x1234_5678_9abc_def0_u64;
        // A leaked guard stands in for an abort, which never runs `Drop`.
        std::mem::forget(inflight.begin(7, key));
        inflight.recover();
        inflight.recover();
        assert_eq!(
            inflight.deaths(7, key),
            1,
            "the first version of this guard bumped the marker itself on every recover, so a \
             second recover with no second death would have made this 2"
        );
        assert!(
            !dir.path().join("in-flight/7").exists(),
            "the marker must not outlive the launch that turned it into a record"
        );
    }

    #[test]
    fn a_record_for_another_photo_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        let (k1, k2) = (0x1111_1111_1111_1111_u64, 0x2222_2222_2222_2222_u64);
        std::mem::forget(inflight.begin(7, k1));
        inflight.recover();
        assert_eq!(
            inflight.deaths(7, k2),
            0,
            "a reused id, or a changed file, must not inherit a death from before"
        );
        std::mem::forget(inflight.begin(7, k2));
        inflight.recover();
        assert_eq!(inflight.deaths(7, k2), 1, "a new key starts a fresh count");
    }

    /// A marker whose content is not a number (a torn write when the power went) still
    /// counts as a death, under a key nothing real can ever match, and is removed either way.
    #[test]
    fn an_unreadable_marker_counts_as_a_first_death() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("in-flight")).unwrap();
        std::fs::write(dir.path().join("in-flight/5"), "garbage").unwrap();
        let inflight = InFlight::new(dir.path());
        inflight.recover();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("deaths/5"))
                .unwrap()
                .trim(),
            "1",
            "still a counted death"
        );
        assert_eq!(
            inflight.deaths(5, 0),
            0,
            "but an empty key never matches a real one, even 0"
        );
        assert!(!dir.path().join("in-flight/5").exists());
    }

    #[test]
    fn a_marker_disappears_when_its_guard_drops() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        let marker = inflight.begin(3, 1);
        assert_eq!(inflight.deaths(3, 1), 0, "not a death until recover runs");
        drop(marker);
        assert!(!dir.path().join("in-flight/3").exists());
        assert!(!dir.path().join("deaths/3").exists());
    }

    /// A `.tmp` file is a write a previous run's own death interrupted before the rename -
    /// never a live marker or record, since nothing writes here before `recover` runs. Left
    /// alone, it would wedge disk space forever with a name `deaths` can never match anyway.
    #[test]
    fn recover_clears_leftover_tmp_files_in_both_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("in-flight")).unwrap();
        std::fs::create_dir_all(dir.path().join("deaths")).unwrap();
        std::fs::write(dir.path().join("in-flight/7.tmp"), "dead write").unwrap();
        std::fs::write(dir.path().join("deaths/9.tmp"), "dead write").unwrap();
        let inflight = InFlight::new(dir.path());
        inflight.recover();
        assert!(!dir.path().join("in-flight/7.tmp").exists());
        assert!(!dir.path().join("deaths/9.tmp").exists());
    }

    #[test]
    fn a_clean_close_leaves_no_marker_to_count() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        let key = 5_u64;
        std::mem::forget(inflight.begin(3, key));
        inflight.disarm();

        // Once disarmed, a new `begin` writes nothing at all.
        std::mem::forget(inflight.begin(4, key));
        assert!(!dir.path().join("in-flight/4").exists());

        // Release the lock file before the "next launch" acquires it - held both at once,
        // the next launch would run unguarded (see `InFlight::guarded`) and its `recover()`
        // would be a no-op regardless of whether `disarm`'s sweep actually ran, which is
        // exactly what made this test pass with the sweep disabled.
        drop(inflight);

        // Stands in for the next launch: nothing survived disarm's sweep to be counted.
        let next_launch = InFlight::new(dir.path());
        next_launch.recover();
        assert_eq!(next_launch.deaths(3, key), 0);
    }

    /// Two instances on one cache root, simulating a double-clicked launcher: the second
    /// must not turn the first's still-live marker into a death (which would also delete
    /// it out from under the first's still-running decode), and the second's own `disarm`
    /// must not sweep it either. Before the lock, both of those happened: `recover` and
    /// `disarm` had no way to tell "another instance's marker" from "a previous run's".
    #[test]
    fn a_second_instance_does_not_touch_the_firsts_markers() {
        let dir = tempfile::tempdir().unwrap();
        let first = InFlight::new(dir.path());
        let key = 0xdead_beef_0000_0001_u64;
        // Stands in for a decode still genuinely running in the first instance.
        std::mem::forget(first.begin(1, key));
        assert!(dir.path().join("in-flight/1").exists());

        let second = InFlight::new(dir.path());
        second.recover();
        assert!(
            dir.path().join("in-flight/1").exists(),
            "a second instance must not turn the first's live marker into a death"
        );
        assert_eq!(
            second.deaths(1, key),
            0,
            "and must not have recorded one either"
        );

        second.disarm();
        assert!(
            dir.path().join("in-flight/1").exists(),
            "a second instance's disarm must not sweep the first's marker"
        );

        // The first instance's own view is unaffected throughout.
        assert_eq!(first.deaths(1, key), 0, "not a death until recover runs");
    }

    /// An unguarded instance's own `Marker` must not erase what a guarded instance owns at
    /// the same id: neither the guarded instance's still-live marker (a decode genuinely in
    /// flight there) nor its death record, whether through the unguarded `Marker`'s own drop,
    /// an explicit `resolve(true)`, or a direct `clear` call. Before this fix `Marker::drop`
    /// removed its paths unconditionally, so an unguarded second instance racing a `begin` for
    /// the same id as a guarded first instance's live decode deleted that decode's marker out
    /// from under it, and any death record standing against it.
    #[test]
    fn an_unguarded_instances_marker_does_not_erase_the_guarded_ones_records() {
        let dir = tempfile::tempdir().unwrap();
        let first = InFlight::new(dir.path());
        let key = 0xaaaa_bbbb_cccc_dddd_u64;

        // One death already on record for id 2, the way `recover` would leave it.
        std::mem::forget(first.begin(2, key));
        first.recover();
        assert_eq!(first.deaths(2, key), 1);

        // Stands in for a fresh decode of the same photo, genuinely in flight in the first
        // (guarded) instance right now.
        std::mem::forget(first.begin(2, key));
        assert!(dir.path().join("in-flight/2").exists());
        assert!(dir.path().join("deaths/2").exists());

        // A second instance on the same root, unguarded because `first` still holds the lock
        // file - simulating a double-clicked launcher racing a job for the very same id.
        let second = InFlight::new(dir.path());
        let marker = second.begin(2, key);
        marker.resolve(true); // consumes and drops the guard, as a finished decode would.

        assert!(
            dir.path().join("in-flight/2").exists(),
            "the unguarded instance's Marker must not delete the guarded instance's live marker"
        );
        assert!(
            dir.path().join("deaths/2").exists(),
            "nor the guarded instance's death record, even though this attempt was resolved"
        );

        second.clear(2);
        assert!(
            dir.path().join("deaths/2").exists(),
            "and a direct clear() from the unguarded instance must not erase it either"
        );

        // The first instance's own view is unaffected throughout.
        assert_eq!(first.deaths(2, key), 1);
    }
}
