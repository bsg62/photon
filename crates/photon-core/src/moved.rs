//! Which row a new file used to be (spec `2026-10-05-photon-follow-moved-files-design.md`,
//! "The rule"). `Library::move_candidates` has already matched size, mtime and kind (rule 1's
//! first half). Here: the same pixel size and capture date (the date ignored for a row an older
//! reader described), the old file gone or this very file, its drive there, and exactly one such
//! row, the file name breaking a tie. Only stats; photon never touches a photo file.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::library::{Library, MoveCandidate, NewItem};
use crate::metadata::EXIF_VERSION;
use crate::paths;

/// Which of `candidates` the new file `item` is the moved file of, by the spec's rules 1-5,
/// or `None`. `gone` answers rules 2-4 for one candidate: its file is gone (or is this very
/// file) and its drive is there. `claimed` holds rows another file of this batch took.
pub(crate) fn pick<'c>(
    item: &NewItem,
    candidates: &'c [MoveCandidate],
    claimed: &HashSet<i64>,
    mut gone: impl FnMut(&MoveCandidate) -> bool,
) -> Option<&'c MoveCandidate> {
    // `gone` is last: it is a syscall, possibly on a network share, so it is asked only about
    // rows that could be the file.
    let fits: Vec<&MoveCandidate> = candidates
        .iter()
        .filter(|c| !claimed.contains(&c.id))
        .filter(|c| c.width == item.width && c.height == item.height)
        // An older reader may have dated the photo differently, so its date proves nothing.
        .filter(|c| c.exif_version < EXIF_VERSION || c.taken_at == item.taken_at)
        .filter(|c| gone(c))
        .collect();
    match fits.as_slice() {
        [only] => Some(*only),
        [] => None,
        several => {
            // Photon does not guess which photo's albums and names to hand over: the file name
            // decides, and only when it singles one out.
            let mut named = several.iter().filter(|c| c.file_name == item.file_name);
            match (named.next(), named.next()) {
                (Some(one), None) => Some(*one),
                _ => None,
            }
        }
    }
}

/// The filesystem's answers to rules 2-4, with each watched folder's "is its drive there"
/// worked out once per scan.
pub(crate) struct Probe {
    /// Watched id -> its root is online and a non-empty directory. Filled on first use, and
    /// never refreshed: a scan is one moment, and re-reading a root per file costs a listing
    /// per candidate.
    live: Option<HashMap<i64, bool>>,
}

impl Probe {
    pub(crate) fn new() -> Self {
        Probe { live: None }
    }

    /// Rules 2-4 for `candidate` against the new file at `new_path`.
    pub(crate) fn gone(
        &mut self,
        lib: &Library,
        candidate: &MoveCandidate,
        new_path: &Path,
    ) -> bool {
        // The old spelling is this file (a case-only rename where the old name still opens
        // it): nothing about a drive to ask.
        if paths::same_path(Path::new(&candidate.path), new_path) {
            return true;
        }
        // The root is asked first: an unplugged drive answers NotFound for every file on it,
        // which would read as every photo moved. (Stat-first and refuse-after answer the same;
        // asking the root first also spares a stat per file on a drive that is not there.)
        let live = self.live.get_or_insert_with(|| {
            lib.watched_folders()
                .map(|all| {
                    all.into_iter()
                        .map(|w| (w.id, w.online && root_is_there(Path::new(&w.path))))
                        .collect()
                })
                .unwrap_or_default()
        });
        if !live.get(&candidate.watched_id).copied().unwrap_or(false) {
            return false;
        }
        is_gone(&fs::symlink_metadata(&candidate.path))
    }
}

/// Rule 2's reading of a stat: only "no such file" is gone. Anything else (permissions, a
/// share that timed out) is "cannot tell", and a row that cannot be told is not a candidate.
pub(crate) fn is_gone(stat: &io::Result<fs::Metadata>) -> bool {
    matches!(stat, Err(e) if e.kind() == io::ErrorKind::NotFound)
}

/// Rule 4 for one root: a directory with at least one entry. An unmounted volume's mount point
/// is an empty directory, the same state the scanner's empty-root guard refuses to purge on.
pub(crate) fn root_is_there(root: &Path) -> bool {
    fs::read_dir(root).is_ok_and(|mut entries| entries.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Edit;
    use crate::testutil::{new_item, seed_folder, temp_library, write_file};

    fn candidate(id: i64, path: &str) -> MoveCandidate {
        MoveCandidate {
            id,
            path: path.to_string(),
            file_name: Path::new(path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
            folder_id: 1,
            folder_path: "/p".to_string(),
            watched_id: 1,
            width: 400,
            height: 300,
            taken_at: 5,
            exif_version: EXIF_VERSION,
            edit: Edit::default(),
        }
    }

    fn item() -> NewItem {
        new_item(1, "/p/new.jpg", 5)
    }

    fn pick_ids(cands: &[MoveCandidate], claimed: &[i64], gone: bool) -> Option<i64> {
        let claimed: HashSet<i64> = claimed.iter().copied().collect();
        pick(&item(), cands, &claimed, |_| gone).map(|c| c.id)
    }

    #[test]
    fn one_gone_candidate_is_the_file() {
        assert_eq!(pick_ids(&[candidate(1, "/p/a.jpg")], &[], true), Some(1));
    }

    #[test]
    fn a_candidate_whose_file_is_still_there_is_not() {
        // A copy is not a move.
        assert_eq!(pick_ids(&[candidate(1, "/p/a.jpg")], &[], false), None);
    }

    #[test]
    fn a_different_picture_is_not_the_file() {
        let mut wide = candidate(1, "/p/a.jpg");
        wide.width = 401;
        assert_eq!(pick_ids(&[wide], &[], true), None);
        let mut tall = candidate(1, "/p/a.jpg");
        tall.height = 301;
        assert_eq!(pick_ids(&[tall], &[], true), None);
        let mut dated = candidate(1, "/p/a.jpg");
        dated.taken_at = 6;
        assert_eq!(pick_ids(&[dated.clone()], &[], true), None);
        // An older reader may have dated it differently.
        dated.exif_version = EXIF_VERSION - 1;
        assert_eq!(pick_ids(&[dated], &[], true), Some(1));
    }

    #[test]
    fn a_claimed_row_is_not_offered_twice() {
        assert_eq!(pick_ids(&[candidate(1, "/p/a.jpg")], &[1], true), None);
    }

    #[test]
    fn two_candidates_are_not_guessed_between() {
        let cands = [candidate(1, "/p/a.jpg"), candidate(2, "/p/b.jpg")];
        assert_eq!(pick_ids(&cands, &[], true), None);
    }

    #[test]
    fn the_file_name_decides_between_two() {
        let cands = [candidate(1, "/p/a.jpg"), candidate(2, "/q/new.jpg")];
        assert_eq!(pick_ids(&cands, &[], true), Some(2));
        let both = [candidate(1, "/p/new.jpg"), candidate(2, "/q/new.jpg")];
        assert_eq!(pick_ids(&both, &[], true), None);
    }

    #[test]
    fn gone_is_asked_only_about_rows_that_could_be_the_file() {
        // A stat is a syscall, possibly on a network share.
        let mut other = candidate(2, "/p/b.jpg");
        other.width = 999;
        let mut dated = candidate(3, "/p/c.jpg");
        dated.taken_at = 77;
        let claimed_row = candidate(4, "/p/d.jpg");
        let cands = [candidate(1, "/p/a.jpg"), other, dated, claimed_row];
        let claimed: HashSet<i64> = [4].into_iter().collect();
        let mut asked = Vec::new();
        pick(&item(), &cands, &claimed, |c| {
            asked.push(c.id);
            true
        });
        assert_eq!(asked, vec![1]);
    }

    #[test]
    fn only_not_found_is_gone() {
        let err = |k| Err(io::Error::from(k));
        assert!(is_gone(&err(io::ErrorKind::NotFound)));
        assert!(!is_gone(&err(io::ErrorKind::PermissionDenied)));
        assert!(!is_gone(&err(io::ErrorKind::TimedOut)));
        let dir = tempfile::tempdir().unwrap();
        let file = write_file(dir.path(), "a.jpg", b"x");
        assert!(!is_gone(&fs::symlink_metadata(&file)));
    }

    #[test]
    fn a_root_is_there_only_as_a_directory_with_something_in_it() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!root_is_there(dir.path()));
        let file = write_file(dir.path(), "a.jpg", b"x");
        assert!(root_is_there(dir.path()));
        assert!(!root_is_there(&dir.path().join("missing")));
        assert!(!root_is_there(&file));
    }

    /// A library with one watched root holding `kept.jpg`, and a candidate row in it.
    fn rooted(
        path_in_root: &str,
    ) -> (tempfile::TempDir, tempfile::TempDir, Library, MoveCandidate) {
        let (dbdir, lib) = temp_library();
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "kept.jpg", b"x");
        let (watched, folder) = seed_folder(&lib, root.path());
        let path = root.path().join(path_in_root).to_str().unwrap().to_string();
        let mut c = candidate(1, &path);
        c.watched_id = watched;
        c.folder_id = folder;
        (dbdir, root, lib, c)
    }

    #[test]
    fn the_probe_follows_a_file_that_is_gone_from_a_live_root() {
        let (_db, root, lib, c) = rooted("old.jpg");
        let new_path = root.path().join("elsewhere").join("new.jpg");
        assert!(Probe::new().gone(&lib, &c, &new_path));

        // A candidate whose file exists is not gone.
        let present = candidate_in(&c, root.path().join("kept.jpg"));
        assert!(!Probe::new().gone(&lib, &present, &new_path));

        // Rule 3: the old spelling is the new file, present on disk.
        let same = candidate_in(&c, root.path().join("kept.jpg"));
        let kept = root.path().join("kept.jpg");
        assert!(Probe::new().gone(&lib, &same, &kept));

        // An offline watched folder: its files all answer NotFound, and are not gone.
        lib.set_watched_online(c.watched_id, false).unwrap();
        assert!(!Probe::new().gone(&lib, &c, &new_path));
        lib.set_watched_online(c.watched_id, true).unwrap();
        assert!(Probe::new().gone(&lib, &c, &new_path));

        // An emptied root is an unmounted volume's mount point, not a root of moved photos.
        fs::remove_file(root.path().join("kept.jpg")).unwrap();
        assert!(!Probe::new().gone(&lib, &c, &new_path));
    }

    fn candidate_in(base: &MoveCandidate, path: std::path::PathBuf) -> MoveCandidate {
        let mut c = candidate(base.id, path.to_str().unwrap());
        c.watched_id = base.watched_id;
        c
    }

    #[test]
    fn the_probe_asks_about_a_root_once() {
        // Per scan by design: re-listing a root for every candidate would cost a listing per
        // file, so a root emptied mid-scan keeps the answer it had.
        let (_db, root, lib, c) = rooted("old.jpg");
        let new_path = root.path().join("new.jpg");
        let mut probe = Probe::new();
        assert!(probe.gone(&lib, &c, &new_path));
        fs::remove_file(root.path().join("kept.jpg")).unwrap();
        assert!(probe.gone(&lib, &c, &new_path));
    }
}
