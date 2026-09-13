//! Reads Picasa's per-directory star flags.
//!
//! Picasa writes one INI per directory — `.picasa.ini` on newer versions, `Picasa.ini` on
//! older ones — with a section per file. photon only ever reads them: nothing here writes,
//! creates or deletes an INI, and photon cannot set a star.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// How much of an INI is read. Picasa writes a few lines per photo, so this is far beyond
/// any real file; it exists so a stray enormous one cannot pull unbounded memory through a
/// scan of a hundred thousand photos.
pub const MAX_INI: u64 = 8 * 1024 * 1024;

/// The starred file names in one directory, lowercased.
///
/// `None` means the evidence could not be read — the directory or its INI is unreadable —
/// and the caller must leave existing stars alone, because failing to read is not evidence
/// that the stars are gone. `Some(empty)` is a real answer: the directory was read and
/// nothing is starred, which is what lets a caller clear stars the INI no longer confirms.
pub fn read_stars(dir: &Path) -> Option<HashSet<String>> {
    let Some(path) = ini_path(dir)? else {
        return Some(HashSet::new());
    };
    let bytes = read_capped(&path)?;
    Some(parse_stars(&String::from_utf8_lossy(&bytes)))
}

/// The INI to read, if any. `Some(None)` when the directory has none; `None` when the
/// directory itself could not be listed.
///
/// Listing the directory rather than probing two fixed names is what makes the match
/// case-insensitive: a library written on Windows and read on Linux may carry any casing,
/// and a missed file means a whole folder silently loses its stars. `.picasa.ini` wins when
/// both exist, and the two are never merged.
fn ini_path(dir: &Path) -> Option<Option<PathBuf>> {
    let mut dotted = None;
    let mut plain = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        match name.to_lowercase().as_str() {
            ".picasa.ini" => dotted = Some(entry.path()),
            "picasa.ini" => plain = Some(entry.path()),
            _ => {}
        }
    }
    Some(dotted.or(plain))
}

fn read_capped(path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.by_ref().take(MAX_INI).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Walks the INI line by line. Deliberately lenient: an unterminated header, a stray line
/// or a comment is skipped rather than failing the file, because rejecting a file outright
/// would lose every star in that folder.
fn parse_stars(text: &str) -> HashSet<String> {
    let mut stars = HashSet::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            // An unterminated header names no file, so it clears the section rather than
            // letting the keys below it attach to the previous one.
            section = rest
                .strip_suffix(']')
                .map(|name| name.trim().to_lowercase());
            continue;
        }
        let Some(name) = section.as_ref() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("star") && is_star(value.trim()) {
            stars.insert(name.clone());
        }
    }
    stars
}

fn is_star(value: &str) -> bool {
    value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true") || value == "1"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::write_file;

    /// The starred names in `dir`, sorted, for assertions that do not care about set order.
    fn stars(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = read_stars(dir).unwrap().into_iter().collect();
        v.sort();
        v
    }

    #[test]
    fn reads_a_star_from_a_dotted_ini() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=yes\nbackuphash=12223\n[b.jpg]\nbackuphash=1332313\n",
        );
        assert_eq!(
            stars(dir.path()),
            vec!["a.jpg"],
            "b.jpg has no star key, so it is not starred"
        );
    }

    #[test]
    fn reads_a_star_from_an_undotted_ini() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "Picasa.ini", b"[a.jpg]\nstar=yes\n");
        assert_eq!(stars(dir.path()), vec!["a.jpg"]);
    }

    #[test]
    fn accepts_the_truthy_spellings_and_rejects_everything_else() {
        // Lenient in one direction on purpose: being strict costs a silently missing star,
        // which is the bug this whole feature exists to fix.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=YES\n[b.jpg]\nstar=true\n[c.jpg]\nstar=1\n\
              [d.jpg]\nstar=no\n[e.jpg]\nstar=0\n[f.jpg]\nstar=\n",
        );
        assert_eq!(stars(dir.path()), vec!["a.jpg", "b.jpg", "c.jpg"]);
    }

    #[test]
    fn matches_file_names_case_insensitively() {
        // Picasa came from Windows, where the filesystem is case-insensitive, so an INI
        // written there can disagree in case with the same files read on Linux.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[DSC_0001.JPG]\nstar=yes\n");
        assert_eq!(stars(dir.path()), vec!["dsc_0001.jpg"]);
    }

    #[test]
    fn the_dotted_file_wins_and_the_two_are_never_merged() {
        // This is the test that pins the precedence rule. Merging two disagreeing files
        // would produce a set matching neither, and a star removed in one but left in the
        // other would linger with nothing able to explain it.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        write_file(dir.path(), "Picasa.ini", b"[b.jpg]\nstar=yes\n");
        assert_eq!(
            stars(dir.path()),
            vec!["a.jpg"],
            "b.jpg is starred only in the file that lost"
        );
    }

    #[test]
    fn ignores_properties_that_are_not_star() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nbackuphash=12223\nfaces=rect64(abc)\n",
        );
        assert!(stars(dir.path()).is_empty());
    }

    #[test]
    fn a_directory_with_no_ini_reads_as_nothing_starred() {
        // Some(empty), not None: the directory was read and the answer is "no stars", which
        // is what lets the caller clear stars the INI no longer confirms.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_stars(dir.path()),
            Some(std::collections::HashSet::new())
        );
    }

    #[test]
    fn an_unreadable_directory_reads_as_no_evidence() {
        // None, not Some(empty): failing to read is not evidence that the stars are gone,
        // so the caller must leave existing stars alone.
        let missing = std::path::Path::new("/definitely/not/a/directory/here");
        assert_eq!(read_stars(missing), None);
    }

    #[test]
    fn survives_a_malformed_file() {
        // Real Picasa files carry stray lines, comments and unterminated headers. A strict
        // parser would reject the file outright and lose every star in the folder.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"; a comment\n[unterminated\nstar=yes\nstray line with no equals\n\
              [a.jpg]\nstar=yes\n",
        );
        assert_eq!(
            stars(dir.path()),
            vec!["a.jpg"],
            "the unterminated header claims no file"
        );
    }

    #[test]
    fn survives_a_non_utf8_file() {
        // Picasa files from old Windows locales are not necessarily UTF-8. A section header
        // that survives lossy decoding still matches.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[caf\xe9.jpg]\nstar=yes\n[a.jpg]\nstar=yes\n",
        );
        let found = stars(dir.path());
        assert!(
            found.contains(&"a.jpg".to_string()),
            "a valid section after invalid bytes is still read"
        );
    }

    #[test]
    fn a_duplicate_section_starring_the_file_wins() {
        // Spec §8 names this case. Picasa has been known to write a file twice; whichever
        // way it resolves must be pinned rather than left to whatever the loop happens to
        // do, since a silent change here would move stars.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=no\n[a.jpg]\nstar=yes\n",
        );
        assert_eq!(
            stars(dir.path()),
            vec!["a.jpg"],
            "a star anywhere in the file counts"
        );

        let dir2 = tempfile::tempdir().unwrap();
        write_file(
            dir2.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=yes\n[a.jpg]\nstar=no\n",
        );
        assert_eq!(
            stars(dir2.path()),
            vec!["a.jpg"],
            "inserting into a set never un-stars: a later star=no does not remove an earlier star"
        );
    }

    #[test]
    fn the_read_is_bounded() {
        // Spec §3's cap has to actually bound the read rather than being advisory, the same
        // way `xmp::a_packet_beyond_the_read_cap_is_not_found` pins the XMP one.
        let dir = tempfile::tempdir().unwrap();
        let mut ini = vec![b' '; MAX_INI as usize];
        ini.extend_from_slice(b"\n[a.jpg]\nstar=yes\n");
        write_file(dir.path(), ".picasa.ini", &ini);
        assert!(
            stars(dir.path()).is_empty(),
            "a section past the cap is not read"
        );
    }

    #[test]
    fn a_later_section_does_not_inherit_the_previous_one_s_star() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=yes\n[b.jpg]\nbackuphash=1\n",
        );
        assert_eq!(stars(dir.path()), vec!["a.jpg"]);
    }
}
