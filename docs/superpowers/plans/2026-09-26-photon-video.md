# Video Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Index `.mp4`/`.m4v`/`.mov`/`.webm` files beside photos, give them poster-frame tiles made by the webview, and play them in the viewer over a token-guarded loopback HTTP server.

**Architecture:** photon-core learns `MediaKind::Video`, reads container metadata in pure Rust (`photon_core::video`), and keeps a second `ThumbQueue` for videos that no worker pops - the UI drains it through IPC, draws a frame with a hidden `<video>`, and hands back a JPEG. photon-app gains `media_server.rs` (tiny_http, `127.0.0.1:0`) which streams video bytes with uncapped ranges. The UI gains a thumbnailer factory, a play badge on tiles, and a `<video>` in the viewer.

**Tech Stack:** Rust (rusqlite, image, jiff, tiny_http, getrandom), Tauri 2, Svelte 5 runes + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-09-26-photon-video-design.md` - read it first; this plan argues from it.

## Global Constraints

- **No native library dependencies.** New crates, all pure Rust: `jiff` (photon-core), `tiny_http` and `getrandom` (photon-app). Nothing that links a system C library.
- **photon never writes to, moves or deletes media files.** Videos are opened read-only, by the scanner and by the media server alike.
- **The Rust gate before every commit:** `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate before every commit touching `ui/`:** `npm run check` (0 errors, 0 warnings) and `npm test`, from the repo root.
- **Every new test is shown to fail with its change reverted** (exact revert, not a loose `sed`); a probe that passes is a finding. Where a change has no seam (Svelte effect wiring), the commit message says so.
- **IPC commands touch three files in order:** `commands.rs` (logic, `&Engine`), `ipc.rs` (`#[tauri::command(async)]` wrapper), `app.rs` (`generate_handler!`), plus an answer in `crates/xtask/screenshots/mock.js` (`canned` or `SILENT`).
- **TS mirrors change in the same commit as the Rust struct**, including every literal in `ui/src/lib/library.test.ts`.
- **Schema 20.** Update the literal `19` → `20` in `library/mod.rs` (opened version and `SchemaTooNew { supported }`); never loosen them to `MIGRATIONS.len()`.
- **Extensions:** exactly `mp4`, `m4v`, `mov`, `webm`.
- **Loopback server:** binds `127.0.0.1:0` only; 128-bit token per launch compared in constant time; `Host` must equal `127.0.0.1:<port>`; `Access-Control-Allow-Origin` is `tauri://localhost` (`http://tauri.localhost` on Windows), never `*`; `Cache-Control: no-store`; open-ended ranges answered to their end, never capped.
- **Poster frames:** seek to `min(1 s, duration / 10)`; drawn at up to `ThumbSize::Preview.max_edge()` (1600 px); a 15 s budget per job; one job at a time.
- **Comments carry reasoning, not mechanics**, in the style of the surrounding code.

**Deviations from the spec, decided while planning:**
1. The spec's `videoJobs` event is replaced by a **long-poll**: `next_video_job` blocks up to 25 s for a job. An event plus a `null` answer has a lost-wakeup window (the job lands between the `null` and the listener re-arming); a long-poll has none, and needs no event plumbing.
2. The tile's "film placeholder" is the existing `play` icon drawn large; no new icon is vendored.
3. `photon://image/<id>` refuses a video (404). The spec did not mention it, but that handler reads the whole file into memory, and Compare and the viewer's preload would otherwise ask it for a multi-GB video.

## Review Focus

1. **A phone video whose `moov` comes after a large `mdat`** (every non-"fast start" camera file) - metadata must still be found, by seeking rather than reading. Pinned in Task 2 (`finds_moov_after_a_large_mdat`).
2. **A file named `.mp4` that is empty, truncated or not a video at all** - indexed with the mtime date and aspect 1.0, never a panic. Pinned in Task 2 (`garbage_and_truncated_files_read_as_nothing`) and Task 4 (`a_garbage_mp4_is_still_indexed`).
3. **A library opened on a Linux without the GStreamer plugins** - every video tile's thumbnail request must answer at once, not hold a protocol thread 30 s each. Pinned in Task 3 (`a_video_request_without_a_session_answers_at_once`).
4. **A video rewritten or deleted while its frame is being made** - the frame is refused and anyone waiting on it is released. Pinned in Task 3 (`a_frame_for_a_changed_video_is_refused_and_releases_waiters`).
5. **A Range request against a zero-byte video** - `len - 1` underflows in a naive handler. Pinned in Task 5 (`a_range_on_an_empty_file_is_unsatisfiable`).

---

## File Structure

**photon-core**
- `src/media.rs` - `MediaKind::Video`, the four extensions.
- `src/library/schema.rs` - migration 20 (`items.duration_ms`).
- `src/library/items.rs` - `NewItem`/`Item`/grid rows carry `duration_ms`; `pending_thumb_ids(kind)`.
- `src/grid.rs` - `GridEntry.duration_ms`.
- `src/video.rs` (new) - container metadata: ISO-BMFF box walk, EBML walk, capture-date precedence.
- `src/testutil.rs` - `mp4_box`, `Mp4Spec`, `mp4_bytes` fixture builders.
- `src/scanner.rs` - `describe()` branches on kind.
- `src/search.rs` - `video` / `photo` terms.
- `src/library/similar.rs` - videos out of the look-alike pass.
- `src/thumbs/queue.rs` - `pop_until`.
- `src/thumbs/cache.rs` - `render_frame`.
- `src/thumbs/service.rs` - the video queue, claims, frames, failures, session, crash guard.
- `src/error.rs` - `NotAPhoto`.

**photon-app**
- `src/media_server.rs` (new) - the loopback server.
- `src/commands.rs`, `src/ipc.rs`, `src/app.rs` - five new commands, `ViewerItem.kind`/`durationMs`, refusals.
- `src/engine.rs` - edits refuse videos.
- `src/protocol.rs` - `image/<id>` refuses videos.
- `src/error.rs` - `notAPhoto` kind.
- `tauri.conf.json` - CSP `media-src`, `.deb` depends, AppImage media framework.

**ui**
- `src/lib/api.ts` - types and commands.
- `src/lib/video.ts` (new) - `posterTime`, `formatDuration`, `mediaSupported`, `videoUrl`, error classes.
- `src/lib/video-state.svelte.ts` (new) - the shared `{ base, supported }`.
- `src/lib/video-thumbnailer.svelte.ts` (new) - the job loop.
- `src/lib/video-grab.ts` (new) - the DOM half: load, seek, draw, encode.
- `src/lib/slideshow-order.ts` (new) - `nextStill`.
- `src/App.svelte`, `src/components/Tile.svelte`, `src/components/Viewer.svelte`, `src/components/Compare.svelte`.

**repo**
- `crates/xtask/screenshots/mock.js`, `crates/xtask/src/screenshots.rs` - answers and shots.
- `README.md`, `THIRD-PARTY-NOTICES.md`.

---

### Task 1: `MediaKind::Video`, schema 20, and `duration_ms` through the rows

**Files:**
- Modify: `crates/photon-core/src/media.rs`
- Modify: `crates/photon-core/src/library/schema.rs` (append to `MIGRATIONS`)
- Modify: `crates/photon-core/src/library/mod.rs:176,192` (the `19` literals)
- Modify: `crates/photon-core/src/library/items.rs` (`NewItem`, `Item`, `insert_items`, `update_items`, `item()`, `row_to_item`, `GRID_COLUMNS`, `GRID_COLUMN_COUNT`, `map_grid_row`)
- Modify: `crates/photon-core/src/grid.rs:83` (`GridEntry`) and its test literals
- Modify: `crates/photon-core/src/testutil.rs:32` (`new_item`) and every other `NewItem { … }` literal (31 today: `grep -rn "NewItem {" crates`)

**Interfaces:**
- Produces: `MediaKind::Video` (`to_db` = 1, serde `"video"`); `NewItem.duration_ms: Option<i64>`; `Item.duration_ms: Option<i64>`; `GridEntry.duration_ms: Option<i64>` (serde `durationMs`).
- `MediaKind::from_path` is **not** changed here - videos start being indexed in Task 4, after the thumbnail workers learn to leave them alone (Task 3).

- [ ] **Step 1: Write the failing tests**

In `media.rs`, add a test module (or extend the existing one):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_round_trips_through_the_database_value() {
        assert_eq!(MediaKind::Video.to_db(), 1);
        assert_eq!(MediaKind::from_db(1), Some(MediaKind::Video));
        assert_eq!(MediaKind::from_db(0), Some(MediaKind::Image));
        assert_eq!(MediaKind::from_db(2), None);
    }

    #[test]
    fn video_serialises_as_the_ui_expects() {
        assert_eq!(serde_json::to_string(&MediaKind::Video).unwrap(), "\"video\"");
    }
}
```

In `library/items.rs` tests, add:

```rust
#[test]
fn a_video_row_keeps_its_kind_and_duration() {
    let (_dir, lib) = temp_library();
    let (_, folder) = seed_folder(&lib, Path::new("/photos"));
    let mut video = new_item(folder, "/photos/clip.mp4", 100);
    video.kind = MediaKind::Video;
    video.duration_ms = Some(83_000);
    let id = lib.insert_items(&[video.clone()]).unwrap()[0];

    let item = lib.item(id).unwrap().unwrap();
    assert_eq!(item.kind, MediaKind::Video);
    assert_eq!(item.duration_ms, Some(83_000));
    let row = lib.grid_entries(GridView::All).unwrap().into_iter().find(|e| e.id == id).unwrap();
    assert_eq!(row.kind, MediaKind::Video);
    assert_eq!(row.duration_ms, Some(83_000));

    // A rewrite carries the new running time; a photo's stays NULL.
    video.duration_ms = Some(90_000);
    lib.update_items(&[(id, video)]).unwrap();
    assert_eq!(lib.item(id).unwrap().unwrap().duration_ms, Some(90_000));
    let photo = lib.insert_items(&[new_item(folder, "/photos/a.jpg", 100)]).unwrap()[0];
    assert_eq!(lib.item(photo).unwrap().unwrap().duration_ms, None);
}
```

(Use whatever the file's existing tests call to read grid rows - `grep -n "fn grid_entries\|pub fn entries_for" crates/photon-core/src/library/items.rs` - and import `Path`, `GridView`, `MediaKind` as the module's other tests do.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p photon-core video_round_trips a_video_row_keeps`
Expected: compile errors (`no variant Video`, `no field duration_ms`) - not yet proof; proof comes at Step 5.

- [ ] **Step 3: Implement**

`media.rs`:

```rust
/// What kind of media a library item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    /// Played and poster-framed by the webview, never decoded by photon
    /// (spec `2026-09-26-photon-video-design.md`).
    Video,
}
```

`to_db`: `Self::Video => 1`. `from_db`: `1 => Some(Self::Video)`.

`schema.rs`, append to `MIGRATIONS`:

```rust
    r#"
-- A video's running time in milliseconds, from its container (`photon_core::video`). NULL for
-- every photo, and for a video whose container did not say.
ALTER TABLE items ADD COLUMN duration_ms INTEGER;
"#,
```

`library/mod.rs`: both `19` → `20`.

`items.rs`:
- `NewItem` gains, after `caption`:
  ```rust
      /// A video's running time; `None` for a photo, or a video whose container did not say.
      pub duration_ms: Option<i64>,
  ```
- `Item` gains `pub duration_ms: Option<i64>` (last field).
- `insert_items`: add `duration_ms` to the column list and `?21` to `VALUES` (the `coalesce(... hidden ...)` stays last), pass `it.duration_ms`.
- `update_items`: add `duration_ms = ?21` to the `SET` list and pass `it.duration_ms`.
- `item()`: select `…, edit_turns, edit_crop, hidden, duration_ms`; `row_to_item` reads `duration_ms: r.get(24)?`.
- `GRID_COLUMNS`: append `i.duration_ms` **after** the `has_copies` subquery, i.e. change the tail to `")", ", i.duration_ms"` in the `concat!`; `GRID_COLUMN_COUNT` → `15`; `map_grid_row` sets `duration_ms: r.get(14)?`.

`grid.rs` `GridEntry`, after `kind`:

```rust
    /// A video's running time, for the tile's badge; `None` for a photo.
    pub duration_ms: Option<i64>,
```

`GridEntry` is `Copy` and 100k of them stay in memory: `Option<i64>` adds 16 bytes each, 1.6 MB at 100k - accepted, and said in the commit message.

`testutil::new_item` and every other `NewItem` literal: add `duration_ms: None,`. Every `GridEntry` literal in `grid.rs` tests: add `duration_ms: None,`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p photon-core`
Expected: PASS, including `open_creates_schema_and_is_idempotent` and `refuses_newer_schema` at 20.

- [ ] **Step 5: Probe**

Revert only `duration_ms = ?21` in `update_items` (leave the parameter bound): `a_video_row_keeps_its_kind_and_duration` must fail on the `90_000` assertion. Revert only the `1 => Some(Self::Video)` arm: `video_round_trips…` must fail. Restore both exactly and `touch` the files (cargo can keep running a probed build otherwise).

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add -A crates/photon-core
git commit -m "feat(core): a video media kind, and items.duration_ms (schema 20)"
```

---

### Task 2: `photon_core::video` - container metadata in pure Rust

**Files:**
- Create: `crates/photon-core/src/video.rs`
- Modify: `crates/photon-core/src/lib.rs` (`pub mod video;`)
- Modify: `crates/photon-core/Cargo.toml` (`jiff = "0.2.35"`)
- Modify: `crates/photon-core/src/testutil.rs` (fixture builders)

**Interfaces:**
- Consumes: `metadata::naive_to_unix(i64, u32, u32, u32, u32, u32) -> i64`, `metadata::plausible_taken_at(i64, i64) -> bool`, `crate::now_ms()`.
- Produces:
  ```rust
  pub struct VideoMeta { pub width: u32, pub height: u32, pub duration_ms: Option<i64>,
                         pub taken_at: Option<i64>, pub make: Option<String>, pub model: Option<String> }
  pub fn read_meta(path: &Path) -> VideoMeta;                       // system time zone, now
  pub(crate) fn read_meta_with(path: &Path, tz: &jiff::tz::TimeZone, now: i64) -> VideoMeta;
  ```
  `width`/`height` are **as displayed** (swapped for a quarter-turn matrix); `taken_at` is naive-as-UTC seconds like `items.taken_at`. `testutil`: `mp4_box(&[u8; 4], &[u8]) -> Vec<u8>`, `Mp4Spec<'a>`, `mp4_bytes(&Mp4Spec) -> Vec<u8>`.

- [ ] **Step 1: Add the fixture builders to `testutil.rs`**

```rust
/// One ISO-BMFF box: a 32-bit size, the four-character type, the body.
pub fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

/// What `mp4_bytes` writes. Hand-built, so no encoder is ever in the test path.
#[derive(Clone, Debug, Default)]
pub struct Mp4Spec<'a> {
    pub width: u32,
    pub height: u32,
    /// The video track matrix's turn in degrees: 0, 90, 180 or 270.
    pub rotation: u32,
    pub timescale: u32,
    pub duration: u32,
    /// Seconds since 1904-01-01 UTC, as `mvhd` holds it; 0 is "not set".
    pub mvhd_created: u32,
    pub apple_date: Option<&'a str>,
    pub make: Option<&'a str>,
    pub model: Option<&'a str>,
    /// Put a sound track ahead of the video track.
    pub audio_first: bool,
    /// Bytes of `mdat` before `moov`, as a camera that does not "fast start" writes them.
    pub mdat_before: usize,
}

fn mp4_full(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut with_version = vec![0, 0, 0, 0];
    with_version.extend_from_slice(body);
    mp4_box(kind, &with_version)
}

fn mp4_track(handler: &[u8; 4], width: u32, height: u32, rotation: u32) -> Vec<u8> {
    const ONE: i32 = 0x0001_0000;
    let (a, b, c, d) = match rotation {
        90 => (0, ONE, -ONE, 0),
        180 => (-ONE, 0, 0, -ONE),
        270 => (0, -ONE, ONE, 0),
        _ => (ONE, 0, 0, ONE),
    };
    let mut tkhd = vec![0u8; 20]; // created, modified, track id, reserved, duration
    tkhd.extend_from_slice(&[0u8; 16]); // reserved, layer, group, volume, reserved
    for v in [a, b, 0, c, d, 0, 0, 0, 0x4000_0000] {
        tkhd.extend_from_slice(&v.to_be_bytes());
    }
    tkhd.extend_from_slice(&(width << 16).to_be_bytes());
    tkhd.extend_from_slice(&(height << 16).to_be_bytes());
    let mut hdlr = vec![0u8; 4];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0u8; 13]);
    let mdia = mp4_box(b"mdia", &mp4_full(b"hdlr", &hdlr));
    mp4_box(b"trak", &[mp4_full(b"tkhd", &tkhd), mdia].concat())
}

fn mp4_apple_meta(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut hdlr = vec![0u8; 4];
    hdlr.extend_from_slice(b"mdta");
    hdlr.extend_from_slice(&[0u8; 13]);
    let mut keys = (entries.len() as u32).to_be_bytes().to_vec();
    let mut ilst = Vec::new();
    for (i, (key, value)) in entries.iter().enumerate() {
        keys.extend_from_slice(&((key.len() + 8) as u32).to_be_bytes());
        keys.extend_from_slice(b"mdta");
        keys.extend_from_slice(key.as_bytes());
        let mut data = 1u32.to_be_bytes().to_vec(); // well-known type 1: UTF-8
        data.extend_from_slice(&[0u8; 4]); // locale
        data.extend_from_slice(value.as_bytes());
        ilst.extend(mp4_box(&((i + 1) as u32).to_be_bytes(), &mp4_box(b"data", &data)));
    }
    // QuickTime's `meta` is a plain box, not a full box: no version and flags.
    mp4_box(
        b"meta",
        &[mp4_full(b"hdlr", &hdlr), mp4_full(b"keys", &keys), mp4_box(b"ilst", &ilst)].concat(),
    )
}

pub fn mp4_bytes(spec: &Mp4Spec<'_>) -> Vec<u8> {
    let mut mvhd = Vec::new();
    for v in [spec.mvhd_created, spec.mvhd_created, spec.timescale, spec.duration] {
        mvhd.extend_from_slice(&v.to_be_bytes());
    }
    mvhd.extend_from_slice(&[0u8; 80]);
    let mut moov = mp4_full(b"mvhd", &mvhd);
    let video = mp4_track(b"vide", spec.width, spec.height, spec.rotation);
    let audio = mp4_track(b"soun", 0, 0, 0);
    if spec.audio_first {
        moov.extend(audio);
        moov.extend(video);
    } else {
        moov.extend(video);
        moov.extend(audio);
    }
    let apple: Vec<(&str, &str)> = [
        ("com.apple.quicktime.creationdate", spec.apple_date),
        ("com.apple.quicktime.make", spec.make),
        ("com.apple.quicktime.model", spec.model),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.map(|v| (k, v)))
    .collect();
    if !apple.is_empty() {
        moov.extend(mp4_apple_meta(&apple));
    }
    let mut file = mp4_box(b"ftyp", b"qt  \0\0\0\0qt  ");
    if spec.mdat_before > 0 {
        file.extend(mp4_box(b"mdat", &vec![0u8; spec.mdat_before]));
    }
    file.extend(mp4_box(b"moov", &moov));
    if spec.mdat_before == 0 {
        file.extend(mp4_box(b"mdat", &[0u8; 16]));
    }
    file
}
```

- [ ] **Step 2: Write the failing tests** (bottom of `video.rs`, with the module stubbed as `pub struct VideoMeta` + `todo!()` functions so it compiles)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::naive_to_unix;
    use crate::testutil::{Mp4Spec, mp4_bytes, write_file};
    use jiff::tz::{self, TimeZone};

    const NOW: i64 = 1_800_000_000; // 2027
    /// 2024-06-15 10:30:45 UTC in `mvhd`'s 1904 epoch.
    const MVHD_2024_06_15_1030_UTC: u32 = 3_801_292_245;

    fn read(bytes: &[u8], tz: &TimeZone) -> VideoMeta {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "clip.mov", bytes);
        read_meta_with(&path, tz, NOW)
    }

    fn plus_two() -> TimeZone {
        TimeZone::fixed(tz::offset(2))
    }

    fn base() -> Mp4Spec<'static> {
        Mp4Spec { width: 1920, height: 1080, timescale: 600, duration: 600 * 83, ..Mp4Spec::default() }
    }

    #[test]
    fn reads_size_and_running_time() {
        let meta = read(&mp4_bytes(&base()), &plus_two());
        assert_eq!((meta.width, meta.height), (1920, 1080));
        assert_eq!(meta.duration_ms, Some(83_000));
    }

    #[test]
    fn a_quarter_turn_swaps_the_displayed_size_and_a_half_turn_does_not() {
        for (rotation, size) in [(90, (1080, 1920)), (270, (1080, 1920)), (180, (1920, 1080))] {
            let meta = read(&mp4_bytes(&Mp4Spec { rotation, ..base() }), &plus_two());
            assert_eq!((meta.width, meta.height), size, "{rotation}°");
        }
    }

    #[test]
    fn the_video_track_is_found_behind_a_sound_track() {
        let meta = read(&mp4_bytes(&Mp4Spec { audio_first: true, ..base() }), &plus_two());
        assert_eq!((meta.width, meta.height), (1920, 1080));
    }

    #[test]
    fn apples_date_is_taken_as_wall_clock_and_wins_over_mvhd() {
        let spec = Mp4Spec {
            apple_date: Some("2024-06-15T12:30:45+0200"),
            mvhd_created: MVHD_2024_06_15_1030_UTC - 3600, // an hour off, to tell them apart
            ..base()
        };
        // Read in a zone that is *not* the one the offset names: the wall clock must be
        // taken as written, not converted.
        let meta = read(&mp4_bytes(&spec), &TimeZone::fixed(tz::offset(-5)));
        assert_eq!(meta.taken_at, Some(naive_to_unix(2024, 6, 15, 12, 30, 45)));
    }

    #[test]
    fn an_mvhd_time_is_converted_to_the_zones_wall_clock() {
        let spec = Mp4Spec { mvhd_created: MVHD_2024_06_15_1030_UTC, ..base() };
        let meta = read(&mp4_bytes(&spec), &plus_two());
        assert_eq!(meta.taken_at, Some(naive_to_unix(2024, 6, 15, 12, 30, 45)));
    }

    #[test]
    fn a_zero_mvhd_time_is_no_date() {
        assert_eq!(read(&mp4_bytes(&base()), &plus_two()).taken_at, None);
    }

    #[test]
    fn an_implausible_apple_date_falls_through_to_mvhd() {
        let spec = Mp4Spec {
            apple_date: Some("1904-01-01T00:00:00Z"),
            mvhd_created: MVHD_2024_06_15_1030_UTC,
            ..base()
        };
        let meta = read(&mp4_bytes(&spec), &plus_two());
        assert_eq!(meta.taken_at, Some(naive_to_unix(2024, 6, 15, 12, 30, 45)));
    }

    #[test]
    fn reads_apples_make_and_model() {
        let spec = Mp4Spec { make: Some("Apple"), model: Some("iPhone 15 Pro"), ..base() };
        let meta = read(&mp4_bytes(&spec), &plus_two());
        assert_eq!(meta.make.as_deref(), Some("Apple"));
        assert_eq!(meta.model.as_deref(), Some("iPhone 15 Pro"));
    }

    #[test]
    fn finds_moov_after_a_large_mdat() {
        let spec = Mp4Spec { mdat_before: 3 << 20, ..base() };
        assert_eq!(read(&mp4_bytes(&spec), &plus_two()).duration_ms, Some(83_000));
    }

    #[test]
    fn garbage_and_truncated_files_read_as_nothing() {
        let whole = mp4_bytes(&base());
        for bytes in [&[][..], b"not a video at all", &whole[..whole.len() / 2]] {
            assert_eq!(read(bytes, &plus_two()), VideoMeta::default());
        }
        // A box claiming more than its parent holds ends the walk rather than reading past it.
        let mut lying = whole.clone();
        let moov = lying.windows(4).position(|w| w == b"moov").unwrap() - 4;
        lying[moov..moov + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(read(&lying, &plus_two()), VideoMeta::default());
    }

    /// An EBML element with an 8-byte size, which every reader must accept.
    fn el(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.push(0x01);
        out.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
        out.extend_from_slice(body);
        out
    }

    fn webm(duration: f64) -> Vec<u8> {
        let info = el(&[0x15, 0x49, 0xA9, 0x66], &[
            el(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40]), // 1 ms per tick
            el(&[0x44, 0x89], &duration.to_be_bytes()),
        ].concat());
        let video = el(&[0xE0], &[el(&[0xB0], &1280u16.to_be_bytes()), el(&[0xBA], &720u16.to_be_bytes())].concat());
        let tracks = el(&[0x16, 0x54, 0xAE, 0x6B], &el(&[0xAE], &[el(&[0x83], &[1]), video].concat()));
        let mut segment = vec![0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // unknown size
        segment.extend(info);
        segment.extend(tracks);
        segment.extend(el(&[0x1F, 0x43, 0xB6, 0x75], &[0u8; 32])); // a Cluster: the walk stops here
        [el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"webm")), segment].concat()
    }

    #[test]
    fn reads_a_webms_size_and_running_time_but_no_date() {
        let meta = read(&webm(83_000.0), &plus_two());
        assert_eq!((meta.width, meta.height), (1280, 720));
        assert_eq!(meta.duration_ms, Some(83_000));
        assert_eq!(meta.taken_at, None);
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p photon-core --lib video`
Expected: every test panics at `todo!()`.

- [ ] **Step 4: Implement `video.rs`**

`Cargo.toml` (photon-core `[dependencies]`): `jiff = "0.2.35"` (default features: the system zone database on Unix, a bundled one on Windows - both pure Rust).

```rust
//! What photon reads from a video file's container: its displayed size, running time,
//! capture date, and an iPhone's make and model. Never a frame - the webview draws the
//! poster frame and plays the video (spec `2026-09-26-photon-video-design.md`).
//!
//! Hand-rolled, like the XMP and INI readers: MP4 and QuickTime share ISO-BMFF's boxes, and
//! photon needs five of them. The walk follows fixed paths (`moov/mvhd`, `moov/trak/tkhd`,
//! `moov/trak/mdia/hdlr`, `moov/meta/keys|ilst`), so it never recurses deeper than those,
//! and a box that claims more than its parent holds ends the walk with whatever was read.

use crate::metadata::{naive_to_unix, plausible_taken_at};
use jiff::{Timestamp, tz::TimeZone};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoMeta {
    /// As displayed: a quarter-turn matrix swaps the stored size, so nothing downstream ever
    /// rotates a frame the webview has already rotated.
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<i64>,
    /// Naive local wall-clock seconds, the way `items.taken_at` holds a photo's EXIF date.
    pub taken_at: Option<i64>,
    pub make: Option<String>,
    pub model: Option<String>,
}

/// `moov` is read whole, so it is capped: a real one is kilobytes to a few megabytes.
const MAX_MOOV: u64 = 64 << 20;
/// A WebM's Info and Tracks come before its first Cluster, well inside this.
const WEBM_HEAD: u64 = 1 << 20;
const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];
/// Seconds from 1904-01-01 (`mvhd`'s epoch) to 1970-01-01.
const MAC_EPOCH: i64 = 2_082_844_800;

pub fn read_meta(path: &Path) -> VideoMeta {
    read_meta_with(path, &TimeZone::system(), crate::now_ms() / 1000)
}

pub(crate) fn read_meta_with(path: &Path, tz: &TimeZone, now: i64) -> VideoMeta {
    let Ok(mut file) = File::open(path) else {
        return VideoMeta::default();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return VideoMeta::default();
    }
    if magic == EBML_MAGIC {
        let mut head = Vec::new();
        let _ = file.rewind();
        let _ = (&mut file).take(WEBM_HEAD).read_to_end(&mut head);
        return read_webm(&head).unwrap_or_default();
    }
    let Some(moov) = find_moov(&mut file, len) else {
        return VideoMeta::default();
    };
    read_moov(&moov, tz, now)
}

/// Walks the top-level boxes by seeking, not reading: a camera that does not "fast start"
/// writes `moov` after gigabytes of `mdat`.
fn find_moov<R: Read + Seek>(r: &mut R, len: u64) -> Option<Vec<u8>> {
    let mut pos = 0u64;
    while pos + 8 <= len {
        r.seek(SeekFrom::Start(pos)).ok()?;
        let mut h = [0u8; 8];
        r.read_exact(&mut h).ok()?;
        let (mut size, mut header) = (u64::from(u32::from_be_bytes(h[..4].try_into().ok()?)), 8);
        if size == 1 {
            let mut large = [0u8; 8];
            r.read_exact(&mut large).ok()?;
            (size, header) = (u64::from_be_bytes(large), 16);
        } else if size == 0 {
            size = len - pos;
        }
        if size < header || pos.checked_add(size)? > len {
            return None;
        }
        if &h[4..8] == b"moov" {
            if size > MAX_MOOV {
                return None;
            }
            let mut body = vec![0u8; (size - header) as usize];
            r.read_exact(&mut body).ok()?;
            return Some(body);
        }
        pos += size;
    }
    None
}

/// The child boxes of one box's body. Stops, rather than guessing, at the first box whose
/// size does not fit.
struct Boxes<'a>(&'a [u8]);

impl<'a> Iterator for Boxes<'a> {
    type Item = ([u8; 4], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let d = self.0;
        let size32 = be32(d, 0)?;
        let kind: [u8; 4] = d.get(4..8)?.try_into().ok()?;
        let (size, header) = match size32 {
            0 => (d.len() as u64, 8),
            1 => (be64(d, 8)?, 16),
            n => (u64::from(n), 8),
        };
        if size < header as u64 || size > d.len() as u64 {
            self.0 = &[];
            return None;
        }
        let (this, rest) = d.split_at(size as usize);
        self.0 = rest;
        Some((kind, &this[header..]))
    }
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    Boxes(data).find(|(k, _)| k == kind).map(|(_, body)| body)
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

fn read_moov(moov: &[u8], tz: &TimeZone, now: i64) -> VideoMeta {
    let (created, duration_ms) = child(moov, b"mvhd").and_then(read_mvhd).unwrap_or((None, None));
    let (width, height) = Boxes(moov)
        .filter(|(k, _)| k == b"trak")
        .find(|(_, trak)| {
            child(trak, b"mdia").and_then(|m| child(m, b"hdlr")).and_then(|h| h.get(8..12)) == Some(b"vide")
        })
        .and_then(|(_, trak)| child(trak, b"tkhd"))
        .and_then(read_tkhd)
        .unwrap_or((0, 0));
    let apple = apple_keys(moov);
    let get = |key: &str| apple.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    VideoMeta {
        width,
        height,
        duration_ms,
        taken_at: capture_date(get("com.apple.quicktime.creationdate").as_deref(), created, tz, now),
        make: get("com.apple.quicktime.make"),
        model: get("com.apple.quicktime.model"),
    }
}

/// (creation time in 1904-epoch seconds, running time in ms). Zero, or an all-ones
/// duration, means the writer did not say.
fn read_mvhd(b: &[u8]) -> Option<(Option<u64>, Option<i64>)> {
    let (created, timescale, duration, unknown) = match *b.first()? {
        1 => (be64(b, 4)?, be32(b, 20)?, be64(b, 24)?, u64::MAX),
        _ => (u64::from(be32(b, 4)?), be32(b, 12)?, u64::from(be32(b, 16)?), u64::from(u32::MAX)),
    };
    let duration_ms = (timescale > 0 && duration != unknown)
        .then(|| i64::try_from(u128::from(duration) * 1000 / u128::from(timescale)).ok())
        .flatten();
    Some(((created != 0).then_some(created), duration_ms))
}

/// The displayed size: the stored one, swapped when the matrix turns a quarter.
fn read_tkhd(b: &[u8]) -> Option<(u32, u32)> {
    let (matrix, size) = if *b.first()? == 1 { (52, 88) } else { (40, 76) };
    let m = |i: usize| be32(b, matrix + 4 * i).map(|v| v as i32);
    let (a, bb, c, d) = (m(0)?, m(1)?, m(3)?, m(4)?);
    let (w, h) = (be32(b, size)? >> 16, be32(b, size + 4)? >> 16);
    let quarter = a == 0 && d == 0 && bb != 0 && c != 0;
    Some(if quarter { (h, w) } else { (w, h) })
}

/// QuickTime's `moov/meta` key list, UTF-8 values only.
fn apple_keys(moov: &[u8]) -> Vec<(String, String)> {
    let Some(meta) = child(moov, b"meta") else {
        return Vec::new();
    };
    // QuickTime writes `meta` as a plain box, ISO-BMFF as a full box with four bytes of
    // version and flags first; the first child's type tells the two apart.
    let meta = if meta.get(4..8) == Some(b"hdlr") { meta } else { meta.get(4..).unwrap_or_default() };
    let mut names = Vec::new();
    if let Some(keys) = child(meta, b"keys") {
        let mut at = 8;
        while let Some(size) = be32(keys, at).map(|s| s as usize) {
            let Some(name) = keys.get(at + 8..at + size.max(8)) else { break };
            names.push(String::from_utf8_lossy(name).into_owned());
            at += size.max(8);
        }
    }
    let Some(ilst) = child(meta, b"ilst") else {
        return Vec::new();
    };
    Boxes(ilst)
        .filter_map(|(index, item)| {
            let name = names.get((u32::from_be_bytes(index) as usize).checked_sub(1)?)?;
            let data = child(item, b"data")?;
            (be32(data, 0)? == 1).then(|| (name.clone(), String::from_utf8_lossy(data.get(8..)?).into_owned()))
        })
        .collect()
}

/// Apple's date, then `mvhd`'s, each only if believable (`plausible_taken_at`).
fn capture_date(apple: Option<&str>, mvhd: Option<u64>, tz: &TimeZone, now: i64) -> Option<i64> {
    let apple = apple.and_then(parse_apple_date);
    let mvhd = mvhd.and_then(|t| wall_clock(i64::try_from(t).ok()? - MAC_EPOCH, tz));
    [apple, mvhd].into_iter().flatten().find(|&t| plausible_taken_at(t, now))
}

/// `2024-06-15T12:30:45+0200`: the wall clock as written. Its offset is dropped on purpose -
/// `taken_at` is the camera's local time, and this *is* the camera's local time.
fn parse_apple_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<u32>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    ((1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && se < 61)
        .then(|| naive_to_unix(i64::from(y), mo, d, h, mi, se))
}

/// `mvhd` is UTC; photos are the camera's wall clock. The zone in force *at that instant*
/// (DST included) is the best the file allows - the camera's own zone is not recorded, so
/// a video shot abroad is placed by the home zone.
fn wall_clock(unix: i64, tz: &TimeZone) -> Option<i64> {
    let at = Timestamp::from_second(unix).ok()?;
    Some(unix + i64::from(tz.to_offset(at).seconds()))
}

fn read_webm(buf: &[u8]) -> Option<VideoMeta> {
    let (_, segment) = ebml_children(buf).find(|(id, _)| *id == 0x1853_8067)?;
    let mut meta = VideoMeta::default();
    let (mut scale, mut ticks) = (1_000_000u64, None::<f64>);
    for (id, body) in ebml_children(segment) {
        match id {
            0x1549_A966 => {
                for (id, v) in ebml_children(body) {
                    match id {
                        0x2A_D7B1 => scale = ebml_uint(v).unwrap_or(scale),
                        0x4489 => ticks = ebml_float(v),
                        _ => {}
                    }
                }
            }
            0x1654_AE6B => {
                for (_, entry) in ebml_children(body).filter(|(id, _)| *id == 0xAE) {
                    let is_video = ebml_children(entry).any(|(id, v)| id == 0x83 && ebml_uint(v) == Some(1));
                    let Some((_, video)) = ebml_children(entry).find(|(id, _)| *id == 0xE0) else { continue };
                    if !is_video {
                        continue;
                    }
                    for (id, v) in ebml_children(video) {
                        match id {
                            0xB0 => meta.width = ebml_uint(v).and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                            0xBA => meta.height = ebml_uint(v).and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                            _ => {}
                        }
                    }
                    break;
                }
            }
            0x1F43_B675 => break, // the first Cluster: Info and Tracks are behind us
            _ => {}
        }
    }
    meta.duration_ms = ticks.filter(|t| t.is_finite() && *t >= 0.0).map(|t| (t * scale as f64 / 1e6) as i64);
    Some(meta)
}

/// An EBML variable-length integer: (value, length). An ID keeps its marker bit; a size
/// drops it, and a size of all ones means "unknown", returned as `None`.
fn vint(d: &[u8], keep_marker: bool) -> Option<(Option<u64>, usize)> {
    let first = *d.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || d.len() < len {
        return None;
    }
    let mask = if len == 8 { 0 } else { 0xFFu64 >> len };
    let mut v = if keep_marker { u64::from(first) } else { u64::from(first) & mask };
    for &b in &d[1..len] {
        v = (v << 8) | u64::from(b);
    }
    let unknown = !keep_marker && v == (1u64 << (7 * len)) - 1;
    Some(((!unknown).then_some(v), len))
}

/// Child elements. A master element whose size runs past the buffer (a Segment, read only
/// as far as `WEBM_HEAD`) is cut at the buffer's end; an unknown size runs to it.
fn ebml_children(mut d: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    std::iter::from_fn(move || {
        let (id, id_len) = vint(d, true)?;
        let (size, size_len) = vint(d.get(id_len..)?, false)?;
        let start = id_len + size_len;
        let end = size.map_or(d.len(), |s| start.saturating_add(s as usize).min(d.len()));
        let body = d.get(start..end)?;
        d = &d[end..];
        Some((id?, body))
    })
}

fn ebml_uint(d: &[u8]) -> Option<u64> {
    (d.len() <= 8).then(|| d.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b)))
}

fn ebml_float(d: &[u8]) -> Option<f64> {
    match d.len() {
        4 => Some(f64::from(f32::from_be_bytes(d.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(d.try_into().ok()?)),
        _ => None,
    }
}
```

`lib.rs`: `pub mod video;` in alphabetical place.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p photon-core --lib video`
Expected: all PASS.

- [ ] **Step 6: Probe** (each exactly reverted and restored, then `touch`):
1. In `read_tkhd`, return `(w, h)` unconditionally → the quarter-turn test fails.
2. In `capture_date`, swap the order to `[mvhd, apple]` → `apples_date_is_taken_as_wall_clock_and_wins_over_mvhd` fails (the fixture's mvhd is an hour off for exactly this).
3. In `wall_clock`, return `Some(unix)` → `an_mvhd_time_is_converted…` fails.
4. In `find_moov`, replace the seek with reading only the first box → `finds_moov_after_a_large_mdat` fails.
5. In `read_moov`, take the first `trak` instead of the `vide` one → `the_video_track_is_found_behind_a_sound_track` fails (the sound track's size is 0x0).

- [ ] **Step 7: Gate and commit**

```bash
git add crates/photon-core Cargo.lock
git commit -m "feat(core): read a video's size, running time and capture date in pure Rust"
```

---

### Task 3: Videos in the thumbnail service - their own queue, claimed by the UI

**Files:**
- Modify: `crates/photon-core/src/thumbs/queue.rs` (`pop_until`)
- Modify: `crates/photon-core/src/thumbs/cache.rs` (`render_frame`)
- Modify: `crates/photon-core/src/thumbs/service.rs`
- Modify: `crates/photon-core/src/thumbs/mod.rs` (re-export `VideoJob`, `VideoFailure`)
- Modify: `crates/photon-core/src/library/items.rs` (`pending_thumb_ids(kind)`)
- Modify: `crates/photon-core/src/library/hidden.rs:572`, `crates/photon-core/examples/index.rs` (callers)

**Interfaces:**
- Consumes: `MediaKind::Video`, `InFlight`/`Marker`/`DEATHS_TO_FAIL` from `thumbs/inflight.rs`.
- Produces (on `ThumbService`):
  ```rust
  pub struct VideoJob { pub id: i64, pub key: u64 }
  #[derive(Deserialize)] #[serde(rename_all = "lowercase")]
  pub enum VideoFailure { Unsupported, Decode, Timeout }
  pub fn video_session_start(&self, supported: bool) -> Result<()>;
  pub fn next_video_job(&self, wait: Duration) -> Result<Option<VideoJob>>;
  pub fn put_video_frame(&self, id: i64, key: u64, jpeg: &[u8]) -> Result<bool>;
  pub fn video_frame_failed(&self, id: i64, key: u64, reason: VideoFailure) -> Result<()>;
  ```
  `ThumbQueue::pop_until(&self, deadline: Instant) -> Option<i64>`. `Library::pending_thumb_ids(&self, kind: MediaKind) -> Result<Vec<i64>>`.

- [ ] **Step 1: Write the failing tests** (in `service.rs`'s test module; `video_setup` beside `setup`)

```rust
fn video_setup(names: &[&str]) -> (TempDir, Arc<Library>, ThumbService, Vec<i64>) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Arc::new(Library::open(&dir.path().join("library.db")).unwrap());
    let photos = dir.path().join("photos");
    let (_, folder) = seed_folder(&lib, &photos);
    let items: Vec<NewItem> = names
        .iter()
        .map(|name| {
            let path = write_file(&photos, name, b"not decoded by photon");
            let mut item = new_item(folder, path.to_str().unwrap(), 0);
            item.kind = MediaKind::Video;
            item
        })
        .collect();
    let ids = lib.insert_items(&items).unwrap();
    let cache = Arc::new(ThumbCache::new(dir.path().join("cache")));
    let service = ThumbService::start(lib.clone(), cache, 1);
    (dir, lib, service, ids)
}

const SHORT: Duration = Duration::from_millis(50);

#[test]
fn workers_never_take_a_video() {
    let (_dir, lib, service, ids) = video_setup(&["a.mp4"]);
    service.enqueue_pending().unwrap();
    service.prioritize(&ids, Priority::Visible); // what the viewer's neighbours do
    service.queue.wait_idle();
    assert_eq!(lib.item(ids[0]).unwrap().unwrap().thumb_state, ThumbState::Pending);
}

#[test]
fn a_session_hands_out_pending_videos_and_a_frame_makes_them_ready() {
    let (_dir, lib, service, ids) = video_setup(&["a.mp4"]);
    service.video_session_start(true).unwrap();
    let job = service.next_video_job(SHORT).unwrap().expect("a job");
    assert_eq!(job.id, ids[0]);
    assert!(service.put_video_frame(job.id, job.key, &jpeg_bytes(320, 180)).unwrap());
    let item = lib.item(ids[0]).unwrap().unwrap();
    assert_eq!(item.thumb_state, ThumbState::Ready);
    assert!(service.cache.is_complete(item.thumb_key()));
    assert!(service.next_video_job(SHORT).unwrap().is_none());
}

#[test]
fn a_waiting_thumbnail_request_resolves_when_the_frame_lands() {
    let (_dir, _lib, service, ids) = video_setup(&["a.mp4"]);
    service.video_session_start(true).unwrap();
    let service = Arc::new(service);
    let waiter = {
        let service = service.clone();
        let id = ids[0];
        std::thread::spawn(move || service.request(id, ThumbSize::Grid, Duration::from_secs(5)))
    };
    let job = service.next_video_job(Duration::from_secs(5)).unwrap().unwrap();
    service.put_video_frame(job.id, job.key, &jpeg_bytes(320, 180)).unwrap();
    assert!(waiter.join().unwrap().unwrap().is_file());
}

#[test]
fn a_video_request_without_a_session_answers_at_once() {
    let (_dir, _lib, service, ids) = video_setup(&["a.mp4"]);
    service.video_session_start(false).unwrap();
    let started = Instant::now();
    assert!(matches!(
        service.request(ids[0], ThumbSize::Grid, Duration::from_secs(30)),
        Err(Error::ThumbUnavailable(_))
    ));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_frame_for_a_changed_video_is_refused_and_releases_waiters() {
    let (_dir, lib, service, ids) = video_setup(&["a.mp4"]);
    service.video_session_start(true).unwrap();
    let job = service.next_video_job(SHORT).unwrap().unwrap();
    // The file is rewritten while the webview is still drawing it.
    let mut changed = new_item(lib.item(ids[0]).unwrap().unwrap().folder_id, &lib.item(ids[0]).unwrap().unwrap().path, 0);
    changed.kind = MediaKind::Video;
    changed.size = 999;
    lib.update_items(&[(ids[0], changed)]).unwrap();
    assert!(!service.put_video_frame(job.id, job.key, &jpeg_bytes(320, 180)).unwrap());
    assert_eq!(lib.item(ids[0]).unwrap().unwrap().thumb_state, ThumbState::Pending);
    // Released: a waiter would see the id neither queued nor in flight.
    assert!(service.videos.wait_for(ids[0], Instant::now() + SHORT));
}

#[test]
fn a_frame_that_is_not_a_jpeg_is_refused() {
    let (_dir, _lib, service, _ids) = video_setup(&["a.mp4"]);
    service.video_session_start(true).unwrap();
    let job = service.next_video_job(SHORT).unwrap().unwrap();
    assert!(service.put_video_frame(job.id, job.key, &png_bytes(8, 8)).is_err());
}

#[test]
fn unsupported_is_skipped_for_the_session_but_decode_fails_the_row() {
    let (_dir, lib, service, ids) = video_setup(&["a.mp4", "b.mp4"]);
    service.video_session_start(true).unwrap();
    let first = service.next_video_job(SHORT).unwrap().unwrap();
    service.video_frame_failed(first.id, first.key, VideoFailure::Unsupported).unwrap();
    let second = service.next_video_job(SHORT).unwrap().unwrap();
    service.video_frame_failed(second.id, second.key, VideoFailure::Decode).unwrap();

    let state = |id| lib.item(id).unwrap().unwrap().thumb_state;
    assert_eq!(state(first.id), ThumbState::Pending, "codecs installed later must still fill it");
    assert_eq!(state(second.id), ThumbState::Failed);
    service.enqueue_pending().unwrap();
    assert!(service.next_video_job(SHORT).unwrap().is_none(), "skipped for the rest of the session");
    service.video_session_start(true).unwrap();
    assert_eq!(service.next_video_job(SHORT).unwrap().unwrap().id, first.id, "a new session tries again");
    let _ = ids;
}

#[test]
fn a_page_that_dies_twice_on_a_video_fails_it() {
    let (_dir, lib, service, ids) = video_setup(&["a.mp4"]);
    for _ in 0..DEATHS_TO_FAIL {
        service.video_session_start(true).unwrap();
        // The page claims the job and is never heard from again.
        assert_eq!(service.next_video_job(SHORT).unwrap().unwrap().id, ids[0]);
    }
    service.video_session_start(true).unwrap();
    assert!(service.next_video_job(SHORT).unwrap().is_none());
    let item = lib.item(ids[0]).unwrap().unwrap();
    assert_eq!(item.thumb_state, ThumbState::Failed);
    assert_eq!(item.thumb_error.as_deref(), Some(VIDEO_CRASH_MESSAGE));
}

#[test]
fn a_clean_close_does_not_forgive_a_claimed_video() {
    // A web process that dies leaves photon running; the user quits it cleanly. That quit
    // must not sweep the video's marker, or the next launch replays the crash.
    let (dir, lib, service, ids) = video_setup(&["a.mp4"]);
    service.video_session_start(true).unwrap();
    service.next_video_job(SHORT).unwrap().unwrap();
    service.close();
    drop(service);
    let service = ThumbService::start(lib.clone(), Arc::new(ThumbCache::new(dir.path().join("cache"))), 1);
    service.video_session_start(true).unwrap();
    assert_eq!(service.video_inflight.deaths(ids[0], lib.item(ids[0]).unwrap().unwrap().thumb_key()), 1);
}
```

Also `use crate::testutil::png_bytes;`, `crate::media::MediaKind`, and `super::VideoFailure` in the test imports.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p photon-core --lib thumbs::service`
Expected: compile errors for the missing API; after stubbing with `todo!()`, the new tests panic.

- [ ] **Step 3: Implement**

`queue.rs`, beside `pop_blocking`:

```rust
    /// `pop_blocking`, but giving up at `deadline`: for a consumer that is a person's
    /// webview asking over IPC, not a worker thread that lives as long as the queue.
    pub fn pop_until(&self, deadline: Instant) -> Option<i64> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            state.admit_due(Instant::now());
            if let Some(id) = state.pop() {
                state.in_flight.insert(id);
                return Some(id);
            }
            let wake = state.next_delayed().map_or(deadline, |d| d.min(deadline));
            if self.changed.wait_until(&mut state, wake).timed_out() && Instant::now() >= deadline {
                return None;
            }
        }
    }
```

`cache.rs`, beside `render`:

```rust
    /// A poster frame the webview drew, as the preview and grid images. It arrives upright
    /// and at most preview-sized, so this only shrinks.
    pub(crate) fn render_frame(&self, frame: &DynamicImage) -> (DynamicImage, DynamicImage) {
        let preview = shrink(frame, ThumbSize::Preview.max_edge());
        let grid = shrink(&preview, ThumbSize::Grid.max_edge());
        (preview, grid)
    }
```

`items.rs`: `pending_thumb_ids(&self, kind: MediaKind)` - add `AND i.kind = ?1` to the `WHERE` and bind `kind.to_db()` (`stmt.query_map(params![kind.to_db()], …)`). Update its callers: `hidden.rs:572` and `examples/index.rs` pass `MediaKind::Image`.

`service.rs`:

```rust
/// What the webview is asked to draw: the video, and the key its frame will be stored under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoJob {
    pub id: i64,
    pub key: u64,
}

/// Why the webview could not draw a frame. `Unsupported` is the platform's answer (no codec)
/// and is not held against the file; the other two are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoFailure {
    Unsupported,
    Decode,
    Timeout,
}

pub(crate) const VIDEO_CRASH_MESSAGE: &str = "photon's window stopped while opening this video";
/// A frame bigger than this on either side is not one the webview drew at preview size.
const MAX_FRAME_EDGE: u32 = 8192;
```

New `ThumbService` fields (initialised in `start_with`):

```rust
    /// Videos waiting for a poster frame. No worker pops it: the webview draws a video's
    /// frame (`next_video_job`), and `request` waits on it exactly as it waits on `queue`.
    videos: ThumbQueue,
    /// The crash-loop guard for frames, under `<cache>/video`. Separate from `inflight`
    /// because what dies is the web process, not photon: its window goes blank, the user
    /// quits cleanly, and `disarm` - right for a worker - would sweep the evidence. This one
    /// is never disarmed, and is judged by `recover` whenever a page starts a session.
    video_inflight: InFlight,
    video_claims: Mutex<HashMap<i64, Marker>>,
    /// Ids the webview said it cannot play, skipped until the next session.
    video_skipped: Mutex<HashSet<i64>>,
    /// Whether a page is making frames. Without one, a thumbnail request for a video
    /// answers at once rather than holding a protocol thread for `THUMB_TIMEOUT`.
    video_session: AtomicBool,
```

(`Mutex` is `parking_lot::Mutex`; `InFlight::new(&cache.root().join("video"))`; no `recover()` in `start_with` - the first session does it.)

```rust
    pub fn enqueue_pending(&self) -> Result<usize> {
        let ids = self.lib.pending_thumb_ids(MediaKind::Image)?;
        self.queue.push_many(&ids, Priority::Background);
        let videos = if self.video_session.load(Ordering::SeqCst) {
            self.enqueue_pending_videos()?
        } else {
            0
        };
        Ok(ids.len() + videos)
    }

    fn enqueue_pending_videos(&self) -> Result<usize> {
        let ids = self.lib.pending_thumb_ids(MediaKind::Video)?;
        self.videos.push_many(&ids, Priority::Background);
        Ok(ids.len())
    }

    /// A page has loaded and says whether it can play video. Every claim still open belongs
    /// to a page that no longer exists - it reloaded, or its process died with the frame half
    /// drawn - so `recover` turns their markers into deaths first, the judgement a worker's
    /// marker gets at launch.
    pub fn video_session_start(&self, supported: bool) -> Result<()> {
        self.video_inflight.recover();
        // Dropping a marker after `recover` removes nothing more: its file is already a
        // record, and an unresolved marker never clears one.
        for (id, _marker) in self.video_claims.lock().drain() {
            self.videos.done(id);
        }
        self.video_skipped.lock().clear();
        self.video_session.store(supported, Ordering::SeqCst);
        if supported {
            self.enqueue_pending_videos()?;
        }
        Ok(())
    }

    /// The next video to draw, waiting up to `wait` for one: a long-poll, so the page never
    /// has to be told a job arrived.
    pub fn next_video_job(&self, wait: Duration) -> Result<Option<VideoJob>> {
        let deadline = Instant::now() + wait;
        while let Some(id) = self.videos.pop_until(deadline) {
            match self.claim_video(id) {
                Ok(Some(job)) => return Ok(Some(job)),
                Ok(None) => self.videos.done(id),
                Err(err) => {
                    self.videos.done(id);
                    return Err(err);
                }
            }
        }
        Ok(None)
    }

    fn claim_video(&self, id: i64) -> Result<Option<VideoJob>> {
        let Some(item) = self.lib.item(id)? else { return Ok(None) };
        if item.kind != MediaKind::Video
            || item.missing_since.is_some()
            || item.thumb_state == ThumbState::Failed
            || self.video_skipped.lock().contains(&id)
        {
            return Ok(None);
        }
        let key = item.thumb_key();
        if self.cache.is_complete(key) {
            if item.thumb_state != ThumbState::Ready {
                self.lib.set_thumb_state_if_unchanged(&item, ThumbState::Ready, None)?;
            }
            self.video_inflight.clear(id);
            return Ok(None);
        }
        let deaths = self.video_inflight.deaths(id, key);
        if deaths >= DEATHS_TO_FAIL {
            tracing::error!(id, path = %item.path, deaths, "the window died with this video open; not opening it again");
            self.lib.set_thumb_state_if_unchanged(&item, ThumbState::Failed, Some(VIDEO_CRASH_MESSAGE))?;
            self.video_inflight.clear(id);
            return Ok(None);
        }
        self.video_claims.lock().insert(id, self.video_inflight.begin(id, key));
        Ok(Some(VideoJob { id, key }))
    }

    /// Stores the frame the webview drew for `id`. `false`, storing nothing, when the video
    /// changed since the job was handed out - the frame is of a file that is not there.
    /// The bytes came over IPC, so they are decoded as a JPEG of bounded size and nothing else.
    pub fn put_video_frame(&self, id: i64, key: u64, jpeg: &[u8]) -> Result<bool> {
        let marker = self.video_claims.lock().remove(&id);
        let stored = self.store_video_frame(id, key, jpeg);
        // The page lived to answer: whatever it sent, this was not a death.
        if let Some(marker) = marker {
            marker.resolve(true);
        }
        self.videos.done(id);
        stored
    }

    fn store_video_frame(&self, id: i64, key: u64, jpeg: &[u8]) -> Result<bool> {
        let Some(item) = self.lib.item(id)? else { return Ok(false) };
        if item.kind != MediaKind::Video || item.thumb_key() != key {
            return Ok(false);
        }
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(jpeg), image::ImageFormat::Jpeg);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_FRAME_EDGE);
        limits.max_image_height = Some(MAX_FRAME_EDGE);
        reader.limits(limits);
        let frame = reader.decode()?;
        let (preview, grid) = self.cache.render_frame(&frame);
        self.cache.store(key, &preview, &grid)?;
        self.lib.set_thumb_state_if_unchanged(&item, ThumbState::Ready, None)
    }

    pub fn video_frame_failed(&self, id: i64, key: u64, reason: VideoFailure) -> Result<()> {
        if let Some(marker) = self.video_claims.lock().remove(&id) {
            marker.resolve(true);
        }
        let result = match reason {
            VideoFailure::Unsupported => {
                self.video_skipped.lock().insert(id);
                Ok(())
            }
            VideoFailure::Decode | VideoFailure::Timeout => self.fail_video(id, key, reason),
        };
        self.videos.done(id);
        result
    }

    fn fail_video(&self, id: i64, key: u64, reason: VideoFailure) -> Result<()> {
        let Some(item) = self.lib.item(id)? else { return Ok(()) };
        if item.thumb_key() != key {
            return Ok(());
        }
        let message = if reason == VideoFailure::Timeout {
            "This video took too long to open."
        } else {
            "This video can't be read."
        };
        self.lib.set_thumb_state_if_unchanged(&item, ThumbState::Failed, Some(message))?;
        Ok(())
    }
```

`request`: route by kind.

```rust
    pub fn request(&self, id: i64, size: ThumbSize, timeout: Duration) -> Result<PathBuf> {
        let deadline = Instant::now() + timeout;
        let video = self.lib.item(id)?.is_some_and(|item| item.kind == MediaKind::Video);
        let queue = if video { &self.videos } else { &self.queue };
        for _ in 0..2 {
            if let Some(path) = self.cached(id, size)? {
                return Ok(path);
            }
            if video && (!self.video_session.load(Ordering::SeqCst) || self.video_skipped.lock().contains(&id)) {
                // No page is drawing frames - the webview cannot play video here - or it
                // could not draw this one: waiting would hold a protocol thread for nothing.
                return Err(Error::ThumbUnavailable(id));
            }
            queue.push(id, Priority::Visible);
            if !queue.wait_for(id, deadline) {
                return Err(Error::ThumbTimeout(id));
            }
        }
        self.cached(id, size)?.ok_or(Error::ThumbUnavailable(id))
    }
```

`process`, right after the missing/failed early return:

```rust
    if item.kind != MediaKind::Image {
        // The webview draws a video's frame (`next_video_job`). `set_visible` and
        // `prioritize` take ids without asking what they are, so one can still land here.
        suspects.resolved(id, queue);
        return Ok(());
    }
```

`close`: also `self.videos.close();` (a blocked `next_video_job` returns). Do **not** disarm `video_inflight`; say why in `close`'s doc. GC (line ~363): `+ self.video_inflight.collect_garbage(&live)`.

`thumbs/mod.rs`: `pub use service::{ThumbService, VideoFailure, VideoJob, default_workers};`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p photon-core --lib thumbs`
Expected: PASS, old and new.

- [ ] **Step 5: Probe** (exact revert, restore, `touch`):
1. Remove the `item.kind != MediaKind::Image` guard in `process` → `workers_never_take_a_video` fails (the worker tries to decode the text file and marks it Failed).
2. Remove the no-session early return in `request` → `a_video_request_without_a_session_answers_at_once` fails on the elapsed-time assertion.
3. Remove `self.videos.done(id)` from `put_video_frame` → `a_frame_for_a_changed_video…` fails on `wait_for`, and `a_waiting_thumbnail_request…` still passes (it is released by `cached`); note that in the commit message as the reason the changed-video test checks `wait_for` directly.
4. Remove `self.video_inflight.recover()` from `video_session_start` → `a_page_that_dies_twice…` fails.
5. Make `close` call `self.video_inflight.disarm()` → `a_clean_close_does_not_forgive…` fails.
6. Treat `Unsupported` like `Decode` → `unsupported_is_skipped…` fails.

- [ ] **Step 6: Gate and commit**

```bash
git add -A crates/photon-core
git commit -m "feat(thumbs): a video queue the webview drains, with its own crash-loop guard"
```

---

### Task 4: Index videos - scanner, search terms, look-alikes

**Files:**
- Modify: `crates/photon-core/src/media.rs` (`from_path`)
- Modify: `crates/photon-core/src/scanner.rs` (`describe`)
- Modify: `crates/photon-core/src/search.rs` (`Term::Kind`, `Fields.kind`)
- Modify: `crates/photon-core/src/library/items.rs` (`search_entries` passes the kind)
- Modify: `crates/photon-core/src/library/similar.rs:39-45,128-132` (`kind = 0`)

**Interfaces:**
- Consumes: `video::read_meta`, `VideoMeta`, `testutil::{Mp4Spec, mp4_bytes}`.
- Produces: `.mp4/.m4v/.mov/.webm` indexed as `MediaKind::Video`; search `video` / `photo`; `search::Fields.kind: Option<MediaKind>`.

- [ ] **Step 1: Write the failing tests**

`scanner.rs` tests:

```rust
#[test]
fn indexes_a_video_with_its_size_running_time_and_date() {
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(
        &root,
        "IMG_0001.MOV",
        &mp4_bytes(&Mp4Spec {
            width: 1920,
            height: 1080,
            rotation: 90,
            timescale: 600,
            duration: 600 * 12,
            apple_date: Some("2024-06-15T12:30:45+0200"),
            make: Some("Apple"),
            model: Some("iPhone 15 Pro"),
            ..Mp4Spec::default()
        }),
    );
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);

    let id = lib.known_items(watched.id).unwrap().values().next().unwrap().id;
    let item = lib.item(id).unwrap().unwrap();
    assert_eq!(item.kind, MediaKind::Video);
    assert_eq!((item.width, item.height, item.orientation), (1080, 1920, 1));
    assert_eq!(item.duration_ms, Some(12_000));
    assert_eq!(item.taken_at, naive_to_unix(2024, 6, 15, 12, 30, 45));
    assert_eq!(item.camera.model.as_deref(), Some("iPhone 15 Pro"));
}

#[test]
fn a_garbage_mp4_is_still_indexed() {
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "broken.mp4", b"");
    write_file(&root, "junk.webm", b"\x1a\x45\xdf\xa3 and then nothing");
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);
    let known = lib.known_items(watched.id).unwrap();
    assert_eq!(known.len(), 2);
    for k in known.values() {
        let item = lib.item(k.id).unwrap().unwrap();
        assert_eq!(item.kind, MediaKind::Video);
        assert_eq!((item.width, item.height, item.duration_ms), (0, 0, None));
        assert_eq!(item.taken_at, item.mtime_ms.div_euclid(1000));
    }
}
```

`media.rs` tests:

```rust
#[test]
fn the_four_video_extensions_are_videos_in_any_case() {
    for name in ["a.mp4", "b.M4V", "c.MOV", "d.webm"] {
        assert_eq!(MediaKind::from_path(Path::new(name)), Some(MediaKind::Video), "{name}");
    }
    for name in ["e.avi", "f.mkv", "g.3gp"] {
        assert_eq!(MediaKind::from_path(Path::new(name)), None, "{name}");
    }
}
```

`search.rs` tests:

```rust
#[test]
fn video_and_photo_filter_on_kind_and_quotes_make_them_words() {
    let clip = Fields { any: &["clip.mp4", "Trips"], kind: Some(MediaKind::Video), ..Fields::default() };
    let shot = Fields { any: &["video night.jpg"], kind: Some(MediaKind::Image), ..Fields::default() };
    assert!(Query::parse("video").matches(&clip));
    assert!(!Query::parse("video").matches(&shot), "a file named 'video' is not a video");
    assert!(Query::parse("\"video\"").matches(&shot), "quoted, it is the word");
    assert!(Query::parse("photo").matches(&shot));
    assert!(!Query::parse("photo").matches(&clip));
    assert!(Query::parse("video trips").matches(&clip));
}
```

`similar.rs` tests (beside `a_ready_photo_without_a_hash_is_a_candidate_and_takes_one`, using its `item_at` helper):

```rust
#[test]
fn a_video_is_never_a_look_alike_candidate() {
    let (_dir, lib) = temp_library();
    let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
    let video = NewItem { kind: MediaKind::Video, ..item_at(folder, "/pics/clip.mp4", 20, 100) };
    let ids = lib.insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100), video]).unwrap();
    lib.writer().execute("UPDATE items SET thumb_state = 1", []).unwrap();
    let candidates: Vec<i64> = lib.similar_candidates().unwrap().iter().map(|c| c.id).collect();
    assert_eq!(candidates, [ids[0]], "a poster frame is not the video");
    // A hash already stored against a video (a library from a build before this rule) is
    // never compared either.
    lib.writer().execute("UPDATE items SET percep_hash = 1 WHERE id = ?1", [ids[1]]).unwrap();
    assert!(lib.percep_hashes().unwrap().iter().all(|h| h.id != ids[1]));
}
```

(Import `MediaKind` in that test module if it is not already.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p photon-core indexes_a_video a_garbage_mp4 the_four_video video_and_photo a_video_is_never`
Expected: FAIL (videos not indexed; `Fields` has no `kind`).

- [ ] **Step 3: Implement**

`media.rs` `from_path`: add `"mp4" | "m4v" | "mov" | "webm" => Some(Self::Video),` with the comment: `// Only what all three webviews can plausibly play: a tile that will not play is worse than no tile (AVI, MKV and 3GP are out).`

`scanner.rs`:

```rust
fn describe(entry: &DirEntry, path: &str, folder_id: i64, kind: MediaKind, size: i64, mtime_ms: i64) -> NewItem {
    let file_name = entry.file_name().to_string_lossy().into_owned();
    let dated = |taken_at: Option<i64>| taken_at.unwrap_or(mtime_ms.div_euclid(1000));
    match kind {
        MediaKind::Image => {
            let meta = read_image_meta(entry.path());
            let embedded = read_embedded(entry.path());
            NewItem {
                folder_id,
                path: path.to_string(),
                file_name,
                kind,
                size,
                mtime_ms,
                width: meta.width,
                height: meta.height,
                orientation: meta.orientation,
                taken_at: dated(meta.taken_at),
                // Always `None` here; `apply_picasa` sets the real value after the walk.
                rating: meta.rating,
                camera: meta.camera,
                tags: embedded.keywords,
                caption: embedded.caption,
                duration_ms: None,
            }
        }
        MediaKind::Video => {
            // Rotation is resolved into the size (`VideoMeta`), so orientation is always 1.
            let meta = crate::video::read_meta(entry.path());
            NewItem {
                folder_id,
                path: path.to_string(),
                file_name,
                kind,
                size,
                mtime_ms,
                width: meta.width,
                height: meta.height,
                orientation: 1,
                taken_at: dated(meta.taken_at),
                rating: None,
                camera: CameraMeta { make: meta.make, model: meta.model, ..CameraMeta::default() },
                tags: Vec::new(),
                caption: None,
                duration_ms: meta.duration_ms,
            }
        }
    }
}
```

`search.rs`: add `Term::Kind(MediaKind)`; in `parse`, before the prefix checks:

```rust
            // `video` and `photo` filter on what the file is. Unquoted only: `"video"` is
            // still the word, for a folder called Videos.
            if token.unquoted_prefix.is_none() && (text == "video" || text == "photo") {
                let kind = if text == "video" { MediaKind::Video } else { MediaKind::Image };
                let current = alternatives.last_mut().expect("starts with one alternative");
                if !current.contains(&Term::Kind(kind)) {
                    current.push(Term::Kind(kind));
                }
                continue;
            }
```

`Fields` gains `pub kind: Option<MediaKind>` (doc: "What the file is, for `video` and `photo`."); `matches` gains `Term::Kind(kind) => fields.kind == Some(*kind),`. Extend the grammar doc comment with one bullet for the two words. In `items.rs::search_entries`, pass `kind: MediaKind::from_db(r.get(6)?),`.

`similar.rs`: add `AND i.kind = 0` to `CANDIDATES_SQL` and to the `percep_hashes` query, with the comment `-- A poster frame is not the video; it would pair with the still taken beside it.`

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p photon-core`
Expected: PASS.

- [ ] **Step 5: Probe** - revert the `Video` arm of `describe` to the image path: `indexes_a_video…` fails on size and duration. Drop `kind = 0` from `CANDIDATES_SQL` only: the look-alike test fails. Treat `"video"` as `Term::Any` when unquoted: the search test fails on `shot`.

- [ ] **Step 6: Gate and commit**

```bash
git add -A crates/photon-core
git commit -m "feat(scan): index MP4, MOV and WebM videos, searchable as video or photo"
```

---

### Task 5: The loopback media server

**Files:**
- Create: `crates/photon-app/src/media_server.rs`
- Modify: `crates/photon-app/src/lib.rs` (`mod media_server;` - match how the other modules are declared)
- Modify: `crates/photon-app/Cargo.toml` (`tiny_http = "0.12"`, `getrandom = "0.3"`)
- Modify: `crates/photon-app/src/app.rs:150` (start it in `setup`, `app.manage`)

**Interfaces:**
- Consumes: `Engine.lib`, `MediaKind::Video`.
- Produces:
  ```rust
  pub struct MediaServer { /* port, token, server */ }
  impl MediaServer {
      pub fn start(engine: Arc<Engine>) -> std::io::Result<MediaServer>;
      pub fn base_url(&self) -> String;              // "http://127.0.0.1:<port>/<token>"
  }
  pub(crate) enum RangeAnswer { Whole, Partial(u64, u64), Unsatisfiable }
  pub(crate) fn parse_range(header: Option<&str>, len: u64) -> RangeAnswer;
  ```

- [ ] **Step 1: Write the failing tests** (bottom of `media_server.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, jpeg};
    use std::io::{Read, Write};
    use std::net::TcpStream;

    #[test]
    fn ranges_as_http_reads_them() {
        use RangeAnswer::*;
        assert_eq!(parse_range(None, 100), Whole);
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Partial(10, 19));
        assert_eq!(parse_range(Some("bytes=10-"), 100), Partial(10, 99), "open-ended runs to the end, uncapped");
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Partial(90, 99));
        assert_eq!(parse_range(Some("bytes=-30"), 100), Partial(70, 99));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Partial(0, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=20-10"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-1,5-9"), 100), Whole, "multi-range: the whole file is allowed");
        assert_eq!(parse_range(Some("pages=1-2"), 100), Whole, "an unknown unit is ignored");
        assert_eq!(parse_range(Some("bytes=a-b"), 100), Whole, "a malformed range is ignored");
    }

    #[test]
    fn a_range_on_an_empty_file_is_unsatisfiable() {
        assert_eq!(parse_range(Some("bytes=0-"), 0), RangeAnswer::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-1"), 0), RangeAnswer::Unsatisfiable);
        assert_eq!(parse_range(None, 0), RangeAnswer::Whole);
    }

    /// A raw HTTP/1.1 exchange, so the test sees exactly what a player would.
    fn get(port: u16, path: &str, extra: &str) -> (u16, String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n").unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status = head[9..12].parse().unwrap();
        (status, head, raw[split + 4..].to_vec())
    }

    fn server_with(bytes: &[u8]) -> (crate::testutil::Fixture, MediaServer, i64) {
        let f = fixture(&[("clip.mp4", bytes), ("a.jpg", &jpeg(8, 8))]);
        f.add_photos();
        let id = f.ids().into_iter().find(|&id| f.engine.lib.item(id).unwrap().unwrap().kind == MediaKind::Video).unwrap();
        let server = MediaServer::start(f.engine.clone()).unwrap();
        (f, server, id)
    }

    #[test]
    fn serves_a_video_whole_and_in_ranges() {
        let bytes: Vec<u8> = (0..3_000_000u32).map(|i| i as u8).collect();
        let (_f, server, id) = server_with(&bytes);
        let path = format!("/{}/video/{id}", server.token);
        let (status, head, body) = get(server.port, &path, "");
        assert_eq!(status, 200);
        assert_eq!(body, bytes);
        assert!(head.contains("Accept-Ranges: bytes"));
        assert!(head.contains(&format!("Access-Control-Allow-Origin: {}", app_origin())));
        // Open-ended: answered to the end of a file larger than any chunk size. This is the
        // spike's bug - a 1 MB cap broke every far seek.
        let (status, head, body) = get(server.port, &path, "Range: bytes=1000-\r\n");
        assert_eq!(status, 206);
        assert!(head.contains(&format!("Content-Range: bytes 1000-{}/{}", bytes.len() - 1, bytes.len())));
        assert_eq!(body, bytes[1000..]);
        let (status, _, _) = get(server.port, &path, "Range: bytes=9999999-\r\n");
        assert_eq!(status, 416);
    }

    #[test]
    fn refuses_what_is_not_this_apps_video() {
        let (f, server, id) = server_with(b"video bytes");
        let photo = f.ids().into_iter().find(|&i| i != id).unwrap();
        for path in [
            format!("/{}/video/{id}", "0".repeat(32)),     // wrong token
            format!("/{}/video/{photo}", server.token),     // a photo
            format!("/{}/video/999999", server.token),      // no such row
            format!("/{}/../../etc/passwd", server.token),  // not the route
        ] {
            assert_eq!(get(server.port, &path, "").0, 404, "{path}");
        }
        // Right token, wrong Host: what a DNS-rebound web page sends.
        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        write!(s, "GET /{}/video/{id} HTTP/1.1\r\nHost: evil.example:{}\r\nConnection: close\r\n\r\n", server.token, server.port).unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 404"), "{raw}");
    }

    #[test]
    fn a_client_hanging_up_mid_body_leaves_the_server_serving() {
        let bytes = vec![7u8; 8_000_000];
        let (_f, server, id) = server_with(&bytes);
        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        write!(s, "GET /{}/video/{id} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", server.token, server.port).unwrap();
        let mut first = [0u8; 1024];
        s.read_exact(&mut first).unwrap();
        drop(s); // the player seeked elsewhere
        let (status, _, body) = get(server.port, &format!("/{}/video/{id}", server.token), "Range: bytes=0-9\r\n");
        assert_eq!((status, body.len()), (206, 10));
    }
}
```

(`jpeg` is `crate::testutil::jpeg(w, h)`. The fixture scans `clip.mp4` as a video since Task 4.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p photon-app media_server`
Expected: compile errors, then `todo!()` panics once stubbed.

- [ ] **Step 3: Implement**

`Cargo.toml` (photon-app): `tiny_http = "0.12"`, `getrandom = "0.3"`.

```rust
//! Serves videos to the webview over loopback HTTP.
//!
//! Not `photon://`: WebKitGTK hands a media URL to GStreamer, whose WebKit source takes only
//! http(s) and blob URLs, so `<video src="photon://…">` fails before a pipeline exists
//! (spike, spec `2026-09-26-photon-video-design.md`). Plain HTTP with ranges is what every
//! webview plays best, so all three platforms use this one path.
//!
//! A loopback port is reachable by anything on the machine, and by a page in the user's
//! browser through DNS rebinding. Hence the per-launch token, compared in constant time; the
//! `Host` check, which is what defeats rebinding (a rebound page sends its own host name);
//! the exact origin in `Access-Control-Allow-Origin`; and one route that serves video rows
//! by id and never turns any part of a URL into a path.

use crate::engine::Engine;
use photon_core::media::MediaKind;
use std::{fs::File, io::{Read, Seek, SeekFrom}, path::Path, sync::Arc};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const WORKERS: usize = 4;

pub struct MediaServer {
    pub(crate) port: u16,
    pub(crate) token: String,
    _server: Arc<Server>,
}

/// The page's own origin, which is the only one allowed to read a response - and the one
/// that keeps a canvas the frame was drawn on readable.
pub(crate) fn app_origin() -> &'static str {
    if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }
}

impl MediaServer {
    pub fn start(engine: Arc<Engine>) -> std::io::Result<Self> {
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(std::io::Error::other)?);
        let port = server.server_addr().to_ip().map(|a| a.port()).ok_or_else(|| std::io::Error::other("not an IP listener"))?;
        let mut raw = [0u8; 16];
        getrandom::fill(&mut raw).map_err(std::io::Error::other)?;
        let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        for i in 0..WORKERS {
            let (server, engine, token) = (server.clone(), engine.clone(), token.clone());
            std::thread::Builder::new()
                .name(format!("photon-media-{i}"))
                .spawn(move || {
                    for request in server.incoming_requests() {
                        serve(&engine, &token, port, request);
                    }
                })?;
        }
        Ok(Self { port, token, _server: server })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/{}", self.port, self.token)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RangeAnswer {
    Whole,
    /// Inclusive, as HTTP writes it.
    Partial(u64, u64),
    Unsatisfiable,
}

/// One `bytes=` range. A multi-range, an unknown unit or a malformed value is answered with
/// the whole file, which HTTP permits and every player accepts. An open-ended range runs to
/// the end of the file: capped, it broke every far seek in WebKitGTK (the spike's
/// `downloadbuffer` offset mismatch).
pub(crate) fn parse_range(header: Option<&str>, len: u64) -> RangeAnswer {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return RangeAnswer::Whole;
    };
    if spec.contains(',') {
        return RangeAnswer::Whole;
    }
    let Some((a, b)) = spec.split_once('-') else { return RangeAnswer::Whole };
    let (a, b) = (a.trim(), b.trim());
    let parse = |s: &str| s.parse::<u64>().ok();
    match (a.is_empty(), b.is_empty()) {
        (true, false) => match parse(b) {
            Some(0) => RangeAnswer::Unsatisfiable,
            Some(_) if len == 0 => RangeAnswer::Unsatisfiable,
            Some(n) => RangeAnswer::Partial(len.saturating_sub(n), len - 1),
            None => RangeAnswer::Whole,
        },
        (false, _) => match (parse(a), if b.is_empty() { Some(u64::MAX) } else { parse(b) }) {
            (Some(start), Some(end)) if start > end => RangeAnswer::Unsatisfiable,
            (Some(start), Some(_)) if start >= len => RangeAnswer::Unsatisfiable,
            (Some(start), Some(end)) => RangeAnswer::Partial(start, end.min(len - 1)),
            _ => RangeAnswer::Whole,
        },
        (true, true) => RangeAnswer::Whole,
    }
}

fn eq_constant_time(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("ASCII header")
}

fn not_found(request: Request) {
    let _ = request.respond(Response::empty(StatusCode(404)));
}

fn serve(engine: &Engine, token: &str, port: u16, request: Request) {
    let host_ok = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Host"))
        .is_some_and(|h| h.value.as_str() == format!("127.0.0.1:{port}"));
    let head = *request.method() == Method::Head;
    if !host_ok || !(head || *request.method() == Method::Get) {
        return not_found(request);
    }
    let parts: Vec<&str> = request.url().trim_start_matches('/').split('/').collect();
    let id = match parts.as_slice() {
        [t, "video", id] if eq_constant_time(t, token) => id.parse::<i64>().ok(),
        _ => None,
    };
    let Some(item) = id.and_then(|id| engine.lib.item(id).ok().flatten()) else {
        return not_found(request);
    };
    if item.kind != MediaKind::Video || item.missing_since.is_some() {
        return not_found(request);
    }
    // Read-only, like every open of a watched file.
    let Ok(mut file) = File::open(&item.path) else { return not_found(request) };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let range = request.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.as_str().to_owned());
    let mut headers = vec![
        header("Content-Type", mime_for(Path::new(&item.path))),
        header("Accept-Ranges", "bytes"),
        header("Access-Control-Allow-Origin", app_origin()),
        header("Cache-Control", "no-store"),
    ];
    let (status, start, count) = match parse_range(range.as_deref(), len) {
        RangeAnswer::Whole => (200, 0, len),
        RangeAnswer::Partial(start, end) => {
            headers.push(header("Content-Range", &format!("bytes {start}-{end}/{len}")));
            (206, start, end - start + 1)
        }
        RangeAnswer::Unsatisfiable => {
            headers.push(header("Content-Range", &format!("bytes */{len}")));
            let _ = request.respond(Response::new(StatusCode(416), headers, std::io::empty(), Some(0), None));
            return;
        }
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return not_found(request);
    }
    let body: Box<dyn Read + Send> = if head { Box::new(std::io::empty()) } else { Box::new(file.take(count)) };
    // Streamed from the file, never built in memory. An error here is the player hanging up
    // after a seek - the normal end of most requests, not something to log.
    let _ = request.respond(Response::new(StatusCode(status), headers, body, Some(count as usize), None));
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("mp4" | "m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}
```

`app.rs`, after `engine.startup(pictures)`:

```rust
                    // Videos play over loopback HTTP (`media_server.rs`). A failure to bind
                    // costs video playback and poster frames, not the app: `media_base`
                    // then errors and the UI treats video as unsupported.
                    match crate::media_server::MediaServer::start(engine.clone()) {
                        Ok(server) => {
                            app.manage(server);
                        }
                        Err(err) => tracing::error!(%err, "could not start the media server"),
                    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p photon-app media_server`
Expected: PASS.

- [ ] **Step 5: Probe** - cap `Partial` at `start + 1_000_000` (Tauri's asset protocol does this): `serves_a_video_whole_and_in_ranges` fails on the open-ended body. Remove the `host_ok` check: the rebinding assertion fails. Replace `len - 1` with an unguarded path for `bytes=-1` on a zero-length file: `a_range_on_an_empty_file…` panics.

- [ ] **Step 6: Gate and commit**

```bash
git add -A crates/photon-app Cargo.lock
git commit -m "feat(app): a token-guarded loopback server that streams videos with ranges"
```

---

### Task 6: IPC for videos, and what refuses a video

**Files:**
- Modify: `crates/photon-app/src/commands.rs` (new commands; `ViewerItem`; `copy_picture`; `neighbours`)
- Modify: `crates/photon-app/src/ipc.rs`, `crates/photon-app/src/app.rs` (`generate_handler!`)
- Modify: `crates/photon-app/src/engine.rs:797` (`write_edit`)
- Modify: `crates/photon-app/src/protocol.rs:93` (`image`)
- Modify: `crates/photon-core/src/error.rs` (`NotAPhoto`), `crates/photon-app/src/error.rs` (`"notAPhoto"`)
- Modify: `crates/photon-app/tauri.conf.json` (CSP)
- Modify: `crates/xtask/screenshots/mock.js`

**Interfaces:**
- Consumes: `ThumbService::{video_session_start, next_video_job, put_video_frame, video_frame_failed}`, `MediaServer::base_url`.
- Produces IPC: `media_base() -> string`, `video_session_start({supported})`, `next_video_job() -> {id, key} | null` (`key` hex, as `thumbKey`), `put_video_frame` (raw body, headers `x-photon-id`, `x-photon-key`), `video_frame_failed({id, key, reason})`. `ViewerItem.kind: 'image' | 'video'`, `ViewerItem.durationMs: number | null`.

- [ ] **Step 1: Write the failing tests** (`commands.rs` tests, using `fixture`)

```rust
#[test]
fn a_video_refuses_edits_and_the_clipboard() {
    let f = fixture(&[("clip.mp4", b"video")]);
    f.add_photos();
    let id = f.ids()[0];
    let refused = |r: CmdResult<()>| matches!(r, Err(AppError { kind: "notAPhoto", .. }));
    assert!(refused(rotate_item(&f.engine, id, true)));
    assert!(refused(set_item_edit(&f.engine, id, 1, None)));
    assert!(matches!(copy_picture(&f.engine, id), Err(AppError { kind: "notAPhoto", .. })));
}

#[test]
fn the_viewer_is_told_it_is_a_video_and_neighbours_leave_it_out() {
    let f = fixture(&[("a.jpg", &jpeg(8, 8)), ("b.mp4", b"video"), ("c.jpg", &jpeg(8, 8))]);
    f.add_photos();
    let video = f.ids().into_iter().find(|&id| f.engine.lib.item(id).unwrap().unwrap().kind == MediaKind::Video).unwrap();
    let item = viewer_item(&f.engine, video).unwrap();
    assert_eq!(item.kind, MediaKind::Video);
    assert!(!neighbours(&f.engine, f.ids()[0], 2).contains(&video));
}

#[test]
fn next_video_job_speaks_the_ui_key() {
    let f = fixture(&[("clip.mp4", b"video")]);
    f.add_photos();
    video_session_start(&f.engine, true).unwrap();
    let job = next_video_job(&f.engine, Duration::from_millis(200)).unwrap().unwrap();
    assert_eq!(job.key, viewer_item(&f.engine, job.id).unwrap().thumb_key);
}
```

`protocol.rs` tests:

```rust
#[test]
fn the_full_image_route_refuses_a_video() {
    let f = fixture(&[("clip.mp4", &vec![0u8; 4096])]);
    f.add_photos();
    let id = f.ids()[0];
    assert_eq!(handle(&f.engine, &format!("image/{id}")).status(), StatusCode::NOT_FOUND);
}
```

(`jpeg` is `crate::testutil::jpeg(w, h)`.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p photon-app a_video_refuses the_viewer_is_told next_video_job_speaks the_full_image_route`
Expected: FAIL / compile errors.

- [ ] **Step 3: Implement**

`photon-core/src/error.rs`:

```rust
    #[error("This is a video; that only works on photos.")]
    NotAPhoto(i64),
```

`photon-app/src/error.rs`: `NotAPhoto(_) => "notAPhoto",`.

`engine.rs` `write_edit`: replace `self.live_item(id)?;` with

```rust
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        // photon never decodes a video, so it cannot render one turned or cropped.
        if item.kind != MediaKind::Image {
            return Err(Error::NotAPhoto(id));
        }
```

(`rotate_item` reaches this too.) If `live_item` has no other callers, remove it; otherwise keep it.

`commands.rs`:
- `ViewerItem` gains `pub kind: MediaKind,` and `pub duration_ms: Option<i64>,` (docs: "A video plays; the viewer shows no zoom, crop or turn for it."), filled from `item.kind` / `item.duration_ms`.
- `copy_picture`: after the lookup, `if item.kind != MediaKind::Image { return Err(Error::NotAPhoto(id).into()); }`.
- `neighbours`: the filter becomes `is_some_and(|item| item.kind == MediaKind::Image && item.edit.is_identity())`, and its doc gains "and videos, which the viewer plays rather than preloads".
- New commands:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoJobDto {
    pub id: i64,
    /// Hex, as `thumbKey` is everywhere else on the wire.
    pub key: String,
}

/// How long `next_video_job` holds the call open waiting for a job. Under the webview's
/// own IPC timeouts, and long enough that an idle page asks about twice a minute.
pub const VIDEO_JOB_WAIT: Duration = Duration::from_secs(25);

pub fn media_base(server: &crate::media_server::MediaServer) -> String {
    server.base_url()
}

pub fn video_session_start(engine: &Engine, supported: bool) -> CmdResult<()> {
    Ok(engine.thumbs.video_session_start(supported)?)
}

pub fn next_video_job(engine: &Engine, wait: Duration) -> CmdResult<Option<VideoJobDto>> {
    Ok(engine.thumbs.next_video_job(wait)?.map(|job| VideoJobDto { id: job.id, key: hex_key(job.key) }))
}

pub fn put_video_frame(engine: &Engine, id: i64, key: &str, jpeg: &[u8]) -> CmdResult<()> {
    let key = crate::protocol::parse_key(key).ok_or(Error::NotFound(id))?;
    if engine.thumbs.put_video_frame(id, key, jpeg)? {
        engine.refresh_grid()?;
    }
    Ok(())
}

pub fn video_frame_failed(engine: &Engine, id: i64, key: &str, reason: VideoFailure) -> CmdResult<()> {
    let key = crate::protocol::parse_key(key).ok_or(Error::NotFound(id))?;
    Ok(engine.thumbs.video_frame_failed(id, key, reason)?)
}
```

(`protocol::parse_key` becomes `pub(crate)`; do not write a second parser - its doc says why the exact spelling matters. `refresh_grid` is what makes a tile's `thumbState` and the viewer's `pictureChanged` see `Ready`; check `grep -n "pub fn refresh_grid" crates/photon-app/src/engine.rs` for its exact signature.)

`ipc.rs`:

```rust
#[tauri::command(async)]
pub fn media_base(server: State<'_, crate::media_server::MediaServer>) -> Result<String, AppError> {
    Ok(commands::media_base(&server))
}

#[tauri::command(async)]
pub fn video_session_start(engine: Eng<'_>, supported: bool) -> Result<(), AppError> {
    commands::video_session_start(&engine, supported)
}

/// Holds the call open up to `VIDEO_JOB_WAIT`, so it runs on the blocking pool like
/// `add_folder`: parked on a worker thread it would stall every other command's dispatch.
#[tauri::command(async)]
pub async fn next_video_job(engine: Eng<'_>) -> Result<Option<commands::VideoJobDto>, AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || commands::next_video_job(&engine, commands::VIDEO_JOB_WAIT))
        .await
        .map_err(AppError::internal)?
}

/// The frame arrives as the raw request body - a JPEG of a few hundred KB, which as a JSON
/// number array would be several MB - with its id and key in headers.
#[tauri::command(async)]
pub async fn put_video_frame(engine: Eng<'_>, request: tauri::ipc::Request<'_>) -> Result<(), AppError> {
    let tauri::ipc::InvokeBody::Raw(jpeg) = request.body().clone() else {
        return Err(AppError::internal("expected the frame as raw bytes"));
    };
    let get = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let id: i64 = get("x-photon-id").and_then(|v| v.parse().ok()).ok_or_else(|| AppError::internal("missing x-photon-id"))?;
    let key = get("x-photon-key").ok_or_else(|| AppError::internal("missing x-photon-key"))?;
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || commands::put_video_frame(&engine, id, &key, &jpeg))
        .await
        .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub fn video_frame_failed(engine: Eng<'_>, id: i64, key: String, reason: photon_core::thumbs::VideoFailure) -> Result<(), AppError> {
    commands::video_frame_failed(&engine, id, &key, reason)
}
```

(Check `AppError::internal`'s signature: `grep -n "fn internal" crates/photon-app/src/error.rs`. If it takes an `impl Display`, the string literals above work as written.)

`app.rs` `generate_handler!`: add `ipc::media_base, ipc::video_session_start, ipc::next_video_job, ipc::put_video_frame, ipc::video_frame_failed`.

`protocol.rs` `image`: after the missing check,

```rust
    // A video is served by `media_server.rs`, streamed; this handler reads the whole file
    // into memory, which for a video is gigabytes.
    if item.kind != MediaKind::Image {
        return text(StatusCode::NOT_FOUND, "not found");
    }
```

`tauri.conf.json` CSP: append `; media-src 'self' http://127.0.0.1:*` and add `http://127.0.0.1:*` to `connect-src`? **No** - `<video>` loads are `media-src`; the canvas draw needs no fetch. Only `media-src`.

`mock.js`: in `canned`, `media_base: () => 'http://127.0.0.1:9/0000'`, `next_video_job: () => null`; in `SILENT`, add `'put_video_frame', 'video_frame_failed', 'video_session_start'`. In `viewerItem`, add `kind: 'image', durationMs: null`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --workspace`
Expected: PASS, including `screenshots.rs`'s every-command-answered test.

- [ ] **Step 5: Probe** - drop the kind check in `write_edit`: `a_video_refuses…` fails. Drop the neighbours kind filter: the second test fails. Drop the `image` guard: the protocol test fails (200 with the bytes).

- [ ] **Step 6: Gate and commit**

```bash
git add -A crates
git commit -m "feat(app): IPC for poster frames and the media base; videos refuse edits and copying"
```

---

### Task 7: The UI half of poster frames

**Files:**
- Modify: `ui/src/lib/api.ts` (types, commands)
- Create: `ui/src/lib/video.ts`, `ui/src/lib/video.test.ts`
- Create: `ui/src/lib/video-state.svelte.ts`
- Create: `ui/src/lib/video-grab.ts`
- Create: `ui/src/lib/video-thumbnailer.svelte.ts`, `ui/src/lib/video-thumbnailer.svelte.test.ts`
- Modify: `ui/src/App.svelte` (`onMount`)
- Modify: `ui/src/lib/library.test.ts` (every `GridEntry` literal: `kind: 'image', durationMs: null`)

**Interfaces:**
- Consumes: the five commands of Task 6.
- Produces:
  ```ts
  // video.ts
  export function posterTime(duration: number): number;
  export function formatDuration(ms: number): string;
  export function mediaSupported(canPlayType: (type: string) => string): boolean;
  export function videoUrl(base: string, id: number): string;
  export class MediaUnsupported extends Error {}
  export class MediaDecodeError extends Error {}
  // video-state.svelte.ts
  export const videoState: { base: string | null; supported: boolean };
  // video-thumbnailer.svelte.ts
  export const JOB_TIMEOUT_MS = 15_000;
  export function createVideoThumbnailer(deps: ThumbnailerDeps): { start(): void; stop(): void };
  ```

- [ ] **Step 1: Write the failing tests**

`video.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { formatDuration, mediaSupported, posterTime, videoUrl } from './video';

describe('posterTime', () => {
  it('is a second in, or a tenth of a short clip', () => {
    expect(posterTime(83)).toBe(1);
    expect(posterTime(4)).toBeCloseTo(0.4);
  });
  it('is the start when the duration is unknown', () => {
    for (const d of [NaN, Infinity, 0, -1]) expect(posterTime(d)).toBe(0);
  });
});

describe('formatDuration', () => {
  it('reads as a player shows it', () => {
    expect(formatDuration(0)).toBe('0:00');
    expect(formatDuration(9_400)).toBe('0:09');
    expect(formatDuration(83_000)).toBe('1:23');
    expect(formatDuration(3_723_000)).toBe('1:02:03');
  });
});

describe('mediaSupported', () => {
  it('asks for MP4, whose demuxer ships with the audio sink whose absence crashes WebKit', () => {
    const asked: string[] = [];
    expect(mediaSupported((t) => (asked.push(t), 'maybe'))).toBe(true);
    expect(asked).toEqual(['video/mp4']);
    expect(mediaSupported(() => '')).toBe(false);
  });
});

it('videoUrl joins the base and the id', () => {
  expect(videoUrl('http://127.0.0.1:5/abc', 12)).toBe('http://127.0.0.1:5/abc/video/12');
});
```

`video-thumbnailer.svelte.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createVideoThumbnailer, JOB_TIMEOUT_MS, IDLE_MS } from './video-thumbnailer.svelte';
import { MediaDecodeError, MediaUnsupported } from './video';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());
const settle = () => vi.advanceTimersByTimeAsync(0);

function setup(jobs: ({ id: number; key: string } | null)[], grab: (url: string, signal: AbortSignal) => Promise<Uint8Array>) {
  const queue = [...jobs];
  const deps = {
    nextJob: vi.fn(async () => (queue.length ? queue.shift()! : new Promise<never>(() => {}))),
    put: vi.fn(async () => {}),
    fail: vi.fn(async () => {}),
    url: (id: number) => `u/${id}`,
    grab: vi.fn(grab),
  };
  return { t: createVideoThumbnailer(deps), deps };
}

describe('createVideoThumbnailer', () => {
  it('draws each job and hands the frame back under its key', async () => {
    const { t, deps } = setup([{ id: 1, key: 'k1' }, { id: 2, key: 'k2' }], async () => new Uint8Array([1]));
    t.start();
    await settle();
    expect(deps.put.mock.calls).toEqual([[1, 'k1', new Uint8Array([1])], [2, 'k2', new Uint8Array([1])]]);
    expect(deps.grab.mock.calls.map((c) => c[0])).toEqual(['u/1', 'u/2']);
  });

  it('reports unsupported, decode and timeout as three different reasons', async () => {
    const outcomes = [new MediaUnsupported(), new MediaDecodeError('x'), null];
    const { t, deps } = setup(
      [{ id: 1, key: 'a' }, { id: 2, key: 'b' }, { id: 3, key: 'c' }],
      (_url, signal) => {
        const o = outcomes.shift();
        if (o) return Promise.reject(o);
        return new Promise((_, reject) => signal.addEventListener('abort', () => reject(signal.reason)));
      },
    );
    t.start();
    await settle();
    await vi.advanceTimersByTimeAsync(JOB_TIMEOUT_MS);
    expect(deps.fail.mock.calls).toEqual([[1, 'a', 'unsupported'], [2, 'b', 'decode'], [3, 'c', 'timeout']]);
  });

  it('does not ask again at once after an empty answer', async () => {
    const { t, deps } = setup([null, null], async () => new Uint8Array());
    t.start();
    await settle();
    expect(deps.nextJob).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(deps.nextJob).toHaveBeenCalledTimes(2);
  });

  it('stops taking jobs when stopped', async () => {
    const { t, deps } = setup([null, { id: 1, key: 'a' }], async () => new Uint8Array());
    t.start();
    await settle();
    t.stop();
    await vi.advanceTimersByTimeAsync(IDLE_MS * 3);
    expect(deps.grab).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `npm test -w ui -- src/lib/video.test.ts src/lib/video-thumbnailer.svelte.test.ts`
Expected: FAIL (modules missing).

- [ ] **Step 3: Implement**

`api.ts`:
- `GridEntry`: `kind: 'image' | 'video'; durationMs: number | null` (doc: "`durationMs`: a video's running time; null for a photo.").
- `ViewerItem`: `kind: 'image' | 'video'; durationMs: number | null;` with doc lines.
- `export interface VideoJob { id: number; key: string }` and `export type VideoFailure = 'unsupported' | 'decode' | 'timeout';`
- In `api`:

```ts
  mediaBase: () => invoke<string>('media_base'),
  videoSessionStart: (supported: boolean) => invoke<void>('video_session_start', { supported }),
  /** Long-polls: resolves with a job, or null after about 25 s with none. */
  nextVideoJob: () => invoke<VideoJob | null>('next_video_job'),
  /** The frame goes as the raw body, not JSON: a JSON number array of a JPEG is ~4x its size. */
  putVideoFrame: (id: number, key: string, jpeg: Uint8Array) =>
    invoke<void>('put_video_frame', jpeg, { headers: { 'x-photon-id': String(id), 'x-photon-key': key } }),
  videoFrameFailed: (id: number, key: string, reason: VideoFailure) =>
    invoke<void>('video_frame_failed', { id, key, reason }),
```

`video.ts`:

```ts
/** Where a poster frame is taken: a second in, since the first frame is often black, or a
 *  tenth of a clip shorter than ten seconds. An unknown duration (a WebM without one) takes
 *  the start. */
export function posterTime(duration: number): number {
  return Number.isFinite(duration) && duration > 0 ? Math.min(1, duration / 10) : 0;
}

export function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  const [h, m, s] = [Math.floor(total / 3600), Math.floor(total / 60) % 60, total % 60];
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** Whether this webview can play video at all. On Linux this is also the guard against a
 *  crash: GStreamer's MP4 demuxer ships in gst-plugins-good, the same package as the
 *  `autoaudiosink` whose absence aborts WebKit's web process on any `<video>` load - so
 *  "MP4 is supported" proves the crash cannot happen. Do not swap in a codec that lives in
 *  another package. `canPlayType` itself is safe without the plugins (measured). */
export function mediaSupported(canPlayType: (type: string) => string): boolean {
  return canPlayType('video/mp4') !== '';
}

export function videoUrl(base: string, id: number): string {
  return `${base}/video/${id}`;
}

/** The webview has no decoder for this file: a fact about the platform, not the file. */
export class MediaUnsupported extends Error {}
/** The webview tried and failed: a fact about the file. */
export class MediaDecodeError extends Error {}
```

`video-state.svelte.ts`:

```ts
/** Set once, in `App.svelte`'s `onMount`: where videos are served, and whether this webview
 *  can play them. Until both are known nothing creates a `<video>`. */
export const videoState = $state<{ base: string | null; supported: boolean }>({ base: null, supported: false });
```

`video-grab.ts` (DOM; no unit test - vitest runs in node):

```ts
import { MediaDecodeError, MediaUnsupported, posterTime } from './video';

/** Loads `url` into a hidden, muted video, seeks to the poster time, and returns the frame
 *  as a JPEG no larger than `maxEdge`. The element is attached while it works - as it was in
 *  the spike that measured this - and always unloaded after, which releases the pipeline. */
export async function grabPoster(url: string, signal: AbortSignal, maxEdge: number): Promise<Uint8Array> {
  const v = document.createElement('video');
  v.muted = true;
  v.preload = 'metadata';
  v.crossOrigin = 'anonymous';
  v.className = 'poster-grab';
  document.body.append(v);
  try {
    await until(v, 'loadedmetadata', signal, () => (v.src = url));
    await until(v, 'seeked', signal, () => (v.currentTime = posterTime(v.duration)));
    const scale = Math.min(1, maxEdge / Math.max(v.videoWidth, v.videoHeight, 1));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(v.videoWidth * scale));
    canvas.height = Math.max(1, Math.round(v.videoHeight * scale));
    canvas.getContext('2d')?.drawImage(v, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/jpeg', 0.9));
    if (!blob) throw new MediaDecodeError('no frame');
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    v.removeAttribute('src');
    v.load();
    v.remove();
  }
}

function until(v: HTMLVideoElement, event: string, signal: AbortSignal, begin: () => void): Promise<void> {
  return new Promise((resolve, reject) => {
    const done = () => {
      v.removeEventListener(event, ok);
      v.removeEventListener('error', bad);
      signal.removeEventListener('abort', aborted);
    };
    const ok = () => (done(), resolve());
    const bad = () => {
      done();
      reject(v.error?.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED ? new MediaUnsupported() : new MediaDecodeError(v.error?.message ?? 'error'));
    };
    const aborted = () => (done(), reject(signal.reason));
    v.addEventListener(event, ok);
    v.addEventListener('error', bad);
    signal.addEventListener('abort', aborted);
    begin();
  });
}
```

`.poster-grab` in `App.svelte`'s global styles: `position: fixed; left: 0; top: 0; width: 1px; height: 1px; opacity: 0; pointer-events: none;` (no colours, so `no-literals.test.ts` is unaffected).

`video-thumbnailer.svelte.ts`:

```ts
import type { VideoFailure, VideoJob } from './api';
import { MediaUnsupported } from './video';

/** A frame that has not come in this long is a failure, not a slow disk: the spike's 4K file
 *  gave its frame in 200-300 ms. */
export const JOB_TIMEOUT_MS = 15_000;
/** After an empty answer. The backend long-polls, so this only paces a backend that
 *  answers at once - an error, or the screenshot mock. */
export const IDLE_MS = 1_000;

export interface ThumbnailerDeps {
  nextJob: () => Promise<VideoJob | null>;
  put: (id: number, key: string, jpeg: Uint8Array) => Promise<void>;
  fail: (id: number, key: string, reason: VideoFailure) => Promise<void>;
  url: (id: number) => string;
  grab: (url: string, signal: AbortSignal) => Promise<Uint8Array>;
}

/** Draws videos' poster frames, one at a time - a WebKit media pipeline is heavy - for as
 *  long as it runs. Generation-counted, so a stop and a start in quick succession never
 *  leave two loops. */
export function createVideoThumbnailer(deps: ThumbnailerDeps) {
  let generation = 0;
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  async function loop(mine: number) {
    while (mine === generation) {
      let job: VideoJob | null;
      try {
        job = await deps.nextJob();
      } catch {
        job = null;
      }
      if (mine !== generation) return;
      if (!job) {
        await sleep(IDLE_MS);
        continue;
      }
      await draw(job);
    }
  }

  async function draw(job: VideoJob) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(new DOMException('timeout', 'TimeoutError')), JOB_TIMEOUT_MS);
    try {
      const jpeg = await deps.grab(deps.url(job.id), controller.signal);
      await deps.put(job.id, job.key, jpeg);
    } catch (e) {
      const reason: VideoFailure = controller.signal.aborted ? 'timeout' : e instanceof MediaUnsupported ? 'unsupported' : 'decode';
      await deps.fail(job.id, job.key, reason).catch(() => {});
    } finally {
      clearTimeout(timer);
    }
  }

  return {
    start() {
      void loop(++generation);
    },
    stop() {
      generation++;
    },
  };
}
```

`App.svelte` `onMount` (inside the existing one, after the library has loaded):

```ts
    // Videos: where they are served and whether this webview can play them, then the
    // session that tells the backend so - on Linux without GStreamer's plugins, playing one
    // would take the window down, and without a session no thumbnail request waits on us.
    const thumbnailer = createVideoThumbnailer({
      nextJob: api.nextVideoJob,
      put: api.putVideoFrame,
      fail: api.videoFrameFailed,
      url: (id) => videoUrl(videoState.base ?? '', id),
      grab: (url, signal) => grabPoster(url, signal, PREVIEW_MAX_EDGE),
    });
    void (async () => {
      const base = await api.mediaBase().catch(() => null);
      videoState.base = base;
      videoState.supported = base !== null && mediaSupported((t) => document.createElement('video').canPlayType(t));
      await api.videoSessionStart(videoState.supported).catch(() => {});
      if (videoState.supported) thumbnailer.start();
    })();
```

and `thumbnailer.stop()` in the `onMount` cleanup. `PREVIEW_MAX_EDGE = 1600` lives in `video.ts` with the comment "`ThumbSize::Preview.max_edge()` in `thumbs/cache.rs`; the backend shrinks anything larger, so this only saves the IPC bytes."

- [ ] **Step 4: Run to verify they pass**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all tests PASS.

- [ ] **Step 5: Probe** - make `draw` report every error as `'decode'`: the reasons test fails. Remove the `IDLE_MS` sleep: the "does not ask again at once" test fails. Remove the `mine !== generation` check after `nextJob`: the stop test fails.

- [ ] **Step 6: Commit**

```bash
git add -A ui
git commit -m "feat(ui): draw videos' poster frames in the webview and hand them to the backend

The DOM half (video-grab.ts) and App.svelte's wiring have no seam in a node-environment
vitest; svelte-check and the README's smoke checklist cover them."
```

---

### Task 8: Tiles, the viewer, Compare and the slideshow

**Files:**
- Modify: `ui/src/components/Tile.svelte`
- Modify: `ui/src/components/Viewer.svelte`
- Modify: `ui/src/components/Compare.svelte:214`
- Create: `ui/src/lib/slideshow-order.ts`, `ui/src/lib/slideshow-order.test.ts`

**Interfaces:**
- Consumes: `videoState`, `videoUrl`, `formatDuration`, `GridEntry.kind/durationMs`, `ViewerItem.kind`.
- Produces: `nextStill(from: number, len: number, kindAt: (i: number) => Promise<'image' | 'video' | undefined>): Promise<number | null>`.

- [ ] **Step 1: Write the failing test** (`slideshow-order.test.ts`)

```ts
import { describe, expect, it } from 'vitest';
import { nextStill } from './slideshow-order';

const kinds = (s: string) => async (i: number) => (s[i] === 'v' ? 'video' : 'image') as 'image' | 'video';

describe('nextStill', () => {
  it('skips videos and wraps', async () => {
    expect(await nextStill(0, 4, kinds('pvvp'))).toBe(3);
    expect(await nextStill(3, 4, kinds('pvvp'))).toBe(0);
  });
  it('comes back to itself when it is the only photo', async () => {
    expect(await nextStill(1, 3, kinds('vpv'))).toBe(1);
  });
  it('is null when there is no photo at all', async () => {
    expect(await nextStill(0, 3, kinds('vvv'))).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `npm test -w ui -- src/lib/slideshow-order.test.ts`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement**

`slideshow-order.ts`:

```ts
/** The slideshow is a photo slideshow: the next offset after `from` that is a photo,
 *  wrapping, or null when the view holds none. `from` itself comes back when it is the only
 *  photo - the caller then stays, as the one-photo view always has. */
export async function nextStill(
  from: number,
  len: number,
  kindAt: (i: number) => Promise<'image' | 'video' | undefined>,
): Promise<number | null> {
  for (let step = 1; step <= len; step++) {
    const i = (from + step) % len;
    if ((await kindAt(i)) === 'image') return i;
  }
  return null;
}
```

`Viewer.svelte`:
- Slideshow `advance`:
  ```ts
    advance: () => {
      const len = library.info.len;
      void nextStill(current, len, async (i) => {
        await library.ensure(i, i + 1);
        return library.entry(i)?.kind;
      }).then((next) => {
        // One photo has no next: `goto` would reload it, blanking the screen every interval.
        if (next !== null && next !== current) goto(next);
      });
    },
  ```
- `startSlideshow` becomes async: if `item?.kind === 'video'`, find `nextStill(current, …)`; if null, `toasts` "There are no photos here to show." (use the component's existing toast/notice mechanism - `grep -n "toast" ui/src/components/Viewer.svelte`) and return; otherwise `goto(next)` before `slideshow.start(false)`.
- `const isVideo = $derived(item?.kind === 'video');`
- The load effect, after `if (it.thumbState === 'failed')` - videos keep the Failed message only for a real failure, and skip the image preload:
  ```ts
      if (it.kind === 'video') {
        if (!videoState.supported || !videoState.base) {
          error = isWindows()
            ? "This video can't be played here."
            : "This system can't play videos. Install GStreamer's good and libav plugins (see the README).";
          return;
        }
        fullSrc = videoUrl(videoState.base, it.id);
        return; // no neighbour preload from a video, and nothing to decode
      }
  ```
  (The `thumbState === 'failed'` early return must not stop a video that still plays: move the video branch **above** it, since a Failed poster says nothing about playback.)
- Markup, in the `{:else}` branch where the preview and full `<img>` are:
  ```svelte
        {:else if isVideo}
          <!-- Plays on open, with sound, as Picasa did. Not focusable, so the viewer's own
               keys (arrows, Home, End, Escape, Space) keep working over it. -->
          <!-- svelte-ignore a11y_media_has_caption -->
          <video
            class="full"
            src={fullSrc}
            poster={mediaUrl(`thumb/${item.id}/preview/${item.thumbKey}`)}
            controls
            autoplay
            preload="metadata"
            crossorigin="anonymous"
            tabindex="-1"
            bind:this={videoEl}
          ></video>
  ```
  with `let videoEl = $state<HTMLVideoElement | null>(null);`. The existing effect's cleanup (`return () => { cancelled = true; }`) gains: `videoEl?.pause(); videoEl?.removeAttribute('src'); videoEl?.load();` so leaving a video - navigation, close, a `pictureChanged` reload - releases the pipeline.
- Keys: in the plain-letter block, before `r`/`c`: `if (isVideo && ['r', 'R', 'c', 'C'].includes(e.key)) { e.preventDefault(); return; }`; replace the Space rule with
  ```ts
      if (e.key === ' ' && (slideshow.active || isVideo)) {
        e.preventDefault();
        if (slideshow.active) slideshow.toggle();
        else if (videoEl) void (videoEl.paused ? videoEl.play() : videoEl.pause());
        return;
      }
  ```
- `onwheel`, `onzoom` and `onpointerdown`: first line `if (isVideo) return;` (no zoom or pan for a video; the pointer must reach the native controls). Hide the zoom slider and the rotate/crop buttons when `isVideo` (wrap their markup in `{#if !isVideo}`).
- Info panel: under the size row, `{#if item.kind === 'video' && item.durationMs !== null}<dt>Length</dt><dd>{formatDuration(item.durationMs)}</dd>{/if}` - match the panel's existing row markup.
- The menu's "Copy photo" item: `{#if !isVideo}` around it.

`Tile.svelte`:

```svelte
  {#if entry?.kind === 'video'}
    <span class="video-badge" aria-label="Video">
      <Icon name="play" size={12} filled />
      {#if entry.durationMs !== null}{formatDuration(entry.durationMs)}{/if}
    </span>
  {/if}
```

and the problem icon becomes `<Icon name={entry?.kind === 'video' ? 'play' : 'triangle-alert'} size={28} />` (a video with no frame yet - no plugins, or not drawn - reads as a video, not as broken). Style `.video-badge` like the star's corner mark but bottom-right, using the same tokens the star uses (`grep -n "\.star" -A8 ui/src/components/Tile.svelte`).

`Compare.svelte` `fullSrc`: `if (p.kind === 'video') return previewSrc(p);` first, with the comment "A video's full image is not served (`protocol.rs`); Compare compares stills, so its poster stands in."

- [ ] **Step 4: Run to verify**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; PASS.

- [ ] **Step 5: Probe** - in `nextStill`, return `i` for any kind: the skip test fails. The viewer, tile and Compare changes are effect and markup wiring: say so in the commit message; the smoke checklist (Task 9) covers them.

- [ ] **Step 6: Commit**

```bash
git add -A ui
git commit -m "feat(ui): play videos in the viewer, badge them in the grid, and skip them in slideshows

The viewer, tile and Compare changes are markup and effect wiring with no seam in the
node-environment vitest; svelte-check and the smoke checklist cover them."
```

---

### Task 9: Packaging, notices, README, screenshots

**Files:**
- Modify: `crates/photon-app/tauri.conf.json` (`bundle.linux.deb.depends`, `bundle.linux.appimage`)
- Modify: `THIRD-PARTY-NOTICES.md`
- Modify: `README.md` (File formats at :53; smoke checklist at :297)
- Modify: `crates/xtask/screenshots/mock.js` (`entry`), `crates/xtask/src/screenshots.rs` (`SHOTS`)

- [ ] **Step 1: Packaging**

`tauri.conf.json`:

```json
    "linux": {
      "deb": {
        "depends": ["libwebkit2gtk-4.1-0", "libsoup-3.0-0", "gstreamer1.0-plugins-good", "gstreamer1.0-libav"],
        "section": "graphics"
      },
      "appimage": {
        "bundleMediaFramework": true
      }
    },
```

Run `cargo run -p xtask -- metadata`; if it checks the depends list or notices, satisfy it.

- [ ] **Step 2: Notices**

Add sections for `jiff` (MIT OR Unlicense), `tiny_http` (MIT OR Apache-2.0), `getrandom` (MIT OR Apache-2.0) - reproduce each licence text from `~/.cargo/registry/src/*/<crate>-<version>/LICENSE*`, in the file's existing format; plus a section for the GStreamer plugins the AppImage bundles (LGPL-2.1-or-later; `gst-libav` links FFmpeg, LGPL), stating that they are bundled only in the AppImage and where their source is. Run `cargo run -p xtask -- metadata` until it passes.

- [ ] **Step 3: README**

File formats gains:

```markdown
**Video:** MP4, M4V, MOV and WebM, played by the system's own video support - photon ships
no decoder. On macOS everything an iPhone records plays. On Windows, iPhone video (HEVC)
needs Microsoft's *HEVC Video Extensions* from the Store; without it such a video shows no
preview and does not play. On Linux the `.deb` pulls in GStreamer's good and libav plugins
and the AppImage carries its own; anywhere else install them (`gst-plugins-good` and
`gst-libav` on Arch, `gstreamer1.0-plugins-good` and `gstreamer1.0-libav` on Debian and
Ubuntu) - without them photon shows videos but cannot play them. A video's preview is drawn
while photon's window is open.
```

Smoke checklist gains:

```markdown
- [ ] Put an iPhone `.MOV`, an Android `.mp4` and a `.webm` in a watched folder: each gets a tile with a play badge and its length, sorted among the photos taken beside it (not hours away). The preview frame is not black.
- [ ] Open each: it plays at once with sound; the seek bar works, including a jump to the last tenth of a long 4K video; Space pauses and resumes; ←/→ go to the neighbours and the video stops; R and C do nothing; the zoom slider is gone. The info panel shows the length.
- [ ] A slideshow started in a folder of photos and videos shows only the photos; started on a video it begins at the next photo; in a folder of only videos it does not start and says why.
- [ ] Linux, without GStreamer's good plugins (remove them, or run on a fresh Arch): photon starts, video tiles show the play icon, opening one says the plugins are missing, and the window never goes blank.
- [ ] macOS and Windows: a video plays (App Transport Security and WebView2 allow `http://127.0.0.1`), and no firewall prompt appears when photon starts. **Blocking for the release.**
- [ ] Windows without the HEVC extension: an iPhone video shows the play icon and a message, not a broken tile.
```

- [ ] **Step 4: Screenshots**

`mock.js` `entry(i)`: `kind: i % 9 === 4 ? 'video' : 'image', durationMs: i % 9 === 4 ? 83_000 : null,`. Add an action `video: () => open(4)` and in `viewerItem`, return `kind: id === 5 ? 'video' : 'image', durationMs: id === 5 ? 83_000 : null`. `SHOTS` gains:

```rust
    // A grid holding videos: the play badge and its length on tiles 4, 13, 22...
    Shot { name: "grid-videos-light", query: "theme=light", dark: false },
    // The viewer on a video. Chromium without proprietary codecs cannot play MP4, so this
    // shows the unsupported message over the poster - which is the state worth seeing.
    Shot { name: "viewer-video-dark", query: "theme=dark&do=video", dark: true },
```

Run `cargo run -p xtask -- screenshots --only grid-videos-light` and `--only viewer-video-dark`, and look at both PNGs: the badge must be legible on a light and a dark photo, the message readable.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
npm run check && npm test
cargo run -p xtask -- metadata
git add -A
git commit -m "chore(video): GStreamer for Linux packages, notices, README, and screenshots"
```

---

## After the last task

- A whole-branch review before merging (CLAUDE.md: "A large branch gets an independent read"). Point it at: the thumbnail service's claim/marker lifetimes (Task 3), `serve`'s route and host check (Task 5), the viewer's effect cleanup releasing the `<video>` on every exit path (Task 8), and anything the feature *arms* in old code - `set_visible`/`prioritize` now receive video ids; `pictureChanged` now sees video rows turn `Ready` when a frame lands (it must not reload a playing video: check `picture.ts` and add a `kind` guard with a test if it would).
- The release is a minor (schema 20), with notes saying an older photon refuses the library, and that Linux users outside the `.deb`/AppImage need the two GStreamer packages.
