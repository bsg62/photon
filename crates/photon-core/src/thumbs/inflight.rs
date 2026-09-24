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

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// How many deaths with a photo in flight make it the suspect. One is not enough: quitting,
/// a power cut or an unrelated crash while a photo happens to be decoding would blame it.
pub(crate) const DEATHS_TO_FAIL: u32 = 2;

/// Shown where the photo's thumbnail would be, like any other decode failure.
pub(crate) const CRASH_MESSAGE: &str = "photon closed unexpectedly while reading this photo";

pub(crate) struct InFlight {
    dir: PathBuf,
}

impl InFlight {
    pub(crate) fn new(cache_root: &Path) -> Self {
        Self {
            dir: cache_root.join("in-flight"),
        }
    }

    /// Counts one death for every marker a previous run left behind. Called once, before any
    /// worker starts: a marker still present then was being decoded when the process ended.
    pub(crate) fn recover(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let deaths = read_count(&path).unwrap_or(0);
            if let Err(err) = fs::write(&path, (deaths + 1).to_string()) {
                tracing::warn!(%err, ?path, "could not record a death against an in-flight photo");
            }
        }
    }

    /// Deaths recorded against `id` so far; none if it has no marker.
    pub(crate) fn deaths(&self, id: i64) -> u32 {
        read_count(&self.path(id)).unwrap_or(0)
    }

    /// Marks `id` in flight until the returned guard drops. Best-effort: a cache that cannot
    /// be written loses the guard, not the thumbnail.
    pub(crate) fn begin(&self, id: i64, deaths: u32) -> Marker {
        let path = self.path(id);
        let written =
            fs::create_dir_all(&self.dir).and_then(|()| fs::write(&path, deaths.to_string()));
        if let Err(err) = written {
            tracing::warn!(%err, ?path, "could not mark a photo in flight");
        }
        Marker(path)
    }

    /// Forgets `id`'s deaths, once the photo has been failed for them.
    pub(crate) fn clear(&self, id: i64) {
        remove(&self.path(id));
    }

    fn path(&self, id: i64) -> PathBuf {
        self.dir.join(id.to_string())
    }
}

/// Removes its marker when dropped: on success, on an ordinary error, and on a panic that
/// `catch_unwind` caught, since unwinding runs `Drop`. An abort runs nothing, which is the
/// point: that marker stays behind for `recover` to count.
pub(crate) struct Marker(PathBuf);

impl Drop for Marker {
    fn drop(&mut self) {
        remove(&self.0);
    }
}

/// A marker that cannot be parsed (a torn write) is still a death: `Some(0)`, not `None`.
fn read_count(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    Some(text.trim().parse().unwrap_or(0))
}

fn remove(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(%err, ?path, "could not clear an in-flight marker"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_counts_one_death_per_leftover_marker() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        // A leaked guard stands in for an abort, which never runs `Drop`.
        std::mem::forget(inflight.begin(7, 0));
        std::mem::forget(inflight.begin(8, 1));
        inflight.recover();
        assert_eq!((inflight.deaths(7), inflight.deaths(8)), (1, 2));
        assert_eq!(inflight.deaths(9), 0, "no marker, no deaths");
    }

    /// A marker whose content is not a number (a torn write when the power went) still
    /// counts as a death rather than being ignored.
    #[test]
    fn an_unreadable_marker_counts_as_a_first_death() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("in-flight")).unwrap();
        std::fs::write(dir.path().join("in-flight/5"), "garbage").unwrap();
        let inflight = InFlight::new(dir.path());
        inflight.recover();
        assert_eq!(inflight.deaths(5), 1);
    }

    /// `read_count`'s own contract: an unparseable marker is a *counted* death (`Some(0)`),
    /// distinct from no marker at all (`None`). Neither current caller of `read_count`
    /// observes the difference directly - `deaths` and `recover` both fold the `Option` with
    /// their own `unwrap_or(0)` - so a change collapsing `Some(0)` into `None` here is invisible
    /// through them alone; this pins the distinction at its source instead.
    #[test]
    fn an_unparseable_marker_is_some_zero_not_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("in-flight")).unwrap();
        std::fs::write(dir.path().join("in-flight/5"), "garbage").unwrap();
        assert_eq!(read_count(&dir.path().join("in-flight/5")), Some(0));
        assert_eq!(read_count(&dir.path().join("in-flight/no-such-file")), None);
    }

    #[test]
    fn a_marker_disappears_when_its_guard_drops() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        let marker = inflight.begin(3, 1);
        assert_eq!(inflight.deaths(3), 1);
        drop(marker);
        assert!(!dir.path().join("in-flight/3").exists());
    }
}
