//! Reads and writes Picasa's per-directory star flags.
//!
//! Picasa writes one INI per directory — `.picasa.ini` on newer versions, `Picasa.ini` on
//! older ones — with a section per file. photon reads every star from it, and `set_star` is
//! the one place photon writes inside a watched folder: it sets or clears a single `star=`
//! line and leaves every other byte of the file as it found it. Nothing here writes a photo,
//! and nothing else in photon writes an INI.
//!
//! The reader and the writer share one line classifier, `classify`. That is deliberate: a
//! writer with its own idea of what a header or a key looks like would drift from the reader
//! — appending a section the reader already matched, or leaving a `Star = 1` line the reader
//! counts — and the two would then disagree about the file they both own.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// How much of an INI is read. Picasa writes a few lines per photo, so this is far beyond
/// any real file; it exists so a stray enormous one cannot pull unbounded memory through a
/// scan of a hundred thousand photos.
pub const MAX_INI: u64 = 8 * 1024 * 1024;

/// The name photon creates when a folder has no INI yet: the one current Picasa writes.
const NEW_INI: &str = ".picasa.ini";

/// The starred file names in one directory, lowercased.
///
/// `None` means the evidence could not be read — the directory or its INI is unreadable —
/// and the caller must leave existing stars alone, because failing to read is not evidence
/// that the stars are gone. `Some(empty)` is a real answer: the directory was read and
/// nothing is starred, which is what lets a caller clear stars the INI no longer confirms.
pub fn read_stars(dir: &Path) -> Option<HashSet<String>> {
    let Some(path) = ini_path(dir).ok()? else {
        return Some(HashSet::new());
    };
    let bytes = read_capped(&path).ok()?;
    Some(parse_stars(&String::from_utf8_lossy(&bytes)))
}

/// Sets or clears `file_name`'s star in `dir`'s Picasa INI, and returns whether the file
/// changed.
///
/// Edits the same file `read_stars` would read. That matters when a folder carries only an
/// old `Picasa.ini`: creating `.picasa.ini` beside it would make the reader prefer the new
/// file and silently drop every other star in the folder. A folder with no INI gets a
/// `.picasa.ini` when starring, and nothing at all when unstarring — there is no star to
/// clear, so there is nothing to write.
///
/// Every byte outside the affected `star=` lines is preserved, including bytes that are not
/// UTF-8 and the file's own line endings: the INI carries Picasa's face, crop and edit
/// records, and photon has no business rewriting them. A file that already says what was
/// asked is left alone, so its modification time does not move. The write is atomic (a
/// temporary file renamed over the original), so a crash cannot leave a truncated INI.
pub fn set_star(dir: &Path, file_name: &str, starred: bool) -> io::Result<bool> {
    let (path, bytes, template) = match ini_path(dir)? {
        Some(path) => {
            let bytes = read_capped(&path)?;
            let meta = fs::metadata(&path)?;
            (path, bytes, Some(meta))
        }
        None if starred => (dir.join(NEW_INI), Vec::new(), None),
        None => return Ok(false),
    };
    let Some(rewritten) = rewrite(&bytes, file_name, starred) else {
        return Ok(false);
    };
    write_atomically(&path, &rewritten, template.as_ref())?;
    Ok(true)
}

/// The name of the INI `set_star` would write in `dir`, for an error message. Never fails:
/// a directory that cannot be listed is reported under the name that would be created.
pub fn ini_name(dir: &Path) -> String {
    ini_path(dir)
        .ok()
        .flatten()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| NEW_INI.to_string())
}

/// The INI to read, if any. `Ok(None)` when the directory has none; `Err` when the directory
/// itself could not be listed.
///
/// Listing the directory rather than probing two fixed names is what makes the match
/// case-insensitive: a library written on Windows and read on Linux may carry any casing,
/// and a missed file means a whole folder silently loses its stars. `.picasa.ini` wins when
/// both exist, and the two are never merged.
fn ini_path(dir: &Path) -> io::Result<Option<PathBuf>> {
    let mut dotted = None;
    let mut plain = None;
    for entry in fs::read_dir(dir)? {
        // A mid-iteration error here must not be read as "no INI in this directory": that
        // would surface as `Some(empty)` and the reader would clear real stars on it.
        let entry = entry?;
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
    Ok(dotted.or(plain))
}

/// Reads the file, or fails if it is unreadable or larger than `MAX_INI`.
///
/// Reads one byte past the cap so an exactly-`MAX_INI` file can be told apart from a
/// larger one, rather than silently treating "too big to read" as "read, and empty" —
/// the distinction `read_stars`'s `Option` exists to carry, and one the writer needs even
/// more: rewriting a partial read would throw away everything past the cut.
fn read_capped(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut buf = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_INI + 1)
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_INI {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("{} is larger than {MAX_INI} bytes", path.display()),
        ));
    }
    Ok(buf)
}

/// One line of an INI, as both the reader and the writer see it.
enum Line<'a> {
    /// Blank, or a comment.
    Skip,
    /// A section header. `None` when unterminated: `[a.jpg` names no file, and clearing the
    /// section rather than keeping the previous one stops the keys below it attaching to
    /// the wrong photo.
    Header(Option<String>),
    Key {
        key: &'a str,
        value: &'a str,
    },
    /// A line with no `=`. Kept by the writer, ignored by the reader.
    Stray,
}

/// Deliberately lenient: an unterminated header, a stray line or a comment is skipped
/// rather than failing the file, because rejecting a file outright would lose every star in
/// that folder. Section names come back lowercased, the writer's `file_name` is lowercased
/// to match, and both sides trim, so `[ A.JPG ]` and `a.jpg` are the same photo.
fn classify(line: &str) -> Line<'_> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
        return Line::Skip;
    }
    if let Some(rest) = line.strip_prefix('[') {
        return Line::Header(
            rest.strip_suffix(']')
                .map(|name| name.trim().to_lowercase())
                .filter(|name| !name.is_empty()),
        );
    }
    match line.split_once('=') {
        Some((key, value)) => Line::Key {
            key: key.trim(),
            value: value.trim(),
        },
        None => Line::Stray,
    }
}

fn parse_stars(text: &str) -> HashSet<String> {
    let mut stars = HashSet::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        match classify(line) {
            Line::Header(name) => section = name,
            Line::Key { key, value } if is_star_key(key) && is_star(value) => {
                if let Some(name) = &section {
                    stars.insert(name.clone());
                }
            }
            _ => {}
        }
    }
    stars
}

fn is_star_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("star")
}

fn is_star(value: &str) -> bool {
    value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true") || value == "1"
}

/// The file with `file_name`'s star set or cleared, or `None` if it already says so.
///
/// Works on lines of raw bytes: each is decoded lossily only to be classified, and every
/// line the edit does not touch is copied back exactly as it was. Decoding the whole file
/// and re-encoding it would turn every non-UTF-8 byte in an unrelated section into U+FFFD.
///
/// Starring: the first `star` line in the first matching section becomes `star=yes`, and any
/// further `star` lines in matching sections are dropped — Picasa has been known to write a
/// section twice, and the reader counts a star anywhere, so a leftover `star=no` would be
/// harmless today but confusing to whoever reads the file next. A matching section with no
/// `star` line gets one right after its header; no matching section at all means a new one
/// at the end, named with the on-disk casing.
///
/// Unstarring: every `star` line in every matching section is dropped, because the reader
/// treats a star anywhere as starred. The now-empty section stays, as Picasa leaves it.
fn rewrite(bytes: &[u8], file_name: &str, starred: bool) -> Option<Vec<u8>> {
    let eol = line_ending(bytes);
    let wanted = file_name.trim().to_lowercase();
    let mut out = Vec::with_capacity(bytes.len() + 64);
    let mut changed = false;
    let mut in_match = false;
    // Where `star=yes` goes if the first matching section turns out to have no star line:
    // the position in `out` just after that section's header.
    let mut insert_at: Option<usize> = None;
    let mut star_placed = false;

    for chunk in bytes.split_inclusive(|&b| b == b'\n') {
        let (text, line_eol) = split_eol(chunk);
        match classify(&String::from_utf8_lossy(text)) {
            Line::Header(name) => {
                in_match = name.as_deref() == Some(wanted.as_str());
                out.extend_from_slice(chunk);
                if in_match && insert_at.is_none() {
                    // A header that is the file's unterminated last line needs its newline
                    // before anything can follow it, or the inserted key glues onto it.
                    if line_eol.is_empty() {
                        out.extend_from_slice(eol);
                        changed = true;
                    }
                    insert_at = Some(out.len());
                }
            }
            Line::Key { key, value } if in_match && is_star_key(key) => {
                if starred && !star_placed {
                    star_placed = true;
                    if is_star(value) {
                        out.extend_from_slice(chunk);
                    } else {
                        out.extend_from_slice(b"star=yes");
                        out.extend_from_slice(line_eol);
                        changed = true;
                    }
                } else {
                    // Unstarring, or a duplicate star line while starring: dropped.
                    changed = true;
                }
            }
            _ => out.extend_from_slice(chunk),
        }
    }

    if starred && !star_placed {
        let mut key = b"star=yes".to_vec();
        key.extend_from_slice(eol);
        match insert_at {
            Some(at) => {
                out.splice(at..at, key);
            }
            None => {
                if !out.is_empty() && out.last() != Some(&b'\n') {
                    out.extend_from_slice(eol);
                }
                out.extend_from_slice(b"[");
                out.extend_from_slice(file_name.as_bytes());
                out.extend_from_slice(b"]");
                out.extend_from_slice(eol);
                out.extend_from_slice(&key);
            }
        }
        changed = true;
    }

    changed.then_some(out)
}

/// A line as `split_inclusive` yields it, split into its text and its terminator (`\r\n`,
/// `\n`, or nothing for an unterminated last line).
fn split_eol(chunk: &[u8]) -> (&[u8], &[u8]) {
    if let Some(text) = chunk.strip_suffix(b"\r\n") {
        (text, b"\r\n")
    } else if let Some(text) = chunk.strip_suffix(b"\n") {
        (text, b"\n")
    } else {
        (chunk, b"")
    }
}

/// The file's own line ending, from its first newline. A file with none — new, or a single
/// unterminated line — gets CRLF, which is what Picasa itself writes on every platform it
/// ran on; the reader, and every other INI reader, accepts either.
fn line_ending(bytes: &[u8]) -> &'static [u8] {
    match bytes.iter().position(|&b| b == b'\n') {
        Some(0) => b"\n",
        Some(i) if bytes[i - 1] == b'\r' => b"\r\n",
        Some(_) => b"\n",
        None => b"\r\n",
    }
}

/// Windows' hidden attribute. Picasa marks `.picasa.ini` hidden; a rename replaces the
/// file's attributes along with its contents, so the temporary is created with the same
/// bit. A literal rather than a dependency: photon takes no Windows binding crate for one
/// constant.
#[cfg(windows)]
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

/// Source of unique temporary names within one process; the pid covers across processes.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Removes the temporary file unless the rename that consumes it succeeded.
struct RemoveOnDrop<'a>(Option<&'a Path>);

impl Drop for RemoveOnDrop<'_> {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Writes `bytes` to `path` by way of a temporary file in the same directory, fsynced and
/// renamed over the original. `template` is the original's metadata, whose permissions
/// (unix) or hidden attribute (Windows) the new file keeps.
///
/// Hand-rolled rather than `tempfile::NamedTempFile`, whose builder cannot set Windows
/// attributes at creation, and std can set them at no other time. The name is a dotfile
/// with no media extension, so the scanner's walk ignores it and so does Picasa.
fn write_atomically(path: &Path, bytes: &[u8], template: Option<&fs::Metadata>) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("INI path has no parent directory"))?;
    let base = path
        .file_name()
        .ok_or_else(|| io::Error::other("INI path has no file name"))?
        .to_string_lossy();
    let (tmp_path, mut file) = loop {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!("{base}.photon-{}-{seq}.tmp", std::process::id()));
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(windows)]
        if let Some(meta) = template {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            if meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0 {
                opts.attributes(FILE_ATTRIBUTE_HIDDEN);
            }
        }
        match opts.open(&candidate) {
            Ok(file) => break (candidate, file),
            // A leftover from a crashed photon, or a sibling write in flight: try the next
            // name rather than clobbering it.
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    };
    let mut guard = RemoveOnDrop(Some(&tmp_path));
    #[cfg(unix)]
    if let Some(meta) = template {
        fs::set_permissions(&tmp_path, meta.permissions())?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp_path, path)?;
    guard.0 = None;
    // Make the rename itself durable. Best effort: it is only the ordering of two writes
    // that are both already on disk, and Windows cannot open a directory this way.
    #[cfg(unix)]
    {
        let _ = fs::File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
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
        // that survives lossy decoding still matches, with the invalid byte replaced by
        // U+FFFD rather than the section (or the rest of the file) being dropped.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[caf\xe9.jpg]\nstar=yes\n[a.jpg]\nstar=yes\n",
        );
        assert_eq!(
            stars(dir.path()),
            vec!["a.jpg", "caf\u{FFFD}.jpg"],
            "the invalid byte is replaced, not dropped, and does not disturb the section after it"
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
    fn an_oversized_ini_reads_as_no_evidence_rather_than_a_partial_parse() {
        // Spec §3's cap has to actually bound the read rather than being advisory, the same
        // way `xmp::a_packet_beyond_the_read_cap_is_not_found` pins the XMP one. But unlike
        // that reader, something here acts on absence: the scanner clears every star a
        // folder's INI does not confirm. A `take(MAX_INI)`-style truncation would read a
        // too-big file as a *partial* one and silently un-star whatever fell past the cut,
        // so an oversized file must come back as `None` (no evidence) rather than
        // `Some(partial-or-empty)`.
        let dir = tempfile::tempdir().unwrap();
        let mut ini = vec![b' '; MAX_INI as usize + 1];
        ini.extend_from_slice(b"\n[a.jpg]\nstar=yes\n");
        write_file(dir.path(), ".picasa.ini", &ini);
        assert_eq!(
            read_stars(dir.path()),
            None,
            "too big to read is not evidence that nothing is starred"
        );

        // ...but a file that ends just inside the cap is read in full.
        let dir2 = tempfile::tempdir().unwrap();
        let mut ini2 = vec![b' '; MAX_INI as usize - 18];
        ini2.extend_from_slice(b"\n[a.jpg]\nstar=yes\n");
        assert_eq!(ini2.len() as u64, MAX_INI);
        write_file(dir2.path(), ".picasa.ini", &ini2);
        assert_eq!(stars(dir2.path()), vec!["a.jpg"]);
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

    // ---- the writer ----

    fn ini(dir: &std::path::Path, name: &str) -> Vec<u8> {
        std::fs::read(dir.join(name)).unwrap()
    }

    fn entries(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn starring_with_no_ini_creates_a_dotted_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(set_star(dir.path(), "a.jpg", true).unwrap());
        assert_eq!(entries(dir.path()), vec![".picasa.ini"]);
        assert_eq!(ini(dir.path(), ".picasa.ini"), b"[a.jpg]\r\nstar=yes\r\n");
        assert_eq!(stars(dir.path()), vec!["a.jpg"]);
    }

    #[test]
    fn unstarring_in_a_folder_without_an_ini_creates_nothing() {
        // There is no star to clear, so there is nothing to write - and a folder photon has
        // never starred anything in must not grow a file because someone clicked ☆ twice.
        let dir = tempfile::tempdir().unwrap();
        assert!(!set_star(dir.path(), "a.jpg", false).unwrap());
        assert!(entries(dir.path()).is_empty());
    }

    #[test]
    fn starring_edits_picasa_ini_in_place_when_only_the_undotted_file_exists() {
        // The reader prefers `.picasa.ini` and never merges the two, so creating the dotted
        // file here would make every star in `Picasa.ini` vanish from photon: the reader
        // would find the new file, read `[a]`, and clear b's star on the next scan.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "Picasa.ini", b"[b.jpg]\nstar=yes\n");
        assert!(set_star(dir.path(), "a.jpg", true).unwrap());
        assert_eq!(entries(dir.path()), vec!["Picasa.ini"]);
        assert_eq!(stars(dir.path()), vec!["a.jpg", "b.jpg"]);
    }

    #[test]
    fn when_both_files_exist_the_dotted_one_is_edited() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[b.jpg]\nstar=yes\n");
        write_file(dir.path(), "Picasa.ini", b"[c.jpg]\nstar=yes\n");
        assert!(set_star(dir.path(), "a.jpg", true).unwrap());
        assert_eq!(ini(dir.path(), "Picasa.ini"), b"[c.jpg]\nstar=yes\n");
        assert_eq!(stars(dir.path()), vec!["a.jpg", "b.jpg"]);
    }

    #[test]
    fn starring_replaces_the_existing_star_line_and_preserves_every_other_byte() {
        // The INI carries Picasa's own records - hashes, faces, edits - and bytes that are
        // not UTF-8. Decoding the whole file lossily and re-encoding it would turn the \xe9
        // into U+FFFD; reformatting would move Picasa's lines. Only the one line may change.
        let dir = tempfile::tempdir().unwrap();
        let before: &[u8] = b"; written by Picasa\r\n[caf\xe9.jpg]\r\nname=caf\xe9\r\n\
                             [a.jpg]\r\nbackuphash=12223\r\nstar=no\r\nfaces=rect64(abc)\r\n\
                             [b.jpg]\r\nstar=yes\r\n";
        write_file(dir.path(), ".picasa.ini", before);
        assert!(set_star(dir.path(), "a.jpg", true).unwrap());
        let after: &[u8] = b"; written by Picasa\r\n[caf\xe9.jpg]\r\nname=caf\xe9\r\n\
                            [a.jpg]\r\nbackuphash=12223\r\nstar=yes\r\nfaces=rect64(abc)\r\n\
                            [b.jpg]\r\nstar=yes\r\n";
        assert_eq!(ini(dir.path(), ".picasa.ini"), after);
    }

    #[test]
    fn starring_inserts_after_the_header_when_the_section_has_no_star_line() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nbackuphash=12223\n[b.jpg]\nbackuphash=1\n",
        );
        assert!(set_star(dir.path(), "a.jpg", true).unwrap());
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b"[a.jpg]\nstar=yes\nbackuphash=12223\n[b.jpg]\nbackuphash=1\n"
        );
    }

    #[test]
    fn starring_appends_a_section_when_the_file_has_none_for_the_photo() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[b.jpg]\nstar=yes\n");
        assert!(set_star(dir.path(), "DSC_0001.JPG", true).unwrap());
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b"[b.jpg]\nstar=yes\n[DSC_0001.JPG]\nstar=yes\n",
            "the new section carries the on-disk casing, as Picasa's own would"
        );
    }

    #[test]
    fn unstarring_removes_every_star_line_across_duplicate_sections() {
        // The reader counts a star anywhere in the file, so removing only the first would
        // leave the photo starred as far as photon - and Picasa - are concerned.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=yes\nbackuphash=1\n[b.jpg]\nstar=yes\n[A.JPG]\nStar = 1\n",
        );
        assert!(set_star(dir.path(), "a.jpg", false).unwrap());
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b"[a.jpg]\nbackuphash=1\n[b.jpg]\nstar=yes\n[A.JPG]\n"
        );
        assert_eq!(stars(dir.path()), vec!["b.jpg"]);
    }

    #[test]
    fn section_headers_match_case_insensitively_so_no_duplicate_section_is_added() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[dsc_0001.jpg]\nbackuphash=1\n");
        assert!(set_star(dir.path(), "DSC_0001.JPG", true).unwrap());
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b"[dsc_0001.jpg]\nstar=yes\nbackuphash=1\n"
        );
    }

    #[test]
    fn the_writer_and_reader_agree_on_odd_headers() {
        // Both go through `classify`, which trims inside the brackets. If the writer ever
        // grew its own header matching it would append a second section here, and the two
        // halves of this module would disagree about which line is the photo's.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b" [ a.jpg ] \nbackuphash=1\n");
        assert!(set_star(dir.path(), "a.jpg", true).unwrap());
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b" [ a.jpg ] \nstar=yes\nbackuphash=1\n"
        );
        assert_eq!(stars(dir.path()), vec!["a.jpg"]);
    }

    #[test]
    fn a_file_that_already_says_what_we_want_is_not_rewritten() {
        // `false` and identical bytes, rather than an mtime check: mtime granularity varies
        // by filesystem, and the point is that a no-op never touches the file at all.
        let dir = tempfile::tempdir().unwrap();
        let before: &[u8] = b"[a.jpg]\nstar=yes\n[b.jpg]\nbackuphash=1\n";
        write_file(dir.path(), ".picasa.ini", before);
        assert!(!set_star(dir.path(), "a.jpg", true).unwrap());
        assert!(!set_star(dir.path(), "b.jpg", false).unwrap());
        assert!(
            !set_star(dir.path(), "c.jpg", false).unwrap(),
            "no section at all is already unstarred"
        );
        assert_eq!(ini(dir.path(), ".picasa.ini"), before);
        assert_eq!(
            entries(dir.path()),
            vec![".picasa.ini"],
            "no temporary left behind"
        );
    }

    #[test]
    fn line_endings_follow_the_file() {
        // LF stays LF...
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[b.jpg]\nstar=yes\n");
        set_star(dir.path(), "a.jpg", true).unwrap();
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b"[b.jpg]\nstar=yes\n[a.jpg]\nstar=yes\n"
        );

        // ...an unterminated last line is terminated before a section is appended, or the
        // new header would glue onto it...
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[b.jpg]\r\nbackuphash=1");
        set_star(dir.path(), "a.jpg", true).unwrap();
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b"[b.jpg]\r\nbackuphash=1\r\n[a.jpg]\r\nstar=yes\r\n"
        );

        // ...including when that last line is the matching header itself...
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[a.jpg]");
        set_star(dir.path(), "a.jpg", true).unwrap();
        assert_eq!(ini(dir.path(), ".picasa.ini"), b"[a.jpg]\r\nstar=yes\r\n");

        // ...and a file with no newline to learn from gets CRLF, like Picasa's own.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"");
        set_star(dir.path(), "a.jpg", true).unwrap();
        assert_eq!(ini(dir.path(), ".picasa.ini"), b"[a.jpg]\r\nstar=yes\r\n");
    }

    #[test]
    fn an_oversized_ini_is_refused_untouched() {
        // The reader's cap bounds memory; the writer's bounds damage. Rewriting from a
        // partial read would drop everything past the cut, so a too-big file is an error
        // and stays exactly as it was, with no temporary left beside it.
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b' '; MAX_INI as usize + 1];
        write_file(dir.path(), ".picasa.ini", &big);
        let err = set_star(dir.path(), "a.jpg", true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::FileTooLarge);
        assert_eq!(entries(dir.path()), vec![".picasa.ini"]);
        assert_eq!(ini(dir.path(), ".picasa.ini"), big);
    }

    #[test]
    #[cfg(unix)]
    fn a_failed_write_leaves_no_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[b.jpg]\nstar=yes\n");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = set_star(dir.path(), "a.jpg", true);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(entries(dir.path()), vec![".picasa.ini"]);
        assert_eq!(ini(dir.path(), ".picasa.ini"), b"[b.jpg]\nstar=yes\n");
    }

    #[test]
    fn a_failed_rename_leaves_no_temp_file() {
        // The failure that can strand a temporary is one *after* it exists: the rename. A
        // directory squatting on the INI's name makes it fail on every platform, and the
        // guard is the only thing that removes the temporary on that path.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(".picasa.ini");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomically(&target, b"[a.jpg]\r\nstar=yes\r\n", None).is_err());
        assert_eq!(entries(dir.path()), vec![".picasa.ini"]);
    }

    #[test]
    #[cfg(unix)]
    fn the_ini_s_permissions_survive_a_rewrite() {
        // A rename replaces the file wholesale, so the temporary has to carry the original's
        // mode; a fresh file under the default umask would come out 0o644.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), ".picasa.ini", b"[b.jpg]\nstar=yes\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        set_star(dir.path(), "a.jpg", true).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    #[cfg(windows)]
    fn the_hidden_attribute_survives_a_rewrite() {
        // Picasa marks `.picasa.ini` hidden. A rename replaces the attributes with the
        // temporary's, so the temporary is created hidden when the original was.
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".picasa.ini");
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .attributes(FILE_ATTRIBUTE_HIDDEN)
                .open(&path)
                .unwrap();
            f.write_all(b"[b.jpg]\r\nstar=yes\r\n").unwrap();
        }
        set_star(dir.path(), "a.jpg", true).unwrap();
        assert_ne!(
            std::fs::metadata(&path).unwrap().file_attributes() & FILE_ATTRIBUTE_HIDDEN,
            0
        );
        assert_eq!(stars(dir.path()), vec!["a.jpg", "b.jpg"]);
    }

    #[test]
    fn a_star_set_by_photon_reads_back_and_clears_again() {
        let dir = tempfile::tempdir().unwrap();
        assert!(set_star(dir.path(), "a.jpg", true).unwrap());
        assert_eq!(stars(dir.path()), vec!["a.jpg"]);
        assert!(set_star(dir.path(), "a.jpg", false).unwrap());
        assert!(stars(dir.path()).is_empty());
        assert_eq!(
            ini(dir.path(), ".picasa.ini"),
            b"[a.jpg]\r\n",
            "the section stays, as Picasa leaves it"
        );
    }
}
