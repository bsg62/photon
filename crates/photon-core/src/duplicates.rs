//! Finding byte-identical files.
//!
//! photon never deletes a photo, so this only ever *reports*: the Duplicates view and the
//! copies listed in the viewer. What it costs is reading files, and the design is about
//! reading as few as possible: a file is hashed only if another live file has exactly its
//! size (`Library::hash_candidates`). On a real library that is little more than the
//! duplicates themselves.
//!
//! The hash is XXH3-128, already in the tree for thumbnail fingerprints. It is not
//! cryptographic and does not need to be: nobody is forging collisions against their own
//! photo library, and an accidental one between two files of the same size is not a thing
//! that happens at 128 bits.

use crate::{Result, library::Library};
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use xxhash_rust::xxh3::Xxh3;

/// Bytes read per call. Also how often a cancel is noticed inside one file, which matters
/// for a large file on a slow network mount: `remove_folder` blocks until the scan thread
/// has stopped.
const CHUNK: usize = 1 << 20;

/// XXH3-128 of a file's bytes, or `None` if cancelled partway.
pub fn hash_file(path: &Path, cancel: &AtomicBool) -> std::io::Result<Option<[u8; 16]>> {
    let mut file = File::open(path)?;
    let mut hasher = Xxh3::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(Some(hasher.digest128().to_be_bytes()))
}

/// Hashes every current candidate and returns how many rows took a hash.
///
/// A file that cannot be read is skipped and stays a candidate: it is usually a drive that
/// went away mid-pass, and the next scan's pass tries again. Cancelling stops between files
/// or between chunks; what was hashed so far is kept.
pub fn hash_candidates(lib: &Library, cancel: &AtomicBool) -> Result<u64> {
    let mut hashed = 0;
    for candidate in lib.hash_candidates()? {
        match hash_file(Path::new(&candidate.path), cancel) {
            Ok(Some(hash)) => {
                if lib.set_content_hash(&candidate, &hash)? {
                    hashed += 1;
                }
            }
            Ok(None) => break,
            Err(err) => {
                tracing::debug!(path = %candidate.path, %err, "could not hash a duplicate candidate");
            }
        }
    }
    Ok(hashed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridView;
    use crate::scanner::{ScanOptions, progress_only, scan_watched};
    use crate::testutil::{jpeg_bytes, temp_library, write_file};

    fn never() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn scan(lib: &Library, watched: &crate::library::WatchedFolder, scan_id: i64) {
        scan_watched(
            lib,
            watched,
            scan_id,
            &ScanOptions::default(),
            &mut progress_only(|_: &crate::scanner::ScanProgress| {}),
        )
        .unwrap();
    }

    fn ids(lib: &Library, view: GridView) -> Vec<i64> {
        lib.entries_for(view, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect()
    }

    /// Three files of one size, two of them identical, and one file of a size of its own.
    /// `same` and `copy` are the pair; `other` has their length but different bytes, which is
    /// the case the size shortcut alone would get wrong.
    fn library_with_a_pair() -> (tempfile::TempDir, Library, crate::library::WatchedFolder) {
        let (dir, lib) = temp_library();
        let root = dir.path().join("photos");
        let same = jpeg_bytes(4, 2);
        let mut other = same.clone();
        *other.last_mut().unwrap() ^= 0xff;
        write_file(&root, "a/same.jpg", &same);
        write_file(&root, "b/copy.jpg", &same);
        write_file(&root, "b/other.jpg", &other);
        // Trailing bytes after the end-of-image marker: still a JPEG, and a size no other
        // file has. A different picture is not enough - solid tiles compress to one size.
        let mut unique = same.clone();
        unique.extend_from_slice(b"padding");
        write_file(&root, "b/unique.jpg", &unique);
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        (dir, lib, watched)
    }

    fn name(lib: &Library, id: i64) -> String {
        let path = lib.item(id).unwrap().unwrap().path;
        Path::new(&path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn only_files_sharing_a_size_are_read() {
        let (_dir, lib, _) = library_with_a_pair();
        let mut names: Vec<String> = lib
            .hash_candidates()
            .unwrap()
            .iter()
            .map(|c| name(&lib, c.id))
            .collect();
        names.sort();
        assert_eq!(names, ["copy.jpg", "other.jpg", "same.jpg"]);
    }

    #[test]
    fn the_duplicates_view_holds_identical_files_and_not_merely_same_sized_ones() {
        let (_dir, lib, _) = library_with_a_pair();
        assert_eq!(ids(&lib, GridView::Duplicates), Vec::<i64>::new());
        assert_eq!(lib.duplicate_count().unwrap(), 0);

        assert_eq!(hash_candidates(&lib, &never()).unwrap(), 3);

        let mut names: Vec<String> = ids(&lib, GridView::Duplicates)
            .into_iter()
            .map(|id| name(&lib, id))
            .collect();
        names.sort();
        assert_eq!(names, ["copy.jpg", "same.jpg"]);
        assert_eq!(lib.duplicate_count().unwrap(), 2);
        assert_eq!(
            hash_candidates(&lib, &never()).unwrap(),
            0,
            "hashed once, not on every pass"
        );
    }

    #[test]
    fn a_photo_lists_its_copies_and_not_itself() {
        let (_dir, lib, _) = library_with_a_pair();
        hash_candidates(&lib, &never()).unwrap();
        let all = ids(&lib, GridView::All);
        let same = *all
            .iter()
            .find(|&&id| name(&lib, id) == "same.jpg")
            .unwrap();
        let other = *all
            .iter()
            .find(|&&id| name(&lib, id) == "other.jpg")
            .unwrap();

        let copies = lib.copies_of(same).unwrap();
        assert_eq!(copies.len(), 1);
        assert!(copies[0].path.ends_with("copy.jpg"));
        assert!(lib.copies_of(other).unwrap().is_empty());
    }

    #[test]
    fn a_rewritten_file_loses_its_hash_and_leaves_the_view() {
        // `update_items` must clear the hash: it described bytes the file no longer holds.
        // Without that, `copy.jpg` stays listed as a duplicate of a file it now differs
        // from, until something else happens to rehash it - which nothing would, since a
        // row with a hash is never a candidate again.
        let (dir, lib, watched) = library_with_a_pair();
        hash_candidates(&lib, &never()).unwrap();
        assert_eq!(lib.duplicate_count().unwrap(), 2);

        let root = dir.path().join("photos");
        let mut rewritten = jpeg_bytes(4, 2);
        rewritten.extend_from_slice(b"edited");
        write_file(&root, "b/copy.jpg", &rewritten);
        scan(&lib, &watched, 2);

        assert_eq!(lib.duplicate_count().unwrap(), 0);
        assert_eq!(ids(&lib, GridView::Duplicates), Vec::<i64>::new());
    }

    #[test]
    fn a_hash_is_refused_by_a_row_whose_file_has_since_changed() {
        let (_dir, lib, _) = library_with_a_pair();
        let candidate = lib.hash_candidates().unwrap().remove(0);
        let stale = crate::library::HashCandidate {
            mtime_ms: candidate.mtime_ms - 1,
            ..candidate.clone()
        };
        assert!(!lib.set_content_hash(&stale, &[7; 16]).unwrap());
        assert!(lib.set_content_hash(&candidate, &[7; 16]).unwrap());
    }

    #[test]
    fn a_missing_twin_no_longer_makes_a_duplicate() {
        let (dir, lib, watched) = library_with_a_pair();
        hash_candidates(&lib, &never()).unwrap();
        std::fs::remove_file(dir.path().join("photos/a/same.jpg")).unwrap();
        scan(&lib, &watched, 2);
        assert_eq!(lib.duplicate_count().unwrap(), 0);
        let all = ids(&lib, GridView::All);
        let copy = *all
            .iter()
            .find(|&&id| name(&lib, id) == "copy.jpg")
            .unwrap();
        assert!(lib.copies_of(copy).unwrap().is_empty());
    }

    #[test]
    fn an_unreadable_candidate_is_skipped_and_stays_a_candidate() {
        let (dir, lib, _) = library_with_a_pair();
        // Gone from disk, but no scan has noticed: the row is still live.
        std::fs::remove_file(dir.path().join("photos/b/other.jpg")).unwrap();
        assert_eq!(hash_candidates(&lib, &never()).unwrap(), 2);
        let left: Vec<String> = lib
            .hash_candidates()
            .unwrap()
            .iter()
            .map(|c| name(&lib, c.id))
            .collect();
        assert_eq!(left, ["other.jpg"]);
    }

    #[test]
    fn a_cancelled_pass_hashes_nothing_more() {
        let (_dir, lib, _) = library_with_a_pair();
        assert_eq!(hash_candidates(&lib, &AtomicBool::new(true)).unwrap(), 0);
        assert_eq!(lib.hash_candidates().unwrap().len(), 3);
    }
}
