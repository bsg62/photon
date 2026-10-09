# The native UI's foundation and the grid - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate, `photon-ui`, whose binary `photon-native` opens the library the Tauri photon opens and shows its grid, read-only, drawn by egui on the GPU; and a command that writes that grid to PNG without a display.

**Architecture:** *State modules* in plain Rust (geometry, motion, the scroll position, tasks, the thumbnail loader and its bookkeeping, tokens, labels) and *views* that draw them with egui and turn input into calls on them. The app calls `photon-engine` directly: events arrive over a channel, anything that reads SQLite runs on a task thread, and the published `GridIndex` is read in memory. Thumbnails are decoded on a small pool, waited for on one shared thread when they are not built yet, and uploaded as textures within a budget per frame.

**Tech Stack:** Rust 2024, `eframe`/`egui` 0.36.2 on `wgpu`, `egui_extras` (SVG), `egui_kittest` (headless frames and off-screen rendering), `fastframe-fonts` 0.4.1 (the platform's interface face and script fallbacks), `unicode-bidi`, `jiff`, `dirs`, `photon-engine`, `photon-core`.

**Specs:** `docs/superpowers/specs/2026-10-09-photon-native-ui-design.md` (the umbrella) and `docs/superpowers/specs/2026-10-09-photon-native-grid-slice-design.md` (this sub-project). Read both first; they have the reasons this plan does not repeat.

**What this plan leaves out:** the gate - the 300,000-photo fixture, the scroll programme, the Svelte baseline and `xtask grid-gate`. It is the second plan of this sub-project, written against the slice once this one has landed, because its native half hooks the frame loop built here and its Svelte half is a patch against `Grid.svelte`.

## How this plan was written

Every source file below is given whole, tests included, and was **compiled and run before the plan was written**: in a scratch crate, against egui 0.36.2, the real `photon-core` and the real engine (through `photon-app`, renamed), on 2026-10-09. 119 unit tests, the end-to-end test and both screenshots passed, `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` were clean, and each probe listed was run and failed as stated. Two things were not compiled as written, because what they depend on did not exist yet, and are marked where they occur: the two functions Task 1 adds to `photon-core`, and the six lines of `thumbs/source.rs` that call them.

So the tasks are not "write a failing test, then the code". They are: write the file, run its tests, then **prove each rule by its probe** - an exact replacement that breaks the rule, after which the named test must fail. That is the failing-first evidence Conventions asks for. A probe that passes is a finding: stop and report it.

**After every probe: put the original text back, then `touch` the file.** A file restored to bytes cargo has already built can leave the probed build in place (`probe-restore-stale-build` in memory).

## Global Constraints

- Read `CLAUDE.md` before starting. Its rules bind every task.
- **Branch `native-ui`.** Sub-project 0 (`photon-engine`) must be on `main` and merged into `native-ui` first: `git switch native-ui && git merge main`, and check `crates/photon-engine` exists.
- **Never launch the GUI.** Do not run `cargo run -p photon-ui`, `photon-native` or `npm run dev`. What is seen is seen through Task 14's PNGs.
- **The Rust gate before every commit:** `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- `eframe`, `egui_extras` and `egui_kittest` are pinned to exactly `=0.36.2`. Do not write egui code from memory of another version: 0.36 changed `eframe::App` (`fn ui(&mut self, ui: &mut egui::Ui, ...)`) and `Context::run_ui`.
- `photon-ui` depends on nothing of Tauri's. `ui/`, `photon-app` and the `mock.js` harness are not touched.
- **State modules name no egui type** (`args.rs`, `dirs.rs`, `tasks.rs`, `theme/tokens.rs`, `grid/labels.rs`, `grid/layout.rs`, `grid/motion.rs`, `grid/scroll.rs`, `grid/visible.rs`, `thumbs/loader.rs`, `thumbs/textures.rs`). Task 13's tripwire holds it.
- Nothing that reads SQLite or the filesystem runs on the UI thread (`App::ui` and everything it calls). `Engine::published`, `GridIndex::rows`/`sections` and `commands::set_visible` are in memory and may.
- photon never writes to, moves or deletes photo files. Nothing here writes outside the library's own data and cache directories.
- Every probe is run, and the commit message of its task says so. Restore exactly, and `touch`.
- **CI is the first build of this crate on macOS and Windows, and on a Linux image that is not this machine.** If a job fails to build or link `photon-ui` for a missing system library, the fix is the package the error names, added to `ci.yml`'s install line in a commit of its own, and a line in CLAUDE.md's Commands saying so. Do not add packages on a guess: here it built with none.
- Comments carry the reasoning, in the surrounding code's density and voice. No em dashes in comments or docs; the codebase uses " - ".
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Do not push; the controller does.

## Review Focus

Inputs the spec implies and a person is likely to meet; each has its test in the task named.

1. **A library with nothing in it** - a first run, and every launch until the engine's first build publishes: the grid draws nothing, pins nothing, and Home, End and the page keys do nothing rather than panic. Task 12, `an_empty_library_draws_nothing_and_still_takes_its_keys`.
2. **A window with no room** - minimised, or dragged narrower than the scrollbar: no panic, and a grid again when the room comes back. Task 12, `a_window_with_no_room_is_drawn_without_a_panic`.
3. **A photo whose thumbnail can never be made** - an unreadable file, an offline drive's photo not yet cached: the tile shows its mark, is not asked for again on every frame, and the view still counts as settled. Task 12, `a_thumbnail_that_cannot_be_made_settles_as_its_mark`; Task 8, `a_thumbnail_that_was_unavailable_is_left_alone_for_a_while`.
4. **A display scaled to 125%, 150% or 175%**: rows are drawn on whole device pixels, or every picture is resampled on every frame of a scroll. Task 12, `rows_are_drawn_on_whole_device_pixels_at_any_scale`.
5. **A folder, file or person named in Arabic or Hebrew, with a year or a Latin word in it**: egui alone draws these wrong (measured; see Task 10). Task 10, `a_line_is_set_down_in_the_order_it_is_read`.

And one the engine's contract rules out but a crash would be the price of: **an index published without its layout generation moving** under rows built for the one before. Task 12, `rows_that_outlive_their_index_are_drawn_as_far_as_it_goes`.

## File Structure

```
crates/photon-core/src/thumbs/cache.rs      ThumbCache::decode (Task 1)
crates/photon-core/src/thumbs/service.rs    ThumbService::decoded, decode_file (Task 1)
crates/photon-ui/Cargo.toml                 the crate (Task 2; the binary in Task 13)
crates/photon-ui/src/lib.rs                 the module list, and the tripwire (Task 13)
crates/photon-ui/src/main.rs                arguments, logging, the window (Task 13)
crates/photon-ui/src/args.rs                the command line (Task 2)
crates/photon-ui/src/dirs.rs                where the library and the cache are (Task 2)
crates/photon-ui/src/theme/tokens.rs        the colours and scales (Task 3)
crates/photon-ui/src/theme/apply.rs         the tokens onto egui (Task 11)
crates/photon-ui/src/theme/fonts.rs         the faces (Task 11)
crates/photon-ui/src/icons.rs               four Lucide icons (Task 11)
crates/photon-ui/src/tasks.rs               latest-wins work off the UI thread (Task 4)
crates/photon-ui/src/text.rs                one line in any mix of scripts (Task 10)
crates/photon-ui/src/events.rs              the engine's events (Task 13)
crates/photon-ui/src/app.rs                 the application, the order of a frame (Task 13)
crates/photon-ui/src/grid/motion.rs         still, scrolling or jumping (Task 5)
crates/photon-ui/src/grid/layout.rs         rows, tiles, the pin, the pinned header (Task 5)
crates/photon-ui/src/grid/scroll.rs         the position and the scrollbar (Task 6)
crates/photon-ui/src/grid/visible.rs        telling the engine what is on screen (Task 6)
crates/photon-ui/src/grid/labels.rs         what the grid writes (Task 7)
crates/photon-ui/src/grid/tile.rs           one tile (Task 12)
crates/photon-ui/src/grid/header.rs         a section's header (Task 12)
crates/photon-ui/src/grid/view.rs           lays out, moves, draws (Task 12)
crates/photon-ui/src/thumbs/textures.rs     which textures are held (Task 8)
crates/photon-ui/src/thumbs/loader.rs       reading, waiting, decoding (Task 9)
crates/photon-ui/src/thumbs/shown.rs        results uploaded as textures (Task 12)
crates/photon-ui/src/thumbs/source.rs       the engine as the source (Task 13)
crates/photon-ui/tests/app.rs               the whole slice without a window (Task 13)
crates/photon-ui/tests/screenshots.rs       the grid as PNG (Task 14)
crates/xtask/src/native_shot.rs             `xtask native-shot` (Task 14)
CLAUDE.md                                   the native UI's section (Task 15)
```

---

### Task 1: A cached thumbnail, decoded

The loader reads thumbnails the way the look-alike pass does, through libwebp, and nothing outside `photon-core` names libwebp. Two public functions over the decode `ThumbCache::read` already has.

**Files:**
- Modify: `crates/photon-core/src/thumbs/cache.rs` (`read`, near line 190)
- Modify: `crates/photon-core/src/thumbs/service.rs` (beside `path_for`, near line 688; and its `tests` module)

**Interfaces:**
- Consumes: `ThumbCache::read(&self, fp: u64, size: ThumbSize) -> Result<DynamicImage>` (crate-private, existing).
- Produces: `ThumbCache::decode(path: &Path) -> Result<DynamicImage>`; `ThumbService::decoded(&self, key: u64, size: ThumbSize) -> Result<image::RgbaImage>`; `ThumbService::decode_file(path: &Path) -> Result<image::RgbaImage>`. Task 13's `thumbs/source.rs` calls the last two.

*Not compiled while the plan was written:* the scratch build could not change `photon-core`. The bodies are three lines each over code that exists.

- [ ] **Step 1: Write the test**

In `crates/photon-core/src/thumbs/service.rs`, in `mod tests`, after `get_or_generate_builds_on_demand`:

```rust
    // What the native grid reads a tile's picture with: by key, from the cache alone.
    #[test]
    fn a_cached_thumbnail_is_decoded_by_its_key_and_an_uncached_one_is_not_built() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        let key = lib.item(ids[0]).unwrap().unwrap().thumb_key();
        assert!(service.decoded(key, ThumbSize::Grid).is_err());
        assert_ne!(state(&lib, ids[0]), ThumbState::Ready, "asking built nothing");

        let path = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        let by_key = service.decoded(key, ThumbSize::Grid).unwrap();
        assert_eq!(by_key.dimensions(), (64, 32));
        assert_eq!(ThumbService::decode_file(&path).unwrap(), by_key);
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p photon-core --lib a_cached_thumbnail_is_decoded_by_its_key`
Expected: does not compile - `no method named decoded`.

- [ ] **Step 3: Write the functions**

In `crates/photon-core/src/thumbs/cache.rs`, replace the `read` function (keep its doc comment on `decode`, where the reasoning now lives) with:

```rust
    /// Decodes a thumbnail file with libwebp, the library that wrote it, rather than with
    /// `image`'s pure-Rust `image-webp`. The pixels are the same - the look-alike pass stored
    /// every `percep_hash` before this from `image-webp`'s decode, and a new hash has to be
    /// comparable with those (`reading_gives_image_webps_pixels` holds it) - and the decode
    /// is about three times faster: ~0.10 ms against ~0.32 for a grid thumbnail. That pass
    /// reads every grid thumbnail in the library on its first run over it, and the native
    /// grid reads one for every tile it draws.
    pub fn decode(path: &Path) -> Result<DynamicImage> {
        let bytes = fs::read(path)?;
        // `None` is libwebp refusing the file, or an animation, which photon never writes.
        webp::Decoder::new(&bytes)
            .decode()
            .map(|img| img.to_image())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "not a still WebP").into()
            })
    }

    /// The thumbnail cached for `fp` at `size`, decoded; see `decode`.
    pub(crate) fn read(&self, fp: u64, size: ThumbSize) -> Result<DynamicImage> {
        Self::decode(&self.path_for(fp, size))
    }
```

(If `Path` is not imported in `cache.rs`, it is `std::path::Path`; the file already uses `PathBuf`.)

In `crates/photon-core/src/thumbs/service.rs`, directly after `path_for`:

```rust
    /// The picture cached under `key`, decoded to straight RGBA. The cache and nothing
    /// else: no database read, no render, no wait. An error when none is cached there,
    /// which is how a caller learns to ask for it (`request_async`).
    pub fn decoded(&self, key: u64, size: ThumbSize) -> Result<image::RgbaImage> {
        Ok(self.cache.read(key, size)?.to_rgba8())
    }

    /// The thumbnail file at `path` - one `request_async` answered with - decoded the same
    /// way.
    pub fn decode_file(path: &Path) -> Result<image::RgbaImage> {
        Ok(ThumbCache::decode(path)?.to_rgba8())
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib thumbs`
Expected: PASS, the new test among them, and `reading_gives_image_webps_pixels` still passing.

- [ ] **Step 5: Probe**

In `decode_file`, replace the body `Ok(ThumbCache::decode(path)?.to_rgba8())` with `let _ = path;` followed by `Ok(image::RgbaImage::new(1, 1))`. Run the test: expected FAIL on the last assertion. Restore, `touch crates/photon-core/src/thumbs/service.rs`, run again: PASS.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core/src/thumbs/cache.rs crates/photon-core/src/thumbs/service.rs
git commit -m "feat(core): a cached thumbnail decoded by its key, for a caller outside the crate

The native grid reads a tile's picture from the cache as the look-alike pass does, through
libwebp, and nothing outside photon-core names libwebp: ThumbCache::decode is the decode
read() had, and ThumbService::decoded and decode_file hand back straight RGBA.

The test was shown to fail with decode_file answering a blank picture.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: The crate, its arguments and its directories

**Files:**
- Create: `crates/photon-ui/Cargo.toml`, `crates/photon-ui/src/lib.rs`, `crates/photon-ui/src/args.rs`, `crates/photon-ui/src/dirs.rs`
- Modify: `Cargo.toml` (the workspace)

**Interfaces:**
- Consumes: nothing.
- Produces: `dirs::IDENTIFIER: &str`; `dirs::Dirs { pub db_path: PathBuf, pub cache_dir: PathBuf }`; `dirs::within(app_data: &Path, app_cache: &Path) -> Dirs`; `dirs::standard() -> Option<Dirs>`; `args::Args { data_dir, cache_dir, fullscreen }`, `Args::parse(impl IntoIterator<Item = String>) -> Result<Args, String>`, `Args::dirs(&self) -> Option<Dirs>`, `args::USAGE`.

- [ ] **Step 1: The workspace**

In the workspace `Cargo.toml`, add the member and raise the Rust version: eframe 0.36.2 needs 1.95 and `fastframe-fonts` 0.4.1 needs 1.98.

```toml
members = ["crates/photon-core", "crates/photon-engine", "crates/photon-app", "crates/photon-ui", "crates/xtask"]
```

```toml
rust-version = "1.98"
```

- [ ] **Step 2: `crates/photon-ui/Cargo.toml`**

```toml
[package]
name = "photon-ui"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description.workspace = true
repository.workspace = true
authors.workspace = true

[dependencies]
photon-core = { path = "../photon-core" }
photon-engine = { path = "../photon-engine" }
# Pinned to the patch: egui changes behaviour, and its API, between releases. This is the
# version `pch/rawmakase` ships on the same three platforms.
eframe = { version = "=0.36.2", default-features = false, features = ["default_fonts", "wgpu", "wayland", "x11"] }
# egui's SVG loader, for the icons.
egui_extras = { version = "=0.36.2", default-features = false, features = ["svg"] }
# The platform's interface face, and an installed face for every script it lacks. MIT. Not
# on crates.io, so pinned to a tag; without its `inter` feature it bundles no font.
fastframe-fonts = { git = "https://github.com/crmne/fastframe", tag = "v0.4.1", default-features = false }
# The runs of a line that mixes right-to-left and left-to-right text (`text.rs`).
unicode-bidi = "0.3.18"
# The data and cache directories, through the crate Tauri resolves them with.
dirs = "6.0.0"
# The viewer's time zone, for the month a folder's header names. photon-core's version.
jiff = "0.2.35"
parking_lot = "0.12.5"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }

[dev-dependencies]
# Whole frames of the application without a window (`eframe`), and off-screen rendering
# for `tests/screenshots.rs` (`wgpu`).
egui_kittest = { version = "=0.36.2", features = ["wgpu", "eframe"] }
image = { version = "0.25.10", default-features = false, features = ["jpeg"] }
tempfile = "3.27.0"
```

There is no `[[bin]]` yet: the binary and its `main.rs` are Task 13's, and until then this is a library.

- [ ] **Step 3: `crates/photon-ui/src/dirs.rs`**

```rust
//! Where the library and the thumbnail cache are: the directories the Tauri shell resolves,
//! so both applications open the same library.
//!
//! Tauri's `app_data_dir` is `dirs::data_dir()` joined with the bundle identifier, and
//! `app_cache_dir` is `dirs::cache_dir()` joined with it (tauri 2.11.5,
//! `src/path/desktop.rs`); `app.rs` then names `library.db` in the first and `thumbs` in
//! the second. A different answer here is not an error anyone sees: it is an empty library.

use std::path::{Path, PathBuf};

/// `identifier` in `crates/photon-app/tauri.conf.json`.
pub const IDENTIFIER: &str = "io.github.bsg62.photon";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    pub db_path: PathBuf,
    pub cache_dir: PathBuf,
}

/// The paths inside an app data directory and an app cache directory. This is what
/// `--data-dir` and `--cache-dir` name: the directories that hold `library.db` and `thumbs`.
pub fn within(app_data: &Path, app_cache: &Path) -> Dirs {
    Dirs {
        db_path: app_data.join("library.db"),
        cache_dir: app_cache.join("thumbs"),
    }
}

/// The standard places, or `None` on a system that has no home directory to put them in.
pub fn standard() -> Option<Dirs> {
    Some(within(
        &dirs::data_dir()?.join(IDENTIFIER),
        &dirs::cache_dir()?.join(IDENTIFIER),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_and_the_cache_are_named_as_the_tauri_shell_names_them() {
        let dirs = within(Path::new("/data/app"), Path::new("/cache/app"));
        assert_eq!(dirs.db_path, Path::new("/data/app/library.db"));
        assert_eq!(dirs.cache_dir, Path::new("/cache/app/thumbs"));
    }

    #[test]
    fn the_identifier_is_the_tauri_shells() {
        let conf = include_str!("../../photon-app/tauri.conf.json");
        assert!(
            conf.contains(&format!("\"identifier\": \"{IDENTIFIER}\"")),
            "tauri.conf.json names another identifier"
        );
    }

    /// The directory a platform keeps application data in, read from the environment the
    /// way the platform defines it and not through the `dirs` crate.
    #[cfg(target_os = "linux")]
    fn platform_dirs() -> (PathBuf, PathBuf) {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let or = |var: &str, fallback: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
        };
        (
            or("XDG_DATA_HOME", ".local/share"),
            or("XDG_CACHE_HOME", ".cache"),
        )
    }

    #[cfg(target_os = "macos")]
    fn platform_dirs() -> (PathBuf, PathBuf) {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        (
            home.join("Library/Application Support"),
            home.join("Library/Caches"),
        )
    }

    #[cfg(target_os = "windows")]
    fn platform_dirs() -> (PathBuf, PathBuf) {
        let var = |name: &str| PathBuf::from(std::env::var_os(name).unwrap());
        (var("APPDATA"), var("LOCALAPPDATA"))
    }

    // What an existing library depends on: the data is in the roaming or shared data
    // directory and the cache in the cache directory, each under the identifier.
    #[test]
    fn the_standard_places_are_the_platforms_data_and_cache_directories() {
        let (data, cache) = platform_dirs();
        let dirs = standard().unwrap();
        assert_eq!(dirs.db_path, data.join(IDENTIFIER).join("library.db"));
        assert_eq!(dirs.cache_dir, cache.join(IDENTIFIER).join("thumbs"));
    }
}
```

- [ ] **Step 4: `crates/photon-ui/src/args.rs`**

```rust
//! The command line: where the library is, and how the window opens.

use crate::dirs::{self, Dirs};
use std::path::PathBuf;

pub const USAGE: &str = "usage: photon-native [--data-dir DIR] [--cache-dir DIR] [--fullscreen]

  --data-dir DIR    the directory that holds library.db
  --cache-dir DIR   the directory that holds thumbs/
  --fullscreen      open fullscreen

Without --data-dir and --cache-dir the library the Tauri photon opens is opened.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub data_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub fullscreen: bool,
}

impl Args {
    /// The arguments after the program's name. An error is the line to print before the
    /// usage.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("{name} needs a directory"))
            };
            match arg.as_str() {
                "--data-dir" => parsed.data_dir = Some(value("--data-dir")?),
                "--cache-dir" => parsed.cache_dir = Some(value("--cache-dir")?),
                "--fullscreen" => parsed.fullscreen = true,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        // One without the other would open one application's library with another's
        // thumbnails: every key would miss, and the cache would fill with a second copy.
        if parsed.data_dir.is_some() != parsed.cache_dir.is_some() {
            return Err("--data-dir and --cache-dir go together".to_owned());
        }
        Ok(parsed)
    }

    /// Where the library is: the two directories named, or the standard ones.
    pub fn dirs(&self) -> Option<Dirs> {
        match (&self.data_dir, &self.cache_dir) {
            (Some(data), Some(cache)) => Some(dirs::within(data, cache)),
            _ => dirs::standard(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn no_arguments_is_the_standard_library_in_a_window() {
        let args = parse(&[]).unwrap();
        assert_eq!(args, Args::default());
        assert_eq!(args.dirs(), dirs::standard());
    }

    #[test]
    fn the_two_directories_name_another_library() {
        let args = parse(&[
            "--data-dir",
            "/x/data",
            "--cache-dir",
            "/x/cache",
            "--fullscreen",
        ])
        .unwrap();
        assert!(args.fullscreen);
        let dirs = args.dirs().unwrap();
        assert_eq!(dirs.db_path, Path::new("/x/data/library.db"));
        assert_eq!(dirs.cache_dir, Path::new("/x/cache/thumbs"));
    }

    #[test]
    fn one_directory_without_the_other_is_refused() {
        assert_eq!(
            parse(&["--data-dir", "/x/data"]),
            Err("--data-dir and --cache-dir go together".to_owned())
        );
        assert!(parse(&["--cache-dir", "/x/cache"]).is_err());
    }

    #[test]
    fn a_directory_left_out_and_an_unknown_argument_are_errors() {
        assert_eq!(
            parse(&["--data-dir"]),
            Err("--data-dir needs a directory".to_owned())
        );
        assert_eq!(
            parse(&["--frobnicate"]),
            Err("unknown argument --frobnicate".to_owned())
        );
    }
}
```

- [ ] **Step 5: `crates/photon-ui/src/lib.rs`**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p photon-ui --lib`
Expected: 7 tests pass (4 in `args`, 3 in `dirs`). The first build compiles eframe and wgpu and takes a minute or two.

- [ ] **Step 7: Probes**

**`the_standard_places_are_the_platforms_data_and_cache_directories`** - in `crates/photon-ui/src/dirs.rs` replace

```rust
        db_path: app_data.join("library.db"),
```

with

```rust
        db_path: app_data.join("photon.db"),
```

Run: `cargo test -p photon-ui --lib the_standard_places_are_the_platforms_data_and_cache_directories`
Expected: FAIL.

**`one_directory_without_the_other_is_refused`** - in `crates/photon-ui/src/args.rs` replace

```rust
        if parsed.data_dir.is_some() != parsed.cache_dir.is_some() {
```

with

```rust
        if false && parsed.data_dir.is_some() != parsed.cache_dir.is_some() {
```

Run: `cargo test -p photon-ui --lib one_directory_without_the_other_is_refused`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

- [ ] **Step 8: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add Cargo.toml Cargo.lock crates/photon-ui
git commit -m "feat(ui): the native UI's crate, and where it finds the library

photon-ui will hold the interface drawn by egui. It opens the library the Tauri shell
opens: the data and cache directories Tauri resolves for the identifier, read from Tauri's
own source and pinned by a test per platform, with --data-dir and --cache-dir to name
another pair together.

The workspace's Rust version is 1.98: eframe 0.36.2 needs 1.95 and fastframe-fonts 1.98.

Probed: library.db under another name, and one directory accepted without the other.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The tokens

**Files:**
- Create: `crates/photon-ui/src/theme/tokens.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: `ui/src/tokens.css`, read as text by the test.
- Produces: `tokens::Rgba(pub u8, pub u8, pub u8, pub u8)`; `tokens::Palette` with the fourteen themed colours as fields; `tokens::LIGHT`, `tokens::DARK`; `tokens::SHADOW_INK`, `PHOTO_LINE`, `SCRIM`; the scales `tokens::R: [f32; 4]`, `S: [f32; 6]`, `T: [f32; 5]`.

- [ ] **Step 1: `crates/photon-ui/src/theme/tokens.rs`**

```rust
//! Every colour in photon, and the scales: `ui/src/tokens.css` as constants.
//!
//! Until the switch-over that file is the source and this one a copy, held to it by
//! `the_tokens_are_the_stylesheets`. Afterwards this is the source, and the contrast
//! assertions of `lib/tokens.test.ts` move here with it.

/// A colour as the stylesheet writes it: straight (not premultiplied) red, green, blue and
/// alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

/// `#rrggbb`.
const fn rgb(hex: u32) -> Rgba {
    Rgba((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 0xff)
}

/// `#rrggbbaa`.
const fn rgba(hex: u32) -> Rgba {
    Rgba(
        (hex >> 24) as u8,
        (hex >> 16) as u8,
        (hex >> 8) as u8,
        hex as u8,
    )
}

/// The colours that differ between the light and the dark theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub surface: Rgba,
    pub chrome: Rgba,
    pub raised: Rgba,
    pub field: Rgba,
    pub field_hover: Rgba,
    pub hover: Rgba,
    pub line: Rgba,
    pub text: Rgba,
    pub text_dim: Rgba,
    pub accent: Rgba,
    pub accent_soft: Rgba,
    pub on_accent: Rgba,
    pub danger: Rgba,
    pub star: Rgba,
}

pub const LIGHT: Palette = Palette {
    surface: rgb(0xffffff),
    chrome: rgb(0xebebed),
    raised: rgb(0xffffff),
    field: rgba(0x0000000d),
    field_hover: rgba(0x00000018),
    hover: rgba(0x0000000a),
    line: rgba(0x00000018),
    text: rgb(0x1f1f23),
    text_dim: rgb(0x5f5f67),
    accent: rgb(0x1f6fd6),
    accent_soft: rgba(0x1f6fd633),
    on_accent: rgb(0xffffff),
    danger: rgb(0xbf302b),
    star: rgb(0xe0a100),
};

pub const DARK: Palette = Palette {
    surface: rgb(0x1b1b1e),
    chrome: rgb(0x2c2c31),
    raised: rgb(0x36363c),
    field: rgba(0xffffff12),
    field_hover: rgba(0xffffff1f),
    hover: rgba(0xffffff0d),
    line: rgba(0xffffff14),
    text: rgb(0xededf0),
    text_dim: rgb(0xa6a6af),
    accent: rgb(0x62a0ea),
    accent_soft: rgba(0x62a0ea33),
    on_accent: rgb(0x111111),
    danger: rgb(0xff8d8d),
    star: rgb(0xffd24a),
};

impl Palette {
    /// Each colour under its name in the stylesheet.
    pub fn named(&self) -> [(&'static str, Rgba); 14] {
        [
            ("--surface", self.surface),
            ("--chrome", self.chrome),
            ("--raised", self.raised),
            ("--field", self.field),
            ("--field-hover", self.field_hover),
            ("--hover", self.hover),
            ("--line", self.line),
            ("--text", self.text),
            ("--text-dim", self.text_dim),
            ("--accent", self.accent),
            ("--accent-soft", self.accent_soft),
            ("--on-accent", self.on_accent),
            ("--danger", self.danger),
            ("--star", self.star),
        ]
    }
}

/// Ink for shadows cast onto photos, which are not themed: dark in both themes.
pub const SHADOW_INK: Rgba = rgba(0x000000b3);
/// Drawn onto a photo: a tile's copies mark and video badge. The same in both themes,
/// because the photo under it is.
pub const PHOTO_LINE: Rgba = rgba(0xffffffe6);
/// Dims whatever is behind. Dark in both themes.
pub const SCRIM: Rgba = rgba(0x000000a6);

/// The three colours no theme changes, under their names in the stylesheet.
pub const UNTHEMED: [(&str, Rgba); 3] = [
    ("--shadow-ink", SHADOW_INK),
    ("--photo-line", PHOTO_LINE),
    ("--scrim", SCRIM),
];

/// Corner radii, `--r-1` to `--r-4`.
pub const R: [f32; 4] = [4.0, 6.0, 8.0, 12.0];
/// Spacing, `--s-1` to `--s-6`.
pub const S: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];
/// Type sizes, `--t-1` to `--t-5`.
pub const T: [f32; 5] = [11.0, 12.0, 13.0, 15.0, 18.0];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const CSS: &str = include_str!("../../../../ui/src/tokens.css");

    /// The custom properties declared in the block that opens with `selector`, by name.
    fn block(selector: &str) -> HashMap<&'static str, &'static str> {
        let start = CSS
            .find(selector)
            .unwrap_or_else(|| panic!("no block for {selector}"));
        let body = &CSS[start..];
        let body = &body[body.find('{').unwrap() + 1..body.find('}').unwrap()];
        body.lines()
            .filter_map(|line| line.trim().strip_suffix(';')?.split_once(':'))
            .filter(|(name, _)| name.starts_with("--"))
            .map(|(name, value)| (name.trim(), value.trim()))
            .collect()
    }

    fn parse(value: &str) -> Rgba {
        let hex = value.strip_prefix('#').unwrap_or_else(|| panic!("{value}"));
        let number = u32::from_str_radix(hex, 16).unwrap();
        match hex.len() {
            6 => rgb(number),
            8 => rgba(number),
            _ => panic!("{value} is neither #rrggbb nor #rrggbbaa"),
        }
    }

    #[test]
    fn the_two_spellings_of_a_colour_are_read_alike() {
        assert_eq!(rgb(0x1f6fd6), Rgba(0x1f, 0x6f, 0xd6, 0xff));
        assert_eq!(rgba(0x1f6fd633), Rgba(0x1f, 0x6f, 0xd6, 0x33));
        assert_eq!(parse("#1f6fd6"), rgb(0x1f6fd6));
        assert_eq!(parse("#1f6fd633"), rgba(0x1f6fd633));
    }

    #[test]
    fn the_tokens_are_the_stylesheets() {
        for (selector, palette) in [
            ("[data-theme='light'] {", LIGHT),
            ("[data-theme='dark'] {", DARK),
        ] {
            let css = block(selector);
            for (name, colour) in palette.named() {
                assert_eq!(parse(css[name]), colour, "{name} in {selector}");
            }
        }
        // The scales and the unthemed colours are in the bare `:root` block.
        let root = block(":root {\n  --r-1");
        for (name, colour) in UNTHEMED {
            assert_eq!(parse(root[name]), colour, "{name}");
        }
        let px = |name: &str| {
            root[name]
                .strip_suffix("px")
                .unwrap()
                .parse::<f32>()
                .unwrap()
        };
        for (i, radius) in R.iter().enumerate() {
            assert_eq!(px(&format!("--r-{}", i + 1)), *radius);
        }
        for (i, space) in S.iter().enumerate() {
            assert_eq!(px(&format!("--s-{}", i + 1)), *space);
        }
        for (i, size) in T.iter().enumerate() {
            assert_eq!(px(&format!("--t-{}", i + 1)), *size);
        }
    }
}
```

- [ ] **Step 2: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod theme {
    pub mod tokens;
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-ui --lib tokens`
Expected: 2 tests pass.

- [ ] **Step 4: Probe**

**`the_tokens_are_the_stylesheets`** - in `crates/photon-ui/src/theme/tokens.rs` replace

```rust
pub const LIGHT: Palette = Palette {
    surface: rgb(0xffffff),
```

with

```rust
pub const LIGHT: Palette = Palette {
    surface: rgb(0xfffffe),
```

Run: `cargo test -p photon-ui --lib the_tokens_are_the_stylesheets`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/theme/tokens.rs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): the colours and scales of tokens.css as constants

Until the switch-over the stylesheet is the source and these a copy: a test reads it and
fails on a value that differs. Probed with one colour changed by a bit.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Work off the UI thread

**Files:**
- Create: `crates/photon-ui/src/tasks.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `tasks::Latest<J, R>` with `Latest::spawn(name: &str, work: impl Fn(J) -> R + Send + 'static, notify: impl Fn() + Send + 'static) -> Self`, `ask(&mut self, job: J)`, `answer(&mut self) -> Option<Result<R, Panicked>>`, `waiting(&self) -> bool`; `tasks::Panicked`.

- [ ] **Step 1: `crates/photon-ui/src/tasks.rs`**

```rust
//! Work the UI thread must not do itself: anything that reads SQLite or the filesystem.
//!
//! One `Latest` per kind of question. It keeps one pending question - a newer one replaces
//! it, since its answer is no longer wanted - and hands back only the answer to the latest
//! question asked. This is the job `LibraryStore`'s generation counters did in the Svelte
//! UI, where a late answer otherwise overwrote a newer one.

use parking_lot::{Condvar, Mutex};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    thread,
};

/// The work panicked. Its own answer fails; the worker goes on to the next question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panicked;

struct Slot<J> {
    pending: Option<(u64, J)>,
    closed: bool,
}

struct Shared<J> {
    slot: Mutex<Slot<J>>,
    wake: Condvar,
}

pub struct Latest<J, R> {
    shared: Arc<Shared<J>>,
    answers: Receiver<(u64, Result<R, Panicked>)>,
    /// The latest question's number. An answer carrying an older one is dropped.
    asked: u64,
    /// The number of the last answer handed out.
    answered: u64,
}

impl<J: Send + 'static, R: Send + 'static> Latest<J, R> {
    /// Starts the worker. `work` answers one question; `notify` is called after each answer
    /// is ready, from the worker's thread, so the UI can ask to be drawn again.
    pub fn spawn(
        name: &str,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot {
                pending: None,
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let (tx, answers) = mpsc::channel();
        let theirs = shared.clone();
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                loop {
                    let (number, job) = {
                        let mut slot = theirs.slot.lock();
                        loop {
                            if slot.closed {
                                return;
                            }
                            if let Some(pending) = slot.pending.take() {
                                break pending;
                            }
                            theirs.wake.wait(&mut slot);
                        }
                    };
                    let answer = catch_unwind(AssertUnwindSafe(|| work(job))).map_err(|_| Panicked);
                    if tx.send((number, answer)).is_err() {
                        return;
                    }
                    notify();
                }
            })
            .expect("a worker thread could not be started");
        Self {
            shared,
            answers,
            asked: 0,
            answered: 0,
        }
    }

    /// Asks. A question still waiting for the worker is replaced; one being worked on
    /// finishes, and its answer is dropped.
    pub fn ask(&mut self, job: J) {
        self.asked += 1;
        self.shared.slot.lock().pending = Some((self.asked, job));
        self.shared.wake.notify_one();
    }

    /// The answer to the latest question, once it is there, once.
    pub fn answer(&mut self) -> Option<Result<R, Panicked>> {
        let mut latest = None;
        while let Ok((number, answer)) = self.answers.try_recv() {
            if number == self.asked {
                self.answered = number;
                latest = Some(answer);
            }
        }
        latest
    }

    /// Whether a question has been asked whose answer has not been handed out.
    pub fn waiting(&self) -> bool {
        self.answered < self.asked
    }
}

/// The worker ends at its next wait. It is not joined: work in flight may be a database
/// read on a slow disk, and closing the window must not wait for it.
impl<J, R> Drop for Latest<J, R> {
    fn drop(&mut self) {
        self.shared.slot.lock().closed = true;
        self.shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc::{Sender, channel},
        time::{Duration, Instant},
    };

    /// Waits for an answer; a worker that never answers fails the test instead of hanging it.
    fn answer_of<J: Send + 'static, R: Send + 'static>(
        task: &mut Latest<J, R>,
    ) -> Result<R, Panicked> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(answer) = task.answer() {
                return answer;
            }
            assert!(Instant::now() < deadline, "no answer within ten seconds");
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// A worker that says when it has started a job and then waits to be let go.
    fn gated() -> (Latest<u32, u32>, Receiver<u32>, Sender<()>) {
        let (started_tx, started) = channel();
        let (go, gate) = channel::<()>();
        let task = Latest::spawn(
            "test",
            move |job: u32| {
                started_tx.send(job).unwrap();
                gate.recv().unwrap();
                job * 10
            },
            || {},
        );
        (task, started, go)
    }

    #[test]
    fn a_question_is_answered_once() {
        let mut task = Latest::spawn("test", |job: u32| job + 1, || {});
        assert!(!task.waiting());
        task.ask(1);
        assert!(task.waiting());
        assert_eq!(answer_of(&mut task), Ok(2));
        assert!(!task.waiting());
        assert_eq!(task.answer(), None);
    }

    #[test]
    fn a_newer_question_replaces_the_one_still_waiting() {
        let (mut task, started, go) = gated();
        task.ask(1);
        assert_eq!(started.recv().unwrap(), 1);
        // The worker is busy with 1: 2 waits, and 3 takes its place.
        task.ask(2);
        task.ask(3);
        go.send(()).unwrap();
        assert_eq!(started.recv().unwrap(), 3, "2 was never started");
        go.send(()).unwrap();
        assert_eq!(answer_of(&mut task), Ok(30));
    }

    #[test]
    fn an_answer_to_an_older_question_is_dropped() {
        let (mut task, started, go) = gated();
        task.ask(1);
        assert_eq!(started.recv().unwrap(), 1);
        task.ask(2);
        // 1 finishes after 2 was asked: its answer must never be handed out.
        go.send(()).unwrap();
        assert_eq!(started.recv().unwrap(), 2);
        assert_eq!(task.answer(), None);
        assert!(task.waiting());
        go.send(()).unwrap();
        assert_eq!(answer_of(&mut task), Ok(20));
    }

    #[test]
    fn a_job_that_panics_fails_its_own_answer_and_the_next_one_runs() {
        let mut task = Latest::spawn(
            "test",
            |job: u32| {
                assert!(job != 1, "the job this test breaks on purpose");
                job
            },
            || {},
        );
        task.ask(1);
        assert_eq!(answer_of(&mut task), Err(Panicked));
        task.ask(2);
        assert_eq!(answer_of(&mut task), Ok(2));
    }

    #[test]
    fn the_ui_is_told_when_an_answer_is_ready() {
        let (told_tx, told) = channel();
        let mut task = Latest::spawn("test", |job: u32| job, move || told_tx.send(()).unwrap());
        task.ask(7);
        told.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(task.answer(), Some(Ok(7)));
    }
}
```

- [ ] **Step 2: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod tasks;
pub mod theme {
    pub mod tokens;
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-ui --lib tasks`
Expected: 5 tests pass. The panic test prints its panic message to stderr; that is the job it breaks on purpose.

- [ ] **Step 4: Probes**

**`an_answer_to_an_older_question_is_dropped`** - in `crates/photon-ui/src/tasks.rs` replace

```rust
            if number == self.asked {
```

with

```rust
            if number <= self.asked {
```

Run: `cargo test -p photon-ui --lib an_answer_to_an_older_question_is_dropped`
Expected: FAIL.

**`a_job_that_panics_fails_its_own_answer_and_the_next_one_runs`** - in `crates/photon-ui/src/tasks.rs` replace

```rust
                    let answer = catch_unwind(AssertUnwindSafe(|| work(job))).map_err(|_| Panicked);
```

with

```rust
                    let answer: Result<R, Panicked> = Ok(work(job));
```

Run: `cargo test -p photon-ui --lib a_job_that_panics_fails_its_own_answer_and_the_next_one_runs`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

For `tasks panic` the failure is the test's own thread never getting its answer: the worker thread died with the job, and `answer_of` panics after its ten seconds.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/tasks.rs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): one worker per kind of question, answering only the latest

What reads SQLite must not run on the UI thread. Latest keeps one pending question, which
a newer one replaces, and hands back only the answer to the latest: the job LibraryStore's
generation counters did. A job that panics fails its own answer and the worker goes on.

Probed: an older answer handed out, and the panic let through.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Motion and geometry

`scroll-speed.svelte.ts` and the parts of `layout.ts` the slice uses, in Rust, with their vitest cases. Read the spec's "What immediate mode changes" first: `renderOverscan` survives as the *wanted range*, and `placeIn`, `fetchSpan`, `pageMove`, `itemsInRect`, `edgeScrollSpeed`, `scrollIntoGrid`, `scrollToStart`, `topFolderId`, `showsTimeline` and `layoutHeight` are deliberately not here.

**Files:**
- Create: `crates/photon-ui/src/grid/motion.rs`, `crates/photon-ui/src/grid/layout.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: `photon_core::grid::Section { folder_id: Option<i64>, offset: usize, count: usize, taken_at_min: i64, period: Option<Period> }`, `photon_core::library::GridTile`.
- Produces, in `motion`: `Direction { Down, Up }`; `Motion { Still, Scroll { direction, speed: f64, peak: f64 }, Jump { stream: bool } }`; `ScrollSpeed` (`Default`) with `motion()`, `sample(top: f64, at: f64, viewport: f64)`, `tick(now: f64)`, `settles_at() -> Option<f64>`; times in milliseconds.
- Produces, in `layout`: the constants `GAP`, `HEADER`, `SECTION_GAP`, `TILE_MAX`, `TILE_SETTLE_MS`, `RENDER_OVERSCAN`, `LEAD_MS`, `LEAD_OVERSCAN_MAX`, `TRAIL_OVERSCAN` (all `f64`); `RowKind { Header, Tiles }`; `Row { kind, section: usize, first: usize, count: usize, top: f64, height: f64 }`; `tile_width(GridTile) -> f64`, `tile_row(f64) -> f64`, `columns_for(width, tile) -> usize`, `tile_for(width, nominal) -> f64`, `has_header(&Section) -> bool`, `build_rows(&[Section], columns: usize, tile: f64) -> Vec<Row>`, `row_width(f64) -> f64`, `total_height(&[Row]) -> f64`, `row_index_at(&[Row], y) -> usize`, `visible_range(&[Row], top, viewport, overscan) -> (usize, usize)`, `wanted_overscan(Motion, viewport) -> (f64, f64)`, `wanted_range(&[Row], top, viewport, Motion) -> (usize, usize)`, `defers_thumbs(Motion, viewport) -> bool`, `item_span(&[Row]) -> Option<(usize, usize)>`, `row_of_item(&[Row], offset) -> Option<usize>`, `header_rows(&[Row]) -> Vec<usize>`, `PinnedHeader { section: usize, y: f64 }`, `pinned_header(&[Row], &[usize], top) -> Option<PinnedHeader>`, `Pin { offset: usize, header: bool, into: f64 }`, `pin_at(&[Row], top) -> Option<Pin>`, `pin_top(&[Row], Pin) -> Option<f64>`.

- [ ] **Step 1: `crates/photon-ui/src/grid/motion.rs`**

```rust
//! What the grid's scroll is doing: still, moving continuously, or jumping. The wanted
//! range and the thumbnail deferral are sized by it (`layout::wanted_overscan`,
//! `layout::defers_thumbs`).

/// How long without a move before a scroll counts as over. Also the longest gap two moves
/// can have and still be measured against each other: the first move after a pause has
/// nothing recent to be a speed relative to.
pub const SCROLL_SETTLE_MS: f64 = 150.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
}

/// - `Still`: no move for `SCROLL_SETTLE_MS`.
/// - `Scroll`: continuous movement - each move was less than a viewport, so what the next
///   frame shows overlaps what this one showed. `speed` is the latest measured speed in
///   pixels per millisecond, 0 when there was no recent move to measure against; `peak` is
///   the fastest this run has gone in this direction, which is what the lead is sized by,
///   so a flick slowing down does not give up the thumbnails it asked for ahead of itself.
/// - `Jump`: one move of a viewport or more - End, a scrollbar drag. Nothing on screen
///   before it is on screen after it. `stream` is true when it came within
///   `SCROLL_SETTLE_MS` of the previous move - a drag, where the next jump is already on
///   its way - and false for a jump on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Still,
    Scroll {
        direction: Direction,
        speed: f64,
        peak: f64,
    },
    Jump {
        stream: bool,
    },
}

/// Follows the grid's position and says what kind of movement it is.
///
/// Speed is distance over the time between two samples, never distance per sample: a frame
/// is a sample, so pixels per sample would call the same gesture twice as fast on a 60Hz
/// screen as on a 120Hz one. A jump *is* measured per sample, on purpose: it is a statement
/// about two consecutive frames - whether the second shares any rows with the first - not
/// about how fast anything is moving.
#[derive(Debug)]
pub struct ScrollSpeed {
    motion: Motion,
    // The position is known across a pause - the grid stays where it was - so a jump after
    // one is still measured from it. Only the time is forgotten.
    last_top: f64,
    last_at: Option<f64>,
}

impl Default for ScrollSpeed {
    fn default() -> Self {
        Self {
            motion: Motion::Still,
            last_top: 0.0,
            last_at: None,
        }
    }
}

impl ScrollSpeed {
    pub fn motion(&self) -> Motion {
        self.motion
    }

    /// The position after a frame's input, the time in milliseconds, and the viewport's
    /// height, which is what decides whether the move was a jump. Called for every frame
    /// in which the position changed.
    pub fn sample(&mut self, top: f64, at: f64, viewport: f64) {
        let moved = top - self.last_top;
        let elapsed = self.last_at.map(|last| at - last);
        let recent = elapsed.is_some_and(|e| e <= SCROLL_SETTLE_MS);
        if moved != 0.0 {
            if moved.abs() >= viewport {
                self.motion = Motion::Jump { stream: recent };
            } else {
                let direction = if moved > 0.0 {
                    Direction::Down
                } else {
                    Direction::Up
                };
                let same = match self.motion {
                    Motion::Scroll {
                        direction: d,
                        speed,
                        peak,
                    } if d == direction => Some((speed, peak)),
                    _ => None,
                };
                // Two samples with the same time say nothing about speed; the run's own
                // speed stands until one that does.
                let speed = match elapsed {
                    Some(e) if recent && e > 0.0 => moved.abs() / e,
                    _ => same.map_or(0.0, |(speed, _)| speed),
                };
                let peak = speed.max(same.map_or(0.0, |(_, peak)| peak));
                self.motion = Motion::Scroll {
                    direction,
                    speed,
                    peak,
                };
            }
        }
        self.last_top = top;
        self.last_at = Some(at);
    }

    /// Settles the motion once `SCROLL_SETTLE_MS` have passed without a sample. Called
    /// every frame; the timer of the Svelte grid, made a question.
    pub fn tick(&mut self, now: f64) {
        if self.last_at.is_some_and(|at| now - at >= SCROLL_SETTLE_MS) {
            self.motion = Motion::Still;
            self.last_at = None;
        }
    }

    /// When `tick` will next change anything, for the frame to ask to be drawn again then:
    /// with no input nothing else would wake it.
    pub fn settles_at(&self) -> Option<f64> {
        self.last_at.map(|at| at + SCROLL_SETTLE_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: f64 = 800.0;

    fn scroll(direction: Direction, speed: f64, peak: f64) -> Motion {
        Motion::Scroll {
            direction,
            speed,
            peak,
        }
    }

    #[test]
    fn speed_is_measured_by_time_and_not_by_sample() {
        // The same distance per sample at 60Hz and at 240Hz: four times the speed.
        let mut slow = ScrollSpeed::default();
        let mut quick = ScrollSpeed::default();
        for i in 0..5 {
            let i = f64::from(i);
            slow.sample(i * 40.0, 1000.0 + i * 16.0, VIEWPORT);
            quick.sample(i * 40.0, 1000.0 + i * 4.0, VIEWPORT);
        }
        assert_eq!(slow.motion(), scroll(Direction::Down, 2.5, 2.5));
        assert_eq!(quick.motion(), scroll(Direction::Down, 10.0, 10.0));
    }

    #[test]
    fn a_flick_is_a_scroll_however_fast_while_each_frame_overlaps_the_last() {
        // 40px/ms at 60Hz is 640px a frame: fast, but continuous.
        let mut speed = ScrollSpeed::default();
        for i in 0..5 {
            let i = f64::from(i);
            speed.sample(10_000.0 - i * 640.0, 1000.0 + i * 16.0, VIEWPORT);
        }
        assert_eq!(speed.motion(), scroll(Direction::Up, 40.0, 40.0));
    }

    #[test]
    fn a_move_of_a_viewport_or_more_in_one_sample_is_a_jump() {
        let mut speed = ScrollSpeed::default();
        speed.sample(100.0, 1000.0, VIEWPORT);
        speed.sample(100.0 + VIEWPORT, 1016.0, VIEWPORT);
        assert_eq!(speed.motion(), Motion::Jump { stream: true });
    }

    #[test]
    fn a_jump_after_a_pause_is_a_jump_on_its_own() {
        let mut speed = ScrollSpeed::default();
        speed.sample(100.0, 1000.0, VIEWPORT);
        speed.tick(1000.0 + SCROLL_SETTLE_MS);
        speed.sample(1_000_000.0, 1000.0 + SCROLL_SETTLE_MS + 1.0, VIEWPORT);
        assert_eq!(speed.motion(), Motion::Jump { stream: false });
    }

    #[test]
    fn the_place_is_remembered_across_a_pause_so_a_small_move_after_one_is_no_jump() {
        let mut speed = ScrollSpeed::default();
        speed.sample(50_000.0, 1000.0, VIEWPORT);
        speed.tick(1000.0 + SCROLL_SETTLE_MS);
        speed.sample(50_100.0, 5000.0, VIEWPORT);
        assert_eq!(speed.motion(), scroll(Direction::Down, 0.0, 0.0));
    }

    #[test]
    fn the_peak_is_kept_while_a_flick_slows_and_starts_again_on_a_reversal() {
        let mut speed = ScrollSpeed::default();
        speed.sample(0.0, 1000.0, VIEWPORT);
        speed.sample(160.0, 1016.0, VIEWPORT); // 10px/ms
        speed.sample(200.0, 1032.0, VIEWPORT); // 2.5px/ms
        assert_eq!(speed.motion(), scroll(Direction::Down, 2.5, 10.0));

        speed.sample(168.0, 1048.0, VIEWPORT); // back up at 2px/ms
        assert_eq!(speed.motion(), scroll(Direction::Up, 2.0, 2.0));
    }

    #[test]
    fn it_settles_once_the_samples_stop_and_says_when() {
        let mut speed = ScrollSpeed::default();
        assert_eq!(speed.settles_at(), None);
        speed.sample(0.0, 0.0, VIEWPORT);
        speed.sample(200.0, 16.0, VIEWPORT);
        assert_eq!(speed.settles_at(), Some(16.0 + SCROLL_SETTLE_MS));

        speed.tick(16.0 + SCROLL_SETTLE_MS - 1.0);
        assert!(matches!(speed.motion(), Motion::Scroll { .. }));
        speed.tick(16.0 + SCROLL_SETTLE_MS);
        assert_eq!(speed.motion(), Motion::Still);
        assert_eq!(speed.settles_at(), None);
    }

    #[test]
    fn the_measured_speed_stands_across_two_samples_with_the_same_time() {
        let mut speed = ScrollSpeed::default();
        speed.sample(0.0, 0.0, VIEWPORT);
        speed.sample(32.0, 16.0, VIEWPORT);
        speed.sample(500.0, 16.0, VIEWPORT);
        assert_eq!(speed.motion(), scroll(Direction::Down, 2.0, 2.0));
    }
}
```

- [ ] **Step 2: `crates/photon-ui/src/grid/layout.rs`**

```rust
//! Grid geometry: square tiles in rows of one height, with a header row for every section
//! that names a folder or a period. `ui/src/lib/layout.ts` in Rust, constant for constant,
//! until the switch-over deletes that file.
//!
//! Everything is pure, and in `f64`: a large library is taller than an `f32` can count
//! pixels in (it is exact only to 16,777,216), and every `top` here is such a count.

use super::motion::{Direction, Motion};
use photon_core::{grid::Section, library::GridTile};

pub const GAP: f64 = 8.0;
pub const HEADER: f64 = 32.0;
/// Extra space above every section's header but the first, on top of the gutter under the
/// last row before it, so one folder reads as ending before the next begins.
pub const SECTION_GAP: f64 = 24.0;
/// The widest a tile is drawn: an eighth past `ThumbSize::Grid`'s 256px edge, which is how
/// much of an enlargement goes unseen.
pub const TILE_MAX: f64 = 288.0;
/// How long a tile must be wanted before its thumbnail is asked for, where asking is
/// deferred at all (`defers_thumbs`).
pub const TILE_SETTLE_MS: f64 = 100.0;
/// How far past the viewport thumbnails are wanted while the grid is still, in viewports,
/// either side: enough that a wheel notch lands on rows that have their pictures.
pub const RENDER_OVERSCAN: f64 = 0.5;
/// While scrolling, how far ahead of the direction of travel thumbnails are wanted, as
/// time: a tile wanted this long before it comes into view has been read and decoded
/// before anyone sees it.
pub const LEAD_MS: f64 = 300.0;
/// The most the lead grows to, in viewports.
pub const LEAD_OVERSCAN_MAX: f64 = 1.5;
/// What stays wanted behind a scroll, in viewports: a small reversal lands on rows that
/// still have their pictures.
pub const TRAIL_OVERSCAN: f64 = 0.25;

/// How wide a tile is at each size, before it is widened to fill its row (`tile_for`).
///
/// Every step is at or below 256, `ThumbSize::Grid`'s maximum edge: a step above it needs
/// its own `ThumbSize`, not a larger `Grid`.
pub fn tile_width(size: GridTile) -> f64 {
    match size {
        GridTile::Small => 120.0,
        GridTile::Medium => 160.0,
        GridTile::Large => 224.0,
    }
}

/// A tile row's full height: the tile plus the gutter under it.
pub fn tile_row(tile: f64) -> f64 {
    tile + GAP
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Header,
    Tiles,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    /// Index into the sections.
    pub section: usize,
    /// Grid offset of the first photo (the section's offset, for a header).
    pub first: usize,
    /// Photos in this row (0 for a header).
    pub count: usize,
    pub top: f64,
    pub height: f64,
}

pub fn columns_for(width: f64, tile: f64) -> usize {
    (((width + GAP) / tile_row(tile)).floor() as usize).max(1)
}

/// How wide a tile is drawn in a row `width` wide: the size chosen, widened so the columns
/// that fit fill the row. A whole number, so every row's `top` is one too; never narrower
/// than the size chosen, which a row too narrow for one tile overflows.
pub fn tile_for(width: f64, nominal: f64) -> f64 {
    let columns = columns_for(width, nominal) as f64;
    let share = ((width - (columns - 1.0) * GAP) / columns).floor();
    nominal.max(share.min(TILE_MAX))
}

/// A section has a header when it names a folder or a period; a flat view's run names
/// neither.
pub fn has_header(section: &Section) -> bool {
    section.folder_id.is_some() || section.period.is_some()
}

pub fn build_rows(sections: &[Section], columns: usize, tile: f64) -> Vec<Row> {
    let columns = columns.max(1);
    let mut rows = Vec::new();
    let mut top = 0.0;
    for (index, section) in sections.iter().enumerate() {
        if has_header(section) {
            if !rows.is_empty() {
                top += SECTION_GAP;
            }
            rows.push(Row {
                kind: RowKind::Header,
                section: index,
                first: section.offset,
                count: 0,
                top,
                height: HEADER,
            });
            top += HEADER;
        }
        let end = section.offset + section.count;
        let mut first = section.offset;
        while first < end {
            rows.push(Row {
                kind: RowKind::Tiles,
                section: index,
                first,
                count: columns.min(end - first),
                top,
                height: tile_row(tile),
            });
            top += tile_row(tile);
            first += columns;
        }
    }
    rows
}

/// The width of a row of tiles in a viewport `viewport` wide: what is left between the
/// gutter down either side.
pub fn row_width(viewport: f64) -> f64 {
    (viewport - 2.0 * GAP).max(0.0)
}

pub fn total_height(rows: &[Row]) -> f64 {
    rows.last().map_or(0.0, |last| last.top + last.height)
}

/// Index of the last item whose `key` is at or below `value`, or 0 if there is none.
pub fn last_index_at_or_before<T>(items: &[T], value: f64, key: impl Fn(&T) -> f64) -> usize {
    items
        .partition_point(|item| key(item) <= value)
        .saturating_sub(1)
}

/// Index of the last row whose top is at or below `y`: the row holding `y`.
pub fn row_index_at(rows: &[Row], y: f64) -> usize {
    last_index_at_or_before(rows, y, |row| row.top)
}

/// Rows intersecting `[top - overscan, top + viewport + overscan]`, as `start..end`.
pub fn visible_range(rows: &[Row], top: f64, viewport: f64, overscan: f64) -> (usize, usize) {
    if rows.is_empty() {
        return (0, 0);
    }
    let start = row_index_at(rows, top - overscan);
    let end = row_index_at(rows, top + viewport + overscan) + 1;
    (start, end.min(rows.len()))
}

/// How far past the viewport thumbnails are wanted, as `(above, below)`.
///
/// A jump wants only what is on screen: it shares no rows with the frame before it, and
/// during a drag the next frame replaces all of them. A continuous scroll needs a lead:
/// without one every row reaches the screen before its tiles have asked for anything.
pub fn wanted_overscan(motion: Motion, viewport: f64) -> (f64, f64) {
    match motion {
        Motion::Still => (viewport * RENDER_OVERSCAN, viewport * RENDER_OVERSCAN),
        Motion::Jump { .. } => (0.0, 0.0),
        Motion::Scroll {
            direction, peak, ..
        } => {
            let lead = (peak * LEAD_MS)
                .max(viewport * RENDER_OVERSCAN)
                .min(viewport * LEAD_OVERSCAN_MAX);
            let trail = viewport * TRAIL_OVERSCAN;
            match direction {
                Direction::Down => (trail, lead),
                Direction::Up => (lead, trail),
            }
        }
    }
}

/// The rows whose thumbnails are wanted, as `start..end`; see `wanted_overscan`.
pub fn wanted_range(rows: &[Row], top: f64, viewport: f64, motion: Motion) -> (usize, usize) {
    if rows.is_empty() {
        return (0, 0);
    }
    let (above, below) = wanted_overscan(motion, viewport);
    let start = row_index_at(rows, top - above);
    let end = row_index_at(rows, top + viewport + below) + 1;
    (start, end.min(rows.len()))
}

/// Whether the tiles in the wanted range wait before asking for their thumbnails.
///
/// Only when they will most likely be gone first: a tile never seen settled costs a render
/// for nothing - on a fresh import, a blocking one each. That is a jump in a stream (a
/// scrollbar drag), where the next frame replaces every tile, and a scroll so fast that a
/// tile crosses the whole wanted window in less than the settle. A jump on its own (End)
/// lands where the user stops, so it asks at once.
pub fn defers_thumbs(motion: Motion, viewport: f64) -> bool {
    match motion {
        Motion::Still => false,
        Motion::Jump { stream } => stream,
        Motion::Scroll { speed, .. } => {
            let (above, below) = wanted_overscan(motion, viewport);
            speed * TILE_SETTLE_MS > viewport + above + below
        }
    }
}

/// Grid offsets covered by the tile rows in `rows`, as `start..end`, or `None` if none.
pub fn item_span(rows: &[Row]) -> Option<(usize, usize)> {
    let mut span: Option<(usize, usize)> = None;
    for row in rows.iter().filter(|row| row.kind == RowKind::Tiles) {
        let (start, end) = span.unwrap_or((usize::MAX, 0));
        span = Some((start.min(row.first), end.max(row.first + row.count)));
    }
    span
}

/// Index of the tile row holding grid offset `offset`.
pub fn row_of_item(rows: &[Row], offset: usize) -> Option<usize> {
    let found = rows
        .partition_point(|row| row.first <= offset)
        .checked_sub(1)?;
    let row = &rows[found];
    (row.kind == RowKind::Tiles && offset < row.first + row.count).then_some(found)
}

/// The indexes of the header rows in `rows`, in order: what `pinned_header` searches, built
/// once per layout.
pub fn header_rows(rows: &[Row]) -> Vec<usize> {
    (0..rows.len())
        .filter(|&i| rows[i].kind == RowKind::Header)
        .collect()
}

/// The header drawn over the top of the grid: its section, and how far up it has been
/// pushed (`y`, zero or negative).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinnedHeader {
    pub section: usize,
    pub y: f64,
}

/// The header to pin to the top of the grid at `top`, or `None` for none.
///
/// It is the header of the section the top edge is inside - the space under a section's
/// last row included, where it is still that section the eye is leaving - and none while
/// that header is itself at the top, or over a run that has no header. The next header
/// pushes it out as it arrives, so the two never lie over each other.
pub fn pinned_header(rows: &[Row], headers: &[usize], top: f64) -> Option<PinnedHeader> {
    if headers.is_empty() || rows.is_empty() {
        return None;
    }
    let at = last_index_at_or_before(headers, top, |&i| rows[i].top);
    let header = &rows[headers[at]];
    if top <= header.top {
        return None;
    }
    if rows[row_index_at(rows, top)].section != header.section {
        return None;
    }
    let y = headers
        .get(at + 1)
        .map_or(0.0, |&next| (rows[next].top - top - HEADER).min(0.0));
    Some(PinnedHeader {
        section: header.section,
        y,
    })
}

/// A place in the grid that survives the rows changing height: the row at the top of the
/// viewport, by its first photo, and how much of it has been scrolled past.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pin {
    /// The grid offset of the row's first photo (for a header, of the section it heads).
    pub offset: usize,
    /// Whether the row is a header: a header and the row of photos under it share an offset.
    pub header: bool,
    /// The share of the row above the top of the viewport, 0 to 1.
    pub into: f64,
}

/// The row at the top of the viewport as a `Pin`, or `None` when the grid has no rows.
///
/// Tiles fill the row, so every row's `top` moves with every pixel of a resize, and a
/// position kept as a number names another photo afterwards. The share is kept because a
/// resize spends the pin on every frame: coming back to the row's top would jump by up to a
/// row on the first one. Past the end of the row - the space between two folders - is the
/// whole of it.
pub fn pin_at(rows: &[Row], top: f64) -> Option<Pin> {
    let row = rows.get(row_index_at(rows, top))?;
    Some(Pin {
        offset: row.first,
        header: row.kind == RowKind::Header,
        into: ((top - row.top) / row.height).clamp(0.0, 1.0),
    })
}

/// Where `pin` is in `rows`, as a position, or `None` when its photo is not there.
pub fn pin_top(rows: &[Row], pin: Pin) -> Option<f64> {
    let i = row_of_item(rows, pin.offset)?;
    let above = i.checked_sub(1).map(|above| &rows[above]);
    let row = match above {
        Some(above) if pin.header && above.kind == RowKind::Header && above.first == pin.offset => {
            above
        }
        _ => &rows[i],
    };
    Some(row.top + pin.into * row.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEDIUM: f64 = 160.0;
    const SMALL: f64 = 120.0;
    const LARGE: f64 = 224.0;

    fn folder(folder_id: i64, offset: usize, count: usize) -> Section {
        Section {
            folder_id: Some(folder_id),
            offset,
            count,
            taken_at_min: 0,
            period: None,
        }
    }

    /// A run drawn under no header, as a flat view's is.
    fn flat(offset: usize, count: usize) -> Section {
        Section {
            folder_id: None,
            ..folder(0, offset, count)
        }
    }

    fn two_folders() -> Vec<Section> {
        vec![folder(1, 0, 5), folder(2, 5, 3)]
    }

    fn shape(rows: &[Row]) -> Vec<(RowKind, usize, usize, f64)> {
        rows.iter()
            .map(|r| (r.kind, r.first, r.count, r.top))
            .collect()
    }

    #[test]
    fn sizes_are_the_svelte_grids() {
        assert_eq!(tile_width(GridTile::Small), SMALL);
        assert_eq!(tile_width(GridTile::Medium), MEDIUM);
        assert_eq!(tile_width(GridTile::Large), LARGE);
    }

    #[test]
    fn columns_fit_the_width() {
        assert_eq!(columns_for(800.0, MEDIUM), 4);
        assert_eq!(columns_for(100.0, MEDIUM), 1);
        assert_eq!(columns_for(0.0, MEDIUM), 1);
        assert_eq!(columns_for(-50.0, MEDIUM), 1);
    }

    #[test]
    fn a_section_is_a_header_row_and_its_tile_rows() {
        use RowKind::{Header, Tiles};
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(
            shape(&rows),
            [
                (Header, 0, 0, 0.0),
                (Tiles, 0, 2, 32.0),
                (Tiles, 2, 2, 200.0),
                (Tiles, 4, 1, 368.0),
                // SECTION_GAP (24) above every header but the first: folder 1 ends at 536.
                (Header, 5, 0, 560.0),
                (Tiles, 5, 2, 592.0),
                (Tiles, 7, 1, 760.0),
            ]
        );
        assert_eq!(total_height(&rows), 928.0);
        assert_eq!(total_height(&[]), 0.0);
    }

    #[test]
    fn a_run_that_names_no_folder_has_no_header() {
        use RowKind::Tiles;
        let rows = build_rows(&[flat(0, 5)], 2, MEDIUM);
        assert_eq!(
            shape(&rows),
            [
                (Tiles, 0, 2, 0.0),
                (Tiles, 2, 2, 168.0),
                (Tiles, 4, 1, 336.0)
            ]
        );
        assert_eq!(total_height(&rows), 504.0);
    }

    #[test]
    fn a_period_gets_a_header_as_a_folder_does() {
        let period = Section {
            period: Some(photon_core::grid::Period {
                year: 2024,
                month: Some(6),
                day: None,
            }),
            ..flat(0, 3)
        };
        assert!(has_header(&period));
        assert!(!has_header(&flat(0, 3)));
        assert_eq!(build_rows(&[period], 2, MEDIUM)[0].kind, RowKind::Header);
    }

    #[test]
    fn rows_are_hit_by_y() {
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(row_index_at(&rows, -5.0), 0);
        assert_eq!(row_index_at(&rows, 31.0), 0);
        assert_eq!(row_index_at(&rows, 32.0), 1);
        assert_eq!(row_index_at(&rows, 500.0), 3);
        assert_eq!(row_index_at(&rows, 10_000.0), 6);
    }

    #[test]
    fn visible_ranges_and_their_photos() {
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(visible_range(&rows, 200.0, 100.0, 0.0), (2, 3));
        assert_eq!(visible_range(&rows, 0.0, 10_000.0, 0.0), (0, 7));
        assert_eq!(visible_range(&[], 0.0, 100.0, 0.0), (0, 0));
        assert_eq!(item_span(&rows[0..3]), Some((0, 4)));
        assert_eq!(item_span(&rows[4..5]), None);
    }

    #[test]
    fn the_row_holding_a_photo() {
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(row_of_item(&rows, 0), Some(1));
        assert_eq!(row_of_item(&rows, 4), Some(3));
        assert_eq!(row_of_item(&rows, 5), Some(5));
        assert_eq!(row_of_item(&rows, 7), Some(6));
        assert_eq!(row_of_item(&rows, 8), None);
        assert_eq!(row_of_item(&[], 0), None);
    }

    mod wanted {
        use super::*;

        // One flat run of 1000 rows, 168px each: a 1680px viewport is ten rows.
        fn rows() -> Vec<Row> {
            build_rows(&[flat(0, 1000)], 1, MEDIUM)
        }
        const ROW: f64 = 168.0;
        const VIEWPORT: f64 = 10.0 * ROW;
        const TOP: f64 = 100.0 * ROW;

        fn scroll(direction: Direction, speed: f64, peak: f64) -> Motion {
            Motion::Scroll {
                direction,
                speed,
                peak,
            }
        }

        #[test]
        fn half_a_viewport_past_the_edges_while_still() {
            assert_eq!(
                wanted_range(&rows(), TOP, VIEWPORT, Motion::Still),
                (95, 116)
            );
        }

        #[test]
        fn only_what_is_on_screen_for_a_jump() {
            for stream in [true, false] {
                let jump = Motion::Jump { stream };
                assert_eq!(wanted_range(&rows(), TOP, VIEWPORT, jump), (100, 111));
            }
        }

        #[test]
        fn a_lead_ahead_of_a_fast_scroll_and_a_little_behind_it() {
            // 4px/ms leads by LEAD_MS of travel (1200px) and trails by a quarter viewport.
            let down = scroll(Direction::Down, 4.0, 4.0);
            assert_eq!(
                wanted_overscan(down, VIEWPORT),
                (VIEWPORT * TRAIL_OVERSCAN, 4.0 * LEAD_MS)
            );
            assert_eq!(wanted_range(&rows(), TOP, VIEWPORT, down), (97, 118));
            // Upward, the lead is above.
            let up = scroll(Direction::Up, 4.0, 4.0);
            assert_eq!(wanted_range(&rows(), TOP, VIEWPORT, up), (92, 113));
        }

        #[test]
        fn the_lead_is_at_least_the_still_overscan_and_at_most_its_maximum() {
            let below = |speed| wanted_overscan(scroll(Direction::Down, speed, speed), VIEWPORT).1;
            assert_eq!(below(0.0), VIEWPORT * RENDER_OVERSCAN);
            assert_eq!(below(0.1), VIEWPORT * RENDER_OVERSCAN);
            assert_eq!(below(100.0), VIEWPORT * LEAD_OVERSCAN_MAX);
        }

        #[test]
        fn the_lead_is_sized_by_the_peak_so_a_flick_slowing_down_keeps_it() {
            assert_eq!(
                wanted_range(&rows(), TOP, VIEWPORT, scroll(Direction::Down, 0.5, 4.0)),
                wanted_range(&rows(), TOP, VIEWPORT, scroll(Direction::Down, 4.0, 4.0)),
            );
        }

        #[test]
        fn thumbnails_wait_only_where_a_tile_will_be_gone_first() {
            assert!(!defers_thumbs(Motion::Still, VIEWPORT));
            // End lands where the user stops.
            assert!(!defers_thumbs(Motion::Jump { stream: false }, VIEWPORT));
            // A scrollbar drag replaces every tile on the next frame.
            assert!(defers_thumbs(Motion::Jump { stream: true }, VIEWPORT));
            // A 4px/ms flick passes a tile through the window in over half a second.
            assert!(!defers_thumbs(scroll(Direction::Down, 4.0, 4.0), VIEWPORT));
            // The window is 2.75 viewports at full lead: crossed within the settle above
            // 46.2px/ms.
            let window = VIEWPORT * (1.0 + LEAD_OVERSCAN_MAX + TRAIL_OVERSCAN);
            let at = |speed| defers_thumbs(scroll(Direction::Down, speed, speed), VIEWPORT);
            assert!(!at(window / TILE_SETTLE_MS - 0.1));
            assert!(at(window / TILE_SETTLE_MS + 0.1));
        }
    }

    mod filling_the_row {
        use super::*;

        #[test]
        fn tiles_widen_to_take_up_what_the_columns_leave_over() {
            // Four medium tiles and three gaps are 664; the 136 left over is 34 a tile.
            assert_eq!(columns_for(800.0, MEDIUM), 4);
            assert_eq!(tile_for(800.0, MEDIUM), 194.0);
        }

        #[test]
        fn a_row_already_filled_keeps_the_chosen_width() {
            assert_eq!(tile_for(4.0 * 160.0 + 3.0 * GAP, MEDIUM), 160.0);
        }

        #[test]
        fn a_tile_is_never_narrower_than_the_size_chosen() {
            assert_eq!(tile_for(100.0, MEDIUM), 160.0);
            assert_eq!(tile_for(0.0, LARGE), 224.0);
        }

        // The row is laid out for `columns_for` columns, so the widened tiles have to be
        // that many and fit: a tile widened past its share would push the last one of each
        // row off the edge.
        #[test]
        fn the_columns_fit_at_every_width_in_whole_pixels() {
            for nominal in [SMALL, MEDIUM, LARGE] {
                for width in nominal as u32..=3000 {
                    let width = f64::from(width);
                    let columns = columns_for(width, nominal) as f64;
                    let tile = tile_for(width, nominal);
                    assert_eq!(tile.fract(), 0.0, "{nominal} at {width}");
                    assert!(tile >= nominal, "{nominal} at {width}");
                    assert!(
                        columns * tile + (columns - 1.0) * GAP <= width,
                        "{nominal} at {width}"
                    );
                }
            }
        }

        // A grid thumbnail is 256px on its long edge: a tile drawn much wider shows it
        // enlarged. Three large tiles in 900px would be 294 each.
        #[test]
        fn a_tile_stops_an_eighth_past_the_thumbnail_it_draws() {
            assert_eq!(TILE_MAX, 288.0);
            assert_eq!(columns_for(900.0, LARGE), 3);
            assert_eq!(tile_for(900.0, LARGE), 288.0);
            assert_eq!(tile_for(860.0, LARGE), 281.0);
        }
    }

    mod keeping_the_place {
        use super::*;

        fn nine() -> Vec<Section> {
            vec![flat(0, 9)]
        }
        // Two folders, so there is a header partway down to land on.
        fn folders() -> Vec<Section> {
            vec![folder(1, 0, 5), folder(2, 5, 4)]
        }

        #[test]
        fn the_pin_is_the_first_photo_of_the_row_at_the_top() {
            let rows = build_rows(&nine(), 3, MEDIUM);
            let pin = |offset| Pin {
                offset,
                header: false,
                into: 0.0,
            };
            assert_eq!(pin_at(&rows, 0.0), Some(pin(0)));
            assert_eq!(pin_at(&rows, tile_row(MEDIUM)), Some(pin(3)));
            assert_eq!(pin_at(&[], 0.0), None);
        }

        #[test]
        fn it_names_the_photo_not_the_pixel_anywhere_within_a_row() {
            let rows = build_rows(&nine(), 3, MEDIUM);
            let row = tile_row(MEDIUM);
            assert_eq!(pin_at(&rows, 2.0 * row).unwrap().offset, 6);
            assert_eq!(pin_at(&rows, 2.0 * row + row - 1.0).unwrap().offset, 6);
        }

        // The pin is spent on every frame of a resize: one that put the row's top at the
        // top of the grid would jump by up to a row on the first frame.
        #[test]
        fn it_comes_back_to_the_same_part_of_the_row_not_to_its_top() {
            let medium = build_rows(&nine(), 3, MEDIUM);
            let wider = build_rows(&nine(), 3, 200.0);
            let pin = pin_at(&medium, tile_row(MEDIUM) + tile_row(MEDIUM) / 4.0).unwrap();
            assert_eq!(
                pin,
                Pin {
                    offset: 3,
                    header: false,
                    into: 0.25
                }
            );
            assert_eq!(
                pin_top(&wider, pin),
                Some(tile_row(200.0) + tile_row(200.0) / 4.0)
            );
        }

        // The pin is read from the layout the user was looking at and spent in the one
        // that replaces it. Medium at three columns and small at five share no row tops.
        #[test]
        fn it_finds_its_row_in_a_layout_it_was_not_taken_in() {
            let medium = build_rows(&folders(), 3, MEDIUM);
            let small = build_rows(&folders(), 5, SMALL);
            for top in [0.0, 40.0, 200.0, 500.0, total_height(&medium) - 1.0] {
                let pin = pin_at(&medium, top).unwrap();
                let back = pin_top(&small, pin).unwrap();
                let first = small[row_of_item(&small, pin.offset).unwrap()].first;
                assert_eq!(pin_at(&small, back).unwrap().offset, first, "from {top}");
            }
        }

        // At a section's header the eye is on that section's first photo: an answer with
        // the tile row above would scroll the user back into a folder they had left. And it
        // comes back to the header itself, not to the row under it, which shares its offset.
        #[test]
        fn a_header_answers_with_the_section_it_heads_and_comes_back_to_it() {
            let medium = build_rows(&folders(), 3, MEDIUM);
            let small = build_rows(&folders(), 5, SMALL);
            let find = |rows: &[Row], kind| {
                *rows
                    .iter()
                    .find(|r| r.kind == kind && r.section == 1)
                    .unwrap()
            };
            let pin = pin_at(&medium, find(&medium, RowKind::Header).top).unwrap();
            assert_eq!(
                pin,
                Pin {
                    offset: 5,
                    header: true,
                    into: 0.0
                }
            );
            assert_eq!(
                pin_top(&small, pin),
                Some(find(&small, RowKind::Header).top)
            );
            // The row under it is another place, a header's height further down.
            let under = Pin {
                header: false,
                ..pin
            };
            assert_eq!(
                pin_top(&small, under),
                Some(find(&small, RowKind::Tiles).top)
            );
        }

        #[test]
        fn the_gap_under_a_folder_is_the_end_of_its_last_row() {
            let medium = build_rows(&folders(), 3, MEDIUM);
            let last = *medium
                .iter()
                .rfind(|r| r.kind == RowKind::Tiles && r.section == 0)
                .unwrap();
            assert_eq!(
                pin_at(&medium, last.top + last.height + 10.0),
                Some(Pin {
                    offset: last.first,
                    header: false,
                    into: 1.0
                })
            );
        }

        #[test]
        fn a_pin_whose_photo_is_gone_has_nowhere_to_come_back_to() {
            let rows = build_rows(&folders(), 3, MEDIUM);
            let gone = Pin {
                offset: 99,
                header: false,
                into: 0.0,
            };
            assert_eq!(pin_top(&rows, gone), None);
        }
    }

    mod the_pinned_header {
        use super::*;

        // Two folders at two columns of medium tiles: header 0, rows at 32, 200, 368; then
        // the section gap, header at 560, rows at 592 and 760.
        fn rows() -> Vec<Row> {
            build_rows(&two_folders(), 2, MEDIUM)
        }
        const SECOND: f64 = 560.0;

        fn pinned(top: f64) -> Option<PinnedHeader> {
            let rows = rows();
            pinned_header(&rows, &header_rows(&rows), top)
        }
        fn at(section: usize, y: f64) -> Option<PinnedHeader> {
            Some(PinnedHeader { section, y })
        }

        #[test]
        fn the_header_rows_in_order() {
            let rows = rows();
            let tops: Vec<_> = header_rows(&rows)
                .iter()
                .map(|&i| (rows[i].section, rows[i].top))
                .collect();
            assert_eq!(tops, [(0, 0.0), (1, SECOND)]);
            assert!(header_rows(&build_rows(&[flat(0, 5)], 2, MEDIUM)).is_empty());
        }

        // The real header is there, exactly where the pinned one would be drawn.
        #[test]
        fn nothing_is_pinned_while_the_header_itself_is_at_the_top() {
            assert_eq!(pinned(0.0), None);
            assert_eq!(pinned(SECOND), None);
        }

        #[test]
        fn the_header_of_the_section_the_top_is_inside_is_pinned() {
            assert_eq!(pinned(1.0), at(0, 0.0));
            assert_eq!(pinned(300.0), at(0, 0.0));
            assert_eq!(pinned(SECOND + 1.0), at(1, 0.0));
            assert_eq!(pinned(900.0), at(1, 0.0));
        }

        // In the space between two folders it is still the folder above the eye is leaving.
        #[test]
        fn the_folder_above_stays_pinned_through_the_gap_under_its_last_row() {
            let last_row_end = SECOND - SECTION_GAP;
            assert_eq!(pinned(last_row_end + 1.0).unwrap().section, 0);
        }

        // The pinned header's bottom edge rides on the arriving header's top.
        #[test]
        fn the_next_header_pushes_it_up_as_it_arrives() {
            assert_eq!(pinned(SECOND - HEADER), at(0, 0.0));
            assert_eq!(pinned(SECOND - HEADER + 10.0), at(0, -10.0));
            assert_eq!(pinned(SECOND - 1.0), at(0, -(HEADER - 1.0)));
        }

        #[test]
        fn nothing_is_pinned_without_headers_or_without_rows() {
            let flat = build_rows(&[flat(0, 5), flat(5, 3)], 2, MEDIUM);
            assert_eq!(pinned_header(&flat, &header_rows(&flat), 300.0), None);
            assert_eq!(pinned_header(&[], &[], 0.0), None);
        }

        // A run with no header after one with: nothing above it is its header.
        #[test]
        fn nothing_is_pinned_over_a_run_that_has_no_header_of_its_own() {
            let mixed = build_rows(&[folder(1, 0, 5), flat(5, 3)], 2, MEDIUM);
            let headers = header_rows(&mixed);
            let run = mixed
                .iter()
                .find(|r| r.kind == RowKind::Tiles && r.section == 1)
                .unwrap();
            assert_eq!(pinned_header(&mixed, &headers, run.top + 10.0), None);
            assert_eq!(pinned_header(&mixed, &headers, 100.0), at(0, 0.0));
        }
    }
}
```

- [ ] **Step 3: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod layout;
    pub mod motion;
}
pub mod tasks;
pub mod theme {
    pub mod tokens;
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-ui --lib grid`
Expected: 41 tests pass (8 in `motion`, 33 in `layout`).

- [ ] **Step 5: Probes**

**`a_move_of_a_viewport_or_more_in_one_sample_is_a_jump`** - in `crates/photon-ui/src/grid/motion.rs` replace

```rust
            if moved.abs() >= viewport {
```

with

```rust
            if moved.abs() >= viewport * 1000.0 {
```

Run: `cargo test -p photon-ui --lib a_move_of_a_viewport_or_more_in_one_sample_is_a_jump`
Expected: FAIL.

**`the_peak_is_kept_while_a_flick_slows_and_starts_again_on_a_reversal`** - in `crates/photon-ui/src/grid/motion.rs` replace

```rust
                let peak = speed.max(same.map_or(0.0, |(_, peak)| peak));
```

with

```rust
                let peak = speed;
```

Run: `cargo test -p photon-ui --lib the_peak_is_kept_while_a_flick_slows_and_starts_again_on_a_reversal`
Expected: FAIL.

**`it_settles_once_the_samples_stop_and_says_when`** - in `crates/photon-ui/src/grid/motion.rs` replace

```rust
        if self.last_at.is_some_and(|at| now - at >= SCROLL_SETTLE_MS) {
```

with

```rust
        if self.last_at.is_some_and(|at| now - at >= SCROLL_SETTLE_MS * 100.0) {
```

Run: `cargo test -p photon-ui --lib it_settles_once_the_samples_stop_and_says_when`
Expected: FAIL.

**`a_tile_stops_an_eighth_past_the_thumbnail_it_draws`** - in `crates/photon-ui/src/grid/layout.rs` replace

```rust
    nominal.max(share.min(TILE_MAX))
```

with

```rust
    nominal.max(share)
```

Run: `cargo test -p photon-ui --lib a_tile_stops_an_eighth_past_the_thumbnail_it_draws`
Expected: FAIL.

**`it_comes_back_to_the_same_part_of_the_row_not_to_its_top`** - in `crates/photon-ui/src/grid/layout.rs` replace

```rust
        into: ((top - row.top) / row.height).clamp(0.0, 1.0),
```

with

```rust
        into: 0.0,
```

Run: `cargo test -p photon-ui --lib it_comes_back_to_the_same_part_of_the_row_not_to_its_top`
Expected: FAIL.

**`a_header_answers_with_the_section_it_heads_and_comes_back_to_it`** - in `crates/photon-ui/src/grid/layout.rs` replace

```rust
        Some(above) if pin.header && above.kind == RowKind::Header && above.first == pin.offset => {
```

with

```rust
        Some(above) if false && pin.header && above.kind == RowKind::Header && above.first == pin.offset => {
```

Run: `cargo test -p photon-ui --lib a_header_answers_with_the_section_it_heads_and_comes_back_to_it`
Expected: FAIL.

**`the_next_header_pushes_it_up_as_it_arrives`** - in `crates/photon-ui/src/grid/layout.rs` replace

```rust
        .map_or(0.0, |&next| (rows[next].top - top - HEADER).min(0.0));
```

with

```rust
        .map_or(0.0, |&next| (rows[next].top - top - HEADER).min(0.0) * 0.0);
```

Run: `cargo test -p photon-ui --lib the_next_header_pushes_it_up_as_it_arrives`
Expected: FAIL.

**`only_what_is_on_screen_for_a_jump`** - in `crates/photon-ui/src/grid/layout.rs` replace

```rust
        Motion::Jump { .. } => (0.0, 0.0),
```

with

```rust
        Motion::Jump { .. } => (viewport * RENDER_OVERSCAN, viewport * RENDER_OVERSCAN),
```

Run: `cargo test -p photon-ui --lib only_what_is_on_screen_for_a_jump`
Expected: FAIL.

**`a_section_is_a_header_row_and_its_tile_rows`** - in `crates/photon-ui/src/grid/layout.rs` replace

```rust
                top += SECTION_GAP;
```

with

```rust
                top += 0.0;
```

Run: `cargo test -p photon-ui --lib a_section_is_a_header_row_and_its_tile_rows`
Expected: FAIL.

**`thumbnails_wait_only_where_a_tile_will_be_gone_first`** - in `crates/photon-ui/src/grid/layout.rs` replace

```rust
        Motion::Jump { stream } => stream,
```

with

```rust
        Motion::Jump { .. } => false,
```

Run: `cargo test -p photon-ui --lib thumbnails_wait_only_where_a_tile_will_be_gone_first`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/grid crates/photon-ui/src/lib.rs
git commit -m "feat(ui): the grid's geometry and its motion, ported with their tests

layout.ts and scroll-speed in Rust, constant for constant, in f64: a large library is
taller than an f32 counts pixels in. What decided which tiles are mounted now decides
which thumbnails are wanted; what bridged a render and the effect after it is not ported,
there being no such gap in immediate mode.

Ten rules probed, each breaking the test named for it.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: The position, the scrollbar, and what is on screen

**Files:**
- Create: `crates/photon-ui/src/grid/scroll.rs`, `crates/photon-ui/src/grid/visible.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces, in `scroll`: `BAR_WIDTH: f64`, `MIN_THUMB: f64`; `Scroll` (`Default`, `Copy`) with `position() -> f64`, `max() -> f64`, `set_extent(total: f64, viewport: f64)`, `set(position: f64)`, `scroll_by(delta: f64)`, `thumb(track: f64) -> Option<(f64, f64)>` as `(start, length)`, `position_for_thumb(track: f64, start: f64) -> f64`.
- Produces, in `visible`: `VISIBLE_DEBOUNCE_MS: f64`; `VisibleReport` (`Default`) with `update(&mut self, on_screen: &[i64], now: f64) -> Option<&[i64]>` and `due_at(&self) -> Option<f64>`; times in milliseconds.

A caveat for later tasks: `Scroll::set` holds the position to the extent, and before the first `set_extent` the extent is nothing. A position set before the grid's first frame is lost.

- [ ] **Step 1: `crates/photon-ui/src/grid/scroll.rs`**

```rust
//! Where the grid is, and its scrollbar.
//!
//! The position is an `f64` in the layout's own coordinates (`layout::Row::top`), and the
//! view draws each visible row at `row.top - position`. The grid's rows are never put in a
//! scrolling container: egui's `ScrollArea` keeps its offset in an `f32`, which counts
//! pixels exactly only to 16,777,216, and a large library at large tiles is more than twice
//! that tall. So there is no scroll map here, as `ui/src/lib/scroll-map.ts` is for a
//! browser's 33,554,428px box: nothing has a limit to map around.

/// The room the scrollbar takes at the grid's right edge. Always taken, whether or not
/// there is anything to scroll: a bar that came with the overflow would narrow the tiles,
/// which shortens the grid, which takes the overflow away.
pub const BAR_WIDTH: f64 = 12.0;
/// The shortest the thumb is drawn, so there is something to take hold of in a library
/// thousands of viewports tall.
pub const MIN_THUMB: f64 = 32.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    position: f64,
    total: f64,
    viewport: f64,
}

impl Scroll {
    pub fn position(&self) -> f64 {
        self.position
    }

    /// The furthest the grid scrolls: the layout's last pixel at the viewport's bottom edge.
    pub fn max(&self) -> f64 {
        (self.total - self.viewport).max(0.0)
    }

    /// The layout's height and the viewport's, on every frame. A position past the new end
    /// is held to it.
    pub fn set_extent(&mut self, total: f64, viewport: f64) {
        self.total = total.max(0.0);
        self.viewport = viewport.max(0.0);
        self.position = self.position.clamp(0.0, self.max());
    }

    pub fn set(&mut self, position: f64) {
        // `clamp` panics on NaN bounds and passes a NaN value through; a NaN position would
        // draw nothing, for ever.
        self.position = if position.is_nan() {
            0.0
        } else {
            position.clamp(0.0, self.max())
        };
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.set(self.position + delta);
    }

    /// The thumb on a track `track` long, as `(start, length)`; `None` when the layout fits
    /// the viewport and there is nothing to scroll.
    pub fn thumb(&self, track: f64) -> Option<(f64, f64)> {
        let max = self.max();
        if max <= 0.0 || track <= 0.0 {
            return None;
        }
        let length = (track * self.viewport / self.total)
            .max(MIN_THUMB)
            .min(track);
        Some(((track - length) * (self.position / max), length))
    }

    /// The position a thumb whose top is at `start` stands for: the inverse of `thumb`,
    /// exact at both ends, so a thumb dragged to the bottom is the last row.
    pub fn position_for_thumb(&self, track: f64, start: f64) -> f64 {
        let Some((_, length)) = self.thumb(track) else {
            return 0.0;
        };
        let travel = track - length;
        if travel <= 0.0 {
            return 0.0;
        }
        self.max() * (start / travel).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taller than twice what an `f32` counts exactly, and than a browser's box.
    const TALL: f64 = 40_000_000.0;
    const VIEWPORT: f64 = 1000.0;

    fn tall() -> Scroll {
        let mut scroll = Scroll::default();
        scroll.set_extent(TALL, VIEWPORT);
        scroll
    }

    // `scroll-probe`'s successor: every pixel of a library this tall is a place the grid
    // can be. In an `f32` the step below 33,000,000 is 2 and above it 4.
    #[test]
    fn a_step_of_one_pixel_moves_one_pixel_anywhere_in_a_tall_library() {
        let mut scroll = tall();
        for from in [0.0, 16_777_216.0, 33_554_428.0, TALL - VIEWPORT - 1.0] {
            scroll.set(from);
            scroll.scroll_by(1.0);
            assert_eq!(scroll.position(), from + 1.0, "from {from}");
        }
    }

    #[test]
    fn the_end_of_a_tall_library_is_reachable_and_is_the_end() {
        let mut scroll = tall();
        scroll.set(f64::INFINITY);
        assert_eq!(scroll.position(), TALL - VIEWPORT);
        scroll.scroll_by(500.0);
        assert_eq!(scroll.position(), TALL - VIEWPORT);
        scroll.set(-5.0);
        assert_eq!(scroll.position(), 0.0);
        scroll.set(f64::NAN);
        assert_eq!(scroll.position(), 0.0);
    }

    #[test]
    fn a_layout_that_shrinks_holds_the_position_to_its_new_end() {
        let mut scroll = tall();
        scroll.set(30_000_000.0);
        scroll.set_extent(5000.0, VIEWPORT);
        assert_eq!(scroll.position(), 4000.0);
        // And one that fits the viewport has nowhere to be but the top.
        scroll.set_extent(800.0, VIEWPORT);
        assert_eq!(scroll.position(), 0.0);
        assert_eq!(scroll.max(), 0.0);
    }

    #[test]
    fn the_thumb_is_at_the_tracks_ends_at_the_layouts_ends() {
        let mut scroll = tall();
        let track = 900.0;
        // 900 * 1000 / 40,000,000 is far under the minimum.
        assert_eq!(scroll.thumb(track), Some((0.0, MIN_THUMB)));
        scroll.set(f64::INFINITY);
        assert_eq!(scroll.thumb(track), Some((track - MIN_THUMB, MIN_THUMB)));
    }

    #[test]
    fn a_thumb_dragged_to_an_end_is_that_end_exactly() {
        let scroll = tall();
        let track = 900.0;
        assert_eq!(scroll.position_for_thumb(track, 0.0), 0.0);
        assert_eq!(
            scroll.position_for_thumb(track, track - MIN_THUMB),
            TALL - VIEWPORT
        );
        // Past either end of the track is the end, not beyond it.
        assert_eq!(scroll.position_for_thumb(track, 5000.0), TALL - VIEWPORT);
        assert_eq!(scroll.position_for_thumb(track, -40.0), 0.0);
    }

    #[test]
    fn the_thumb_and_the_position_are_each_others_inverse() {
        let mut scroll = Scroll::default();
        scroll.set_extent(10_000.0, VIEWPORT);
        let track = 1000.0;
        // A tenth of the layout is in view: the thumb is a tenth of the track.
        assert_eq!(scroll.thumb(track), Some((0.0, 100.0)));
        for position in [0.0, 1234.0, 4500.0, 9000.0] {
            scroll.set(position);
            let (start, _) = scroll.thumb(track).unwrap();
            assert!((scroll.position_for_thumb(track, start) - position).abs() < 1e-6);
        }
    }

    #[test]
    fn there_is_no_thumb_when_everything_fits() {
        let mut scroll = Scroll::default();
        scroll.set_extent(500.0, VIEWPORT);
        assert_eq!(scroll.thumb(900.0), None);
        assert_eq!(scroll.position_for_thumb(900.0, 300.0), 0.0);
        assert_eq!(Scroll::default().thumb(900.0), None);
    }
}
```

- [ ] **Step 2: `crates/photon-ui/src/grid/visible.rs`**

```rust
//! Telling the engine's thumbnail queue what is on screen, once scrolling settles.
//!
//! The queue renders what is visible first (`ThumbService::set_visible`). Telling it on
//! every frame of a scroll would reorder it sixty times a second for rows nobody stops on,
//! so the photos in view are reported only once they have been the same for
//! `VISIBLE_DEBOUNCE_MS`, as the Svelte grid's timer did.

/// How long the photos in view must stay the same before they are reported.
pub const VISIBLE_DEBOUNCE_MS: f64 = 150.0;

#[derive(Debug, Default)]
pub struct VisibleReport {
    /// What is in view now, and since when.
    seen: Vec<i64>,
    since: f64,
    /// What the engine was last told.
    reported: Vec<i64>,
}

impl VisibleReport {
    /// The photos in view this frame, at `now` (milliseconds). Answers them when they are
    /// due to be reported: unchanged for the debounce, and not what was reported last.
    pub fn update(&mut self, on_screen: &[i64], now: f64) -> Option<&[i64]> {
        if self.seen != on_screen {
            self.seen = on_screen.to_vec();
            self.since = now;
        }
        if self.seen != self.reported && now - self.since >= VISIBLE_DEBOUNCE_MS {
            self.reported = self.seen.clone();
            return Some(&self.reported);
        }
        None
    }

    /// When `update` will next have something to report, if nothing changes: the frame to
    /// ask for, since a still grid draws no frame by itself.
    pub fn due_at(&self) -> Option<f64> {
        (self.seen != self.reported).then_some(self.since + VISIBLE_DEBOUNCE_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_in_view_is_reported_once_it_has_stayed_the_same() {
        let mut report = VisibleReport::default();
        assert_eq!(report.update(&[1, 2, 3], 1000.0), None);
        assert_eq!(report.due_at(), Some(1000.0 + VISIBLE_DEBOUNCE_MS));
        assert_eq!(
            report.update(&[1, 2, 3], 1000.0 + VISIBLE_DEBOUNCE_MS - 1.0),
            None
        );
        assert_eq!(
            report.update(&[1, 2, 3], 1000.0 + VISIBLE_DEBOUNCE_MS),
            Some(&[1, 2, 3][..])
        );
        // And once only.
        assert_eq!(report.update(&[1, 2, 3], 5000.0), None);
        assert_eq!(report.due_at(), None);
    }

    #[test]
    fn a_scroll_reports_nothing_until_it_stops() {
        let mut report = VisibleReport::default();
        let mut now = 0.0;
        for first in 0..100 {
            assert_eq!(report.update(&[first, first + 1], now), None);
            now += 16.0;
        }
        assert_eq!(
            report.update(&[99, 100], now + VISIBLE_DEBOUNCE_MS),
            Some(&[99, 100][..])
        );
    }

    #[test]
    fn coming_back_to_what_was_reported_reports_nothing() {
        let mut report = VisibleReport::default();
        report.update(&[1], 0.0);
        assert!(report.update(&[1], 200.0).is_some());
        report.update(&[2], 300.0);
        // Back before the other view was ever reported.
        assert_eq!(report.update(&[1], 320.0), None);
        assert_eq!(report.update(&[1], 900.0), None);
        assert_eq!(report.due_at(), None);
    }

    #[test]
    fn an_empty_grid_has_nothing_to_report() {
        let mut report = VisibleReport::default();
        assert_eq!(report.update(&[], 0.0), None);
        assert_eq!(report.update(&[], 1000.0), None);
        assert_eq!(report.due_at(), None);
    }
}
```

- [ ] **Step 3: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod visible;
}
pub mod tasks;
pub mod theme {
    pub mod tokens;
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-ui --lib grid::scroll && cargo test -p photon-ui --lib grid::visible`
Expected: 7 and 4 tests pass.

- [ ] **Step 5: Probes**

**`a_step_of_one_pixel_moves_one_pixel_anywhere_in_a_tall_library`** - in `crates/photon-ui/src/grid/scroll.rs` replace

```rust
            position.clamp(0.0, self.max())
        };
```

with

```rust
            f64::from(position.clamp(0.0, self.max()) as f32)
        };
```

Run: `cargo test -p photon-ui --lib a_step_of_one_pixel_moves_one_pixel_anywhere_in_a_tall_library`
Expected: FAIL.

**`the_thumb_is_at_the_tracks_ends_at_the_layouts_ends`** - in `crates/photon-ui/src/grid/scroll.rs` replace

```rust
            .max(MIN_THUMB)
```

with

```rust
            .max(0.0)
```

Run: `cargo test -p photon-ui --lib the_thumb_is_at_the_tracks_ends_at_the_layouts_ends`
Expected: FAIL.

**`a_layout_that_shrinks_holds_the_position_to_its_new_end`** - in `crates/photon-ui/src/grid/scroll.rs` replace

```rust
        self.position = self.position.clamp(0.0, self.max());
```

with

```rust
        self.position = self.position.max(0.0);
```

Run: `cargo test -p photon-ui --lib a_layout_that_shrinks_holds_the_position_to_its_new_end`
Expected: FAIL.

**`a_scroll_reports_nothing_until_it_stops`** - in `crates/photon-ui/src/grid/visible.rs` replace

```rust
        if self.seen != self.reported && now - self.since >= VISIBLE_DEBOUNCE_MS {
```

with

```rust
        if self.seen != self.reported && now - self.since >= 0.0 {
```

Run: `cargo test -p photon-ui --lib a_scroll_reports_nothing_until_it_stops`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

`scroll f32` is the probe that says why there is no scroll map: with the position held to an `f32`'s precision, a one-pixel step deep in a tall library goes nowhere.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/grid/scroll.rs crates/photon-ui/src/grid/visible.rs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): the grid's position as an f64, its scrollbar, and the visible report

The grid's rows are never put in a scrolling container: egui's keeps its offset in an f32,
exact to 16,777,216, and a library can be twice that tall. So there is no scroll map to
port; the position is the layout's own, and every pixel of a 40,000,000px library is a
place it can be. The photos in view are reported once they have stayed the same for 150ms.

Probed: the position at f32 precision, the thumb's minimum, the hold to a shrunken
layout, and the debounce.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: What the grid writes

**Files:**
- Create: `crates/photon-ui/src/grid/labels.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: `photon_core::grid::Period { year: i64, month: Option<u32>, day: Option<u32> }`, `photon_core::library::Folder`.
- Produces: `grouped(usize) -> String`, `photo_count(usize) -> String`, `folder_label(&Folder) -> &str`, `folder_summary(count: usize, taken_at_min: i64, zone: &jiff::tz::TimeZone) -> String`, `period_label(Period) -> String`, `format_duration(ms: i64) -> String`.

**A known difference from the Svelte UI, to report in the pull request:** dates are written in English ("July 2026", "Saturday, June 15, 2024"). The browser wrote them in the system's locale. photon takes no ICU; what to do about locales is the decision of the sub-project that brings the sidebar.

- [ ] **Step 1: `crates/photon-ui/src/grid/labels.rs`**

```rust
//! What the grid writes: a header's name, count and month, and a video's running time.
//!
//! Dates are written in English. The Svelte UI wrote them in the system's locale, through
//! the browser; photon takes no ICU, so a locale is a decision for the sub-project that
//! brings the sidebar, which reads the same instants.

use jiff::{Timestamp, civil::Weekday, tz::TimeZone};
use photon_core::{grid::Period, library::Folder};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn month_name(month: i64) -> &'static str {
    MONTHS[(month.clamp(1, 12) - 1) as usize]
}

/// `1234567` as `1,234,567`.
pub fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn photo_count(count: usize) -> String {
    if count == 1 {
        "1 photo".to_owned()
    } else {
        format!("{} photos", grouped(count))
    }
}

/// What photon calls a folder: the user's alias, else its directory name.
pub fn folder_label(folder: &Folder) -> &str {
    folder.alias.as_deref().unwrap_or(&folder.name)
}

/// What a folder's header says after its name: how many of its photos the view holds, and
/// the month its oldest one was taken - "23 photos · July 2026".
///
/// The oldest, not a range to the newest: a photo with no date of its own is dated by its
/// file, so one scan copied over yesterday would stretch a folder from 1998 "to" this
/// month. The month is read in `zone`, the viewer's own.
pub fn folder_summary(count: usize, taken_at_min: i64, zone: &TimeZone) -> String {
    let count = photo_count(count);
    match Timestamp::from_second(taken_at_min) {
        Ok(instant) => {
            let local = instant.to_zoned(zone.clone());
            format!(
                "{count} · {} {}",
                month_name(i64::from(local.month())),
                local.year()
            )
        }
        // A date no calendar holds: the count alone.
        Err(_) => count,
    }
}

/// A period's header: "2024", "June 2024" or "Saturday, June 15, 2024". From the section's
/// own numbers, never from an instant, which would be read again in the viewer's zone.
pub fn period_label(period: Period) -> String {
    let Some(month) = period.month else {
        return period.year.to_string();
    };
    let month_and_year = |day: Option<u32>| match day {
        Some(day) => format!("{} {day}, {}", month_name(i64::from(month)), period.year),
        None => format!("{} {}", month_name(i64::from(month)), period.year),
    };
    let Some(day) = period.day else {
        return month_and_year(None);
    };
    let weekday = i16::try_from(period.year)
        .ok()
        .zip(i8::try_from(month).ok())
        .zip(i8::try_from(day).ok())
        .and_then(|((year, month), day)| jiff::civil::Date::new(year, month, day).ok())
        .map(|date| match date.weekday() {
            Weekday::Monday => "Monday",
            Weekday::Tuesday => "Tuesday",
            Weekday::Wednesday => "Wednesday",
            Weekday::Thursday => "Thursday",
            Weekday::Friday => "Friday",
            Weekday::Saturday => "Saturday",
            Weekday::Sunday => "Sunday",
        });
    match weekday {
        Some(weekday) => format!("{weekday}, {}", month_and_year(Some(day))),
        None => month_and_year(Some(day)),
    }
}

/// A video's running time: "0:07", "12:34", "1:02:03".
pub fn format_duration(ms: i64) -> String {
    let total = ms.max(0) / 1000;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_is_grouped_in_threes() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(300_000), "300,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(photo_count(1), "1 photo");
        assert_eq!(photo_count(0), "0 photos");
        assert_eq!(photo_count(23), "23 photos");
        assert_eq!(photo_count(12_345), "12,345 photos");
    }

    #[test]
    fn a_folder_is_called_by_its_alias_when_it_has_one() {
        let mut folder = Folder {
            id: 1,
            watched_id: 1,
            parent_id: None,
            path: "/photos/2024-06".to_owned(),
            name: "2024-06".to_owned(),
            hidden: false,
            alias: None,
        };
        assert_eq!(folder_label(&folder), "2024-06");
        folder.alias = Some("Summer".to_owned());
        assert_eq!(folder_label(&folder), "Summer");
    }

    // 2026-07-01 00:30 UTC: July in UTC, still June five hours west.
    const JULY_FIRST: i64 = 1_782_865_800;

    #[test]
    fn a_folders_month_is_read_in_the_viewers_zone() {
        assert_eq!(
            folder_summary(23, JULY_FIRST, &TimeZone::UTC),
            "23 photos · July 2026"
        );
        let west = TimeZone::fixed(jiff::tz::offset(-5));
        assert_eq!(folder_summary(1, JULY_FIRST, &west), "1 photo · June 2026");
        assert_eq!(folder_summary(5, i64::MAX, &TimeZone::UTC), "5 photos");
    }

    #[test]
    fn a_period_is_named_by_its_own_numbers() {
        let period = |month, day| Period {
            year: 2024,
            month,
            day,
        };
        assert_eq!(period_label(period(None, None)), "2024");
        assert_eq!(period_label(period(Some(6), None)), "June 2024");
        assert_eq!(
            period_label(period(Some(6), Some(15))),
            "Saturday, June 15, 2024"
        );
        // A day no month has is written without a weekday, not refused.
        assert_eq!(period_label(period(Some(2), Some(31))), "February 31, 2024");
    }

    #[test]
    fn a_running_time_is_minutes_and_seconds_and_hours_when_it_has_them() {
        assert_eq!(format_duration(7_900), "0:07");
        assert_eq!(format_duration(754_000), "12:34");
        assert_eq!(format_duration(3_723_000), "1:02:03");
        assert_eq!(format_duration(-5), "0:00");
    }
}
```

- [ ] **Step 2: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod visible;
}
pub mod tasks;
pub mod theme {
    pub mod tokens;
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-ui --lib labels`
Expected: 5 tests pass.

- [ ] **Step 4: Probe**

**`a_folders_month_is_read_in_the_viewers_zone`** - in `crates/photon-ui/src/grid/labels.rs` replace

```rust
            let local = instant.to_zoned(zone.clone());
```

with

```rust
            let local = instant.to_zoned(TimeZone::UTC);
```

Run: `cargo test -p photon-ui --lib a_folders_month_is_read_in_the_viewers_zone`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/grid/labels.rs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): a header's count and month, a period's name, a video's running time

The month a folder's header names is read in the viewer's zone, as the sidebar's year
will be; a period is named from its own numbers. Dates are in English: the browser wrote
them in the system's locale, and a locale without ICU is a later decision.

Probed with the zone ignored.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Which textures are held

**Files:**
- Create: `crates/photon-ui/src/thumbs/textures.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: `photon_core::thumbs::ThumbSize`.
- Produces: `TexKey { pub key: u64, pub size: ThumbSize }`, `TexKey::grid(key: u64)`; `Pixels { pub width: u32, pub height: u32, pub rgba: Vec<u8> }`; `DEFAULT_LIMIT: usize`, `UPLOADS_PER_FRAME: usize`, `RETRY_SECS: f64`; `Textures<T>` with `new(limit: usize)`, `begin_frame()`, `get(key) -> Option<&T>`, `fail(key)`, `failed(key) -> bool`, `put_off(key, now: f64)`, `offer(key, Pixels, wanted: &HashSet<TexKey>) -> bool`, `missing(&mut self, wanted: &[TexKey], now: f64) -> Vec<TexKey>`, `upload(budget: usize, wanted: &HashSet<TexKey>, make: impl FnMut(TexKey, &Pixels) -> T) -> bool`, `bytes()`, `len()`, `is_empty()`. Times in seconds.

- [ ] **Step 1: `crates/photon-ui/src/thumbs/textures.rs`**

```rust
//! The thumbnails held as textures: which ones, how many bytes, what to upload this frame
//! and what to let go. Bookkeeping only - the texture itself is whatever `T` the caller
//! makes, so none of this names egui and all of it is tested without a GPU.

use photon_core::thumbs::ThumbSize;
use std::collections::{HashMap, HashSet, VecDeque};

/// What a texture is of. A thumbnail key names one picture (`Item::thumb_key`), and the
/// cache holds it at more than one size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexKey {
    pub key: u64,
    pub size: ThumbSize,
}

impl TexKey {
    pub fn grid(key: u64) -> Self {
        Self {
            key,
            size: ThumbSize::Grid,
        }
    }
}

/// A decoded picture on its way to the GPU: straight (not premultiplied) RGBA, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Bytes of texture kept before the least recently drawn are let go. A grid thumbnail is at
/// most 256x256, so 262,144 bytes: this is about a thousand of them.
pub const DEFAULT_LIMIT: usize = 256 << 20;
/// Textures uploaded in one frame. An upload is a copy to the GPU on the UI thread, and a
/// jump to a new place brings a screenful at once.
pub const UPLOADS_PER_FRAME: usize = 32;
/// How long a thumbnail that was not there to be had is left alone before it is asked for
/// again, in seconds. Without it a photo whose thumbnail cannot be built in time - or that
/// is gone - would be asked for again on every frame it is in view.
pub const RETRY_SECS: f64 = 5.0;

struct Held<T> {
    texture: T,
    bytes: usize,
    /// The frame it was last drawn in.
    drawn: u64,
}

pub struct Textures<T> {
    held: HashMap<TexKey, Held<T>>,
    /// Decoded and not uploaded yet, oldest first.
    waiting: VecDeque<(TexKey, Pixels)>,
    waiting_keys: HashSet<TexKey>,
    /// Pictures that could not be made. Remembered so the tile draws its mark and nothing
    /// asks again on every frame.
    failed: HashSet<TexKey>,
    /// Thumbnails that were unavailable, and when each may be asked for again.
    put_off: HashMap<TexKey, f64>,
    bytes: usize,
    limit: usize,
    frame: u64,
}

impl<T> Textures<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            held: HashMap::new(),
            waiting: VecDeque::new(),
            waiting_keys: HashSet::new(),
            failed: HashSet::new(),
            put_off: HashMap::new(),
            bytes: 0,
            limit,
            frame: 0,
        }
    }

    /// Once per frame, before anything is drawn.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The texture for `key`, counted as drawn this frame.
    pub fn get(&mut self, key: TexKey) -> Option<&T> {
        let held = self.held.get_mut(&key)?;
        held.drawn = self.frame;
        Some(&held.texture)
    }

    pub fn fail(&mut self, key: TexKey) {
        self.failed.insert(key);
    }

    pub fn failed(&self, key: TexKey) -> bool {
        self.failed.contains(&key)
    }

    /// The thumbnail was not there to be had at `now` (seconds): not built in time, or its
    /// photo gone. It is asked for again once `RETRY_SECS` have passed.
    pub fn put_off(&mut self, key: TexKey, now: f64) {
        self.put_off.insert(key, now + RETRY_SECS);
    }

    /// A decoded thumbnail, to be uploaded. Dropped - `false` - when it is no longer
    /// wanted or is here already: a scroll has moved on, or the photo has another key by
    /// now (an edit, a rewritten file) and this is a picture nothing draws.
    pub fn offer(&mut self, key: TexKey, pixels: Pixels, wanted: &HashSet<TexKey>) -> bool {
        if !wanted.contains(&key)
            || self.held.contains_key(&key)
            || self.waiting_keys.contains(&key)
        {
            return false;
        }
        self.waiting_keys.insert(key);
        self.waiting.push_back((key, pixels));
        true
    }

    /// The keys of `wanted`, in its order, that have neither a texture, nor pixels waiting
    /// for upload, nor a failure, and are not put off past `now`: what to ask the loader for.
    pub fn missing(&mut self, wanted: &[TexKey], now: f64) -> Vec<TexKey> {
        self.put_off.retain(|_, until| *until > now);
        wanted
            .iter()
            .copied()
            .filter(|key| {
                !self.held.contains_key(key)
                    && !self.waiting_keys.contains(key)
                    && !self.failed.contains(key)
                    && !self.put_off.contains_key(key)
            })
            .collect()
    }

    /// Uploads up to `budget` waiting thumbnails through `make`, then lets go of the least
    /// recently drawn textures until the limit holds. A texture in `wanted` is never let
    /// go, so a view that needs more than the limit keeps what it shows. `true` when
    /// thumbnails are still waiting, and another frame is owed.
    pub fn upload(
        &mut self,
        budget: usize,
        wanted: &HashSet<TexKey>,
        mut make: impl FnMut(TexKey, &Pixels) -> T,
    ) -> bool {
        let mut uploaded = 0;
        while uploaded < budget {
            let Some((key, pixels)) = self.waiting.pop_front() else {
                break;
            };
            self.waiting_keys.remove(&key);
            // Decoded for a place the grid has left since: not worth an upload.
            if !wanted.contains(&key) {
                continue;
            }
            let bytes = pixels.rgba.len();
            self.held.insert(
                key,
                Held {
                    texture: make(key, &pixels),
                    bytes,
                    drawn: self.frame,
                },
            );
            self.bytes += bytes;
            uploaded += 1;
        }
        while self.bytes > self.limit {
            let oldest = self
                .held
                .iter()
                .filter(|(key, _)| !wanted.contains(key))
                .min_by_key(|(_, held)| held.drawn)
                .map(|(key, _)| *key);
            let Some(key) = oldest else {
                break;
            };
            if let Some(held) = self.held.remove(&key) {
                self.bytes -= held.bytes;
            }
        }
        !self.waiting.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x2 picture: 16 bytes.
    fn pixels() -> Pixels {
        Pixels {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        }
    }

    fn keys(range: std::ops::Range<u64>) -> Vec<TexKey> {
        range.map(TexKey::grid).collect()
    }

    fn set(keys: &[TexKey]) -> HashSet<TexKey> {
        keys.iter().copied().collect()
    }

    /// Offers and uploads every key of `wanted`, as a frame with a large budget does.
    fn load(textures: &mut Textures<u64>, wanted: &[TexKey]) {
        let wanted_set = set(wanted);
        for key in wanted {
            textures.offer(*key, pixels(), &wanted_set);
        }
        textures.upload(usize::MAX, &wanted_set, |key, _| key.key);
    }

    #[test]
    fn an_uploaded_thumbnail_is_there_to_draw() {
        let mut textures = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..3);
        assert_eq!(textures.missing(&wanted, 0.0), wanted);
        load(&mut textures, &wanted);
        assert_eq!(textures.get(TexKey::grid(1)), Some(&1));
        assert_eq!(textures.get(TexKey::grid(7)), None);
        assert_eq!((textures.len(), textures.bytes()), (3, 48));
        assert!(textures.missing(&wanted, 0.0).is_empty());
    }

    #[test]
    fn no_more_than_the_budget_is_uploaded_in_a_frame() {
        let mut textures = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..5);
        let wanted_set = set(&wanted);
        for key in &wanted {
            assert!(textures.offer(*key, pixels(), &wanted_set));
        }
        let mut made = Vec::new();
        let more = textures.upload(2, &wanted_set, |key, _| made.push(key.key));
        assert_eq!(made, [0, 1], "oldest first");
        assert!(more, "three are still waiting");
        // Waiting for upload is not missing: the loader is not asked for it again.
        assert!(textures.missing(&wanted, 0.0).is_empty());
        assert!(!textures.upload(usize::MAX, &wanted_set, |key, _| made.push(key.key)));
        assert_eq!(made, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn the_least_recently_drawn_go_first_once_over_the_limit() {
        // Room for three 16-byte textures.
        let mut textures = Textures::new(48);
        load(&mut textures, &keys(0..3));
        textures.begin_frame();
        // 0 and 2 are drawn again; 1 is not.
        textures.get(TexKey::grid(0));
        textures.get(TexKey::grid(2));
        textures.begin_frame();
        load(&mut textures, &keys(3..4));
        assert_eq!(
            textures.get(TexKey::grid(1)),
            None,
            "the one not drawn went"
        );
        for kept in [0, 2, 3] {
            assert_eq!(textures.get(TexKey::grid(kept)), Some(&kept));
        }
        assert_eq!(textures.bytes(), 48);
    }

    // A view that needs more than the limit - small tiles on a large screen - keeps what
    // it shows: letting go of a texture on screen would blank a tile to save memory.
    #[test]
    fn a_wanted_texture_is_never_let_go() {
        let mut textures = Textures::new(32);
        let wanted = keys(0..5);
        load(&mut textures, &wanted);
        assert_eq!(textures.len(), 5);
        assert_eq!(textures.bytes(), 80, "over the limit, and all of it wanted");
        // Once the view moves on, the limit holds again.
        load(&mut textures, &keys(5..6));
        assert!(textures.bytes() <= 32);
        assert_eq!(textures.get(TexKey::grid(5)), Some(&5));
    }

    // A result that comes back after the scroll has moved on, or under a key the photo no
    // longer has, is a picture nothing will draw.
    #[test]
    fn a_thumbnail_no_longer_wanted_is_dropped() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let now_wanted = set(&keys(10..12));
        assert!(!textures.offer(TexKey::grid(3), pixels(), &now_wanted));
        // Wanted when it was decoded, not by the frame that would upload it.
        assert!(textures.offer(TexKey::grid(10), pixels(), &now_wanted));
        let moved_on = set(&keys(20..22));
        assert!(!textures.upload(usize::MAX, &moved_on, |key, _| key.key));
        assert!(textures.is_empty());
        assert_eq!(
            textures.missing(&keys(10..11), 0.0),
            keys(10..11),
            "and may be asked for again"
        );
    }

    #[test]
    fn a_thumbnail_already_here_is_not_taken_twice() {
        let mut textures = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..1);
        let wanted_set = set(&wanted);
        assert!(textures.offer(wanted[0], pixels(), &wanted_set));
        assert!(
            !textures.offer(wanted[0], pixels(), &wanted_set),
            "already waiting"
        );
        textures.upload(usize::MAX, &wanted_set, |key, _| key.key);
        assert!(
            !textures.offer(wanted[0], pixels(), &wanted_set),
            "already held"
        );
        assert_eq!(textures.bytes(), 16);
    }

    #[test]
    fn a_thumbnail_that_was_unavailable_is_left_alone_for_a_while() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..2);
        textures.put_off(wanted[0], 100.0);
        assert_eq!(textures.missing(&wanted, 100.0), keys(1..2));
        assert_eq!(
            textures.missing(&wanted, 100.0 + RETRY_SECS - 0.1),
            keys(1..2)
        );
        assert_eq!(textures.missing(&wanted, 100.0 + RETRY_SECS), wanted);
        // And it is not a failure: nothing draws a mark for it.
        assert!(!textures.failed(wanted[0]));
    }

    #[test]
    fn a_failure_is_remembered_and_not_asked_for_again() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..2);
        textures.fail(wanted[0]);
        assert!(textures.failed(wanted[0]));
        assert!(!textures.failed(wanted[1]));
        assert_eq!(textures.missing(&wanted, 0.0), keys(1..2));
    }
}
```

- [ ] **Step 2: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod visible;
}
pub mod tasks;
pub mod theme {
    pub mod tokens;
}
pub mod thumbs {
    pub mod textures;
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-ui --lib textures`
Expected: 8 tests pass.

- [ ] **Step 4: Probes**

**`a_thumbnail_no_longer_wanted_is_dropped`** - in `crates/photon-ui/src/thumbs/textures.rs` replace

```rust
        if !wanted.contains(&key)
            || self.held.contains_key(&key)
```

with

```rust
        if self.held.contains_key(&key)
```

Run: `cargo test -p photon-ui --lib a_thumbnail_no_longer_wanted_is_dropped`
Expected: FAIL.

**`a_wanted_texture_is_never_let_go`** - in `crates/photon-ui/src/thumbs/textures.rs` replace

```rust
                .filter(|(key, _)| !wanted.contains(key))
```

with

```rust
                .filter(|_| true)
```

Run: `cargo test -p photon-ui --lib a_wanted_texture_is_never_let_go`
Expected: FAIL.

**`no_more_than_the_budget_is_uploaded_in_a_frame`** - in `crates/photon-ui/src/thumbs/textures.rs` replace

```rust
        while uploaded < budget {
```

with

```rust
        while uploaded < usize::MAX {
```

Run: `cargo test -p photon-ui --lib no_more_than_the_budget_is_uploaded_in_a_frame`
Expected: FAIL.

**`a_thumbnail_that_was_unavailable_is_left_alone_for_a_while`** - in `crates/photon-ui/src/thumbs/textures.rs` replace

```rust
                    && !self.put_off.contains_key(key)
```

with

```rust
                    && (true || !self.put_off.contains_key(key))
```

Run: `cargo test -p photon-ui --lib a_thumbnail_that_was_unavailable_is_left_alone_for_a_while`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/thumbs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): the bookkeeping of thumbnail textures, with no GPU in it

Which pictures are held, within how many bytes, which are uploaded this frame and which
are let go: least recently drawn first, and never one the view wants, so small tiles on a
large screen keep what they show. A thumbnail that was not to be had is left alone for
five seconds, not asked for on every frame.

Probed: an unwanted picture kept, a wanted one let go, the budget, and the retry delay.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Reading, waiting and decoding

**Files:**
- Create: `crates/photon-ui/src/thumbs/loader.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: `textures::Pixels`.
- Produces: `Want { pub id: i64, pub key: u64 }`; `LoadError { Failed(String), Unavailable }`; `Loaded { pub key: u64, pub result: Result<Pixels, LoadError> }`; `Building<'a> = Pin<Box<dyn Future<Output = Result<Pixels, LoadError>> + Send + 'a>>`; the trait `ThumbSource: Send + Sync + 'static` with `cached(&self, key: u64) -> Option<Pixels>` and `build(&self, id: i64) -> Building<'_>`; `Loader` with `Loader::spawn<S: ThumbSource>(source: Arc<S>, decoders: usize, timeout: Duration, notify: impl Fn() + Send + Sync + 'static) -> Self`, `want(&self, wanted: Vec<Want>)`, `poll(&self) -> Vec<Loaded>`.

The design to keep in mind while reading: **a decoder never waits**. A miss is handed to the one waiter thread, which polls every pending build as a future. `protocol.rs` records what happens otherwise.

- [ ] **Step 1: `crates/photon-ui/src/thumbs/loader.rs`**

```rust
//! Thumbnails on their way from the cache to the UI thread: read and decoded on a small
//! pool, or waited for when they have not been built yet.
//!
//! **A thumbnail not built yet is waited for, never blocked on.** Every such wait is a
//! future, and one thread polls them all. `protocol.rs` records why: when each waiting
//! request held a thread for up to `THUMB_TIMEOUT`, a fast scroll through a fresh import
//! parked hundreds of them, and thumbnails that *were* cached queued behind. A decoder here
//! never waits: a miss is handed to the waiter and the decoder takes the next thumbnail.

use super::textures::Pixels;
use parking_lot::{Condvar, Mutex};
use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};

/// A photo whose thumbnail the grid wants: the photo, to have it built, and the key its
/// picture is cached under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Want {
    pub id: i64,
    pub key: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// It cannot be made: the file is unreadable or not a picture. Asking again will not help.
    Failed(String),
    /// It was not built in time, or the photo is gone. Worth asking again when it is next
    /// wanted.
    Unavailable,
}

#[derive(Debug)]
pub struct Loaded {
    pub key: u64,
    pub result: Result<Pixels, LoadError>,
}

pub type Building<'a> = Pin<Box<dyn Future<Output = Result<Pixels, LoadError>> + Send + 'a>>;

/// Where thumbnails come from. The engine in the application; a fake in the tests, which
/// is the reason this is a trait.
pub trait ThumbSource: Send + Sync + 'static {
    /// The thumbnail cached under `key`, decoded; `None` when none is cached. Never waits
    /// for one to be built.
    fn cached(&self, key: u64) -> Option<Pixels>;
    /// The photo's thumbnail, once it has been built. Dropping the future gives up the wait.
    fn build(&self, id: i64) -> Building<'_>;
}

struct State {
    /// What the grid wants now, most wanted first. A decoder takes the first one nobody
    /// is working on.
    wanted: Vec<Want>,
    /// The keys of the latest `want`, kept after a decoder has taken its entry from
    /// `wanted`: what the waiter asks to know whether a wait is still worth holding.
    keys: HashSet<u64>,
    /// Keys being decoded or waited for.
    busy: HashSet<u64>,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn finish(&self, loaded: &Sender<Loaded>, key: u64, result: Result<Pixels, LoadError>) {
        self.state.lock().busy.remove(&key);
        if loaded.send(Loaded { key, result }).is_ok() {
            (self.notify)();
        }
    }
}

pub struct Loader {
    shared: Arc<Shared>,
    loaded: Receiver<Loaded>,
    waiter: Thread,
}

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

impl Loader {
    /// Starts `decoders` decoding threads and the one waiting thread. `timeout` bounds a
    /// wait for a thumbnail to be built; `notify` is called from a worker thread after each
    /// result is ready.
    pub fn spawn<S: ThumbSource>(
        source: Arc<S>,
        decoders: usize,
        timeout: Duration,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                wanted: Vec::new(),
                keys: HashSet::new(),
                busy: HashSet::new(),
                closed: false,
            }),
            work: Condvar::new(),
            notify: Box::new(notify),
        });
        let (loaded_tx, loaded) = mpsc::channel();
        let (misses_tx, misses) = mpsc::channel::<Want>();

        let waiter = {
            let (shared, source, loaded_tx) = (shared.clone(), source.clone(), loaded_tx.clone());
            thread::Builder::new()
                .name("thumb-wait".to_owned())
                .spawn(move || wait_for_builds(&shared, &*source, &misses, &loaded_tx, timeout))
                .expect("the thumbnail waiter could not be started")
                .thread()
                .clone()
        };
        for n in 0..decoders.max(1) {
            let (shared, source, loaded_tx, misses_tx, waiter) = (
                shared.clone(),
                source.clone(),
                loaded_tx.clone(),
                misses_tx.clone(),
                waiter.clone(),
            );
            thread::Builder::new()
                .name(format!("thumb-decode-{n}"))
                .spawn(move || decode(&shared, &*source, &loaded_tx, &misses_tx, &waiter))
                .expect("a thumbnail decoder could not be started");
        }
        Self {
            shared,
            loaded,
            waiter,
        }
    }

    /// What the grid wants now, most wanted first, replacing what it wanted before. A
    /// thumbnail being waited for that is not in the list is given up.
    pub fn want(&self, wanted: Vec<Want>) {
        {
            let mut state = self.shared.state.lock();
            state.keys = wanted.iter().map(|want| want.key).collect();
            state.wanted = wanted;
        }
        self.shared.work.notify_all();
        self.waiter.unpark();
    }

    /// Every result ready now.
    pub fn poll(&self) -> Vec<Loaded> {
        self.loaded.try_iter().collect()
    }
}

/// The threads end at their next wait. They are not joined: a decode in flight is a tenth
/// of a millisecond, and a wait is given up by being dropped.
impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.state.lock().closed = true;
        self.shared.work.notify_all();
        self.waiter.unpark();
    }
}

fn decode<S: ThumbSource>(
    shared: &Shared,
    source: &S,
    loaded: &Sender<Loaded>,
    misses: &Sender<Want>,
    waiter: &Thread,
) {
    loop {
        let want = {
            let mut state = shared.state.lock();
            loop {
                if state.closed {
                    return;
                }
                let free = state
                    .wanted
                    .iter()
                    .position(|want| !state.busy.contains(&want.key));
                if let Some(free) = free {
                    let want = state.wanted.remove(free);
                    state.busy.insert(want.key);
                    break want;
                }
                shared.work.wait(&mut state);
            }
        };
        match source.cached(want.key) {
            Some(pixels) => shared.finish(loaded, want.key, Ok(pixels)),
            // Not built yet. The key stays busy; the waiter finishes it.
            None => {
                if misses.send(want).is_err() {
                    return;
                }
                waiter.unpark();
            }
        }
    }
}

fn wait_for_builds<S: ThumbSource>(
    shared: &Shared,
    source: &S,
    misses: &Receiver<Want>,
    loaded: &Sender<Loaded>,
    timeout: Duration,
) {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut waiting: Vec<(Want, Building<'_>, Instant)> = Vec::new();
    loop {
        if shared.state.lock().closed {
            return;
        }
        for want in misses.try_iter() {
            waiting.push((want, source.build(want.id), Instant::now() + timeout));
        }
        let now = Instant::now();
        let mut next = 0;
        while next < waiting.len() {
            let (want, building, deadline) = &mut waiting[next];
            let key = want.key;
            // `None`: given up, nothing to say. Dropping the future is what gives up.
            let outcome = if !shared.state.lock().keys.contains(&key) {
                Some(None)
            } else if let Poll::Ready(result) = building.as_mut().poll(&mut context) {
                Some(Some(result))
            } else if now >= *deadline {
                Some(Some(Err(LoadError::Unavailable)))
            } else {
                None
            };
            match outcome {
                None => next += 1,
                Some(result) => {
                    drop(waiting.swap_remove(next));
                    match result {
                        Some(result) => shared.finish(loaded, key, result),
                        None => {
                            shared.state.lock().busy.remove(&key);
                        }
                    }
                }
            }
        }
        // Woken by a miss, a new `want`, a build finishing or `Drop`; otherwise by the
        // nearest deadline.
        match waiting.iter().map(|(_, _, deadline)| *deadline).min() {
            Some(deadline) => thread::park_timeout(deadline.saturating_duration_since(now)),
            None => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn pixels(key: u64) -> Pixels {
        Pixels {
            width: 1,
            height: 1,
            rgba: vec![key as u8; 4],
        }
    }

    /// A source whose cache and whose builds the test controls. A photo's key is its id.
    #[derive(Default)]
    struct Fake {
        cached: Mutex<HashSet<u64>>,
        built: Mutex<HashSet<i64>>,
        wakers: Mutex<Vec<Waker>>,
        given_up: AtomicUsize,
    }

    impl Fake {
        fn finish_building(&self, id: i64) {
            self.built.lock().insert(id);
            for waker in self.wakers.lock().drain(..) {
                waker.wake();
            }
        }
    }

    struct FakeBuild<'a> {
        fake: &'a Fake,
        id: i64,
        done: bool,
    }

    impl Future for FakeBuild<'_> {
        type Output = Result<Pixels, LoadError>;
        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            if self.fake.built.lock().contains(&self.id) {
                self.done = true;
                return Poll::Ready(Ok(pixels(self.id as u64)));
            }
            self.fake.wakers.lock().push(context.waker().clone());
            Poll::Pending
        }
    }

    impl Drop for FakeBuild<'_> {
        fn drop(&mut self) {
            if !self.done {
                self.fake.given_up.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl ThumbSource for Fake {
        fn cached(&self, key: u64) -> Option<Pixels> {
            self.cached.lock().contains(&key).then(|| pixels(key))
        }
        fn build(&self, id: i64) -> Building<'_> {
            Box::pin(FakeBuild {
                fake: self,
                id,
                done: false,
            })
        }
    }

    fn want(id: i64) -> Want {
        Want { id, key: id as u64 }
    }

    fn loader(fake: &Arc<Fake>, decoders: usize, timeout: Duration) -> Loader {
        Loader::spawn(fake.clone(), decoders, timeout, || {})
    }

    const LONG: Duration = Duration::from_secs(600);

    /// Results until `count` have come, or panics after ten seconds.
    fn results(loader: &Loader, count: usize) -> Vec<Loaded> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut all = Vec::new();
        while all.len() < count {
            all.extend(loader.poll());
            assert!(
                Instant::now() < deadline,
                "{} of {count} results",
                all.len()
            );
            thread::sleep(Duration::from_millis(2));
        }
        all
    }

    /// Waits until `condition` holds, or panics after ten seconds.
    fn eventually(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "never: {what}");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_cached_thumbnail_is_read_and_handed_back_under_its_key() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().insert(7);
        let loader = loader(&fake, 2, LONG);
        loader.want(vec![want(7)]);
        let got = results(&loader, 1);
        assert_eq!(got[0].key, 7);
        assert_eq!(got[0].result, Ok(pixels(7)));
    }

    #[test]
    fn a_thumbnail_not_built_yet_comes_once_it_is() {
        let fake = Arc::new(Fake::default());
        let loader = loader(&fake, 1, LONG);
        loader.want(vec![want(3)]);
        eventually("the build is being waited for", || {
            !fake.wakers.lock().is_empty()
        });
        assert!(loader.poll().is_empty());
        fake.finish_building(3);
        let got = results(&loader, 1);
        assert_eq!((got[0].key, &got[0].result), (3, &Ok(pixels(3))));
    }

    // The lesson of `protocol.rs`: with waits on the decoding threads, five unbuilt
    // thumbnails ahead of a cached one would hold the one decoder for ever.
    #[test]
    fn waits_for_unbuilt_thumbnails_do_not_hold_up_a_cached_one() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().insert(99);
        let loader = loader(&fake, 1, LONG);
        let mut wanted: Vec<Want> = (1..=5).map(want).collect();
        wanted.push(want(99));
        loader.want(wanted);
        let got = results(&loader, 1);
        assert_eq!(got[0].key, 99);
        assert_eq!(
            fake.given_up.load(Ordering::SeqCst),
            0,
            "the five are still waited for"
        );
    }

    #[test]
    fn a_wait_for_a_thumbnail_no_longer_wanted_is_given_up() {
        let fake = Arc::new(Fake::default());
        let loader = loader(&fake, 1, LONG);
        loader.want(vec![want(3)]);
        eventually("the build is being waited for", || {
            !fake.wakers.lock().is_empty()
        });
        loader.want(Vec::new());
        eventually("the wait is dropped", || {
            fake.given_up.load(Ordering::SeqCst) == 1
        });
        assert!(loader.poll().is_empty(), "and nothing is said about it");
        // Wanted again later, it is waited for again and delivered.
        loader.want(vec![want(3)]);
        fake.finish_building(3);
        assert_eq!(results(&loader, 1)[0].key, 3);
    }

    #[test]
    fn a_thumbnail_not_built_in_time_is_unavailable() {
        let fake = Arc::new(Fake::default());
        let loader = loader(&fake, 1, Duration::from_millis(30));
        loader.want(vec![want(3)]);
        let got = results(&loader, 1);
        assert_eq!(
            (got[0].key, &got[0].result),
            (3, &Err(LoadError::Unavailable))
        );
    }

    #[test]
    fn the_ui_is_told_when_a_result_is_ready() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().insert(1);
        let told = Arc::new(AtomicUsize::new(0));
        let counter = told.clone();
        let loader = Loader::spawn(fake, 1, LONG, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        loader.want(vec![want(1)]);
        results(&loader, 1);
        eventually("told once", || told.load(Ordering::SeqCst) == 1);
    }

    #[test]
    fn the_most_wanted_is_decoded_first() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().extend([1, 2, 3]);
        let loader = loader(&fake, 1, LONG);
        loader.want(vec![want(2), want(3), want(1)]);
        let order: Vec<u64> = results(&loader, 3).iter().map(|l| l.key).collect();
        assert_eq!(order, [2, 3, 1]);
    }
}
```

- [ ] **Step 2: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod visible;
}
pub mod tasks;
pub mod theme {
    pub mod tokens;
}
pub mod thumbs {
    pub mod loader;
    pub mod textures;
}
```

- [ ] **Step 3: Run the tests, several times**

Run: `for i in 1 2 3 4 5; do cargo test -p photon-ui --lib loader 2>&1 | grep "test result"; done`
Expected: `7 passed` five times. These tests use real threads; one that passes four times in five is a failing test.

- [ ] **Step 4: Probes**

**`a_wait_for_a_thumbnail_no_longer_wanted_is_given_up`** - in `crates/photon-ui/src/thumbs/loader.rs` replace

```rust
            let outcome = if !shared.state.lock().keys.contains(&key) {
```

with

```rust
            let outcome = if false && !shared.state.lock().keys.contains(&key) {
```

Run: `cargo test -p photon-ui --lib a_wait_for_a_thumbnail_no_longer_wanted_is_given_up`
Expected: FAIL.

**`a_thumbnail_not_built_in_time_is_unavailable`** - in `crates/photon-ui/src/thumbs/loader.rs` replace

```rust
            } else if now >= *deadline {
```

with

```rust
            } else if false && now >= *deadline {
```

Run: `cargo test -p photon-ui --lib a_thumbnail_not_built_in_time_is_unavailable`
Expected: FAIL.

**`waits_for_unbuilt_thumbnails_do_not_hold_up_a_cached_one`** - in `crates/photon-ui/src/thumbs/loader.rs` replace

```rust
            None => {
                if misses.send(want).is_err() {
                    return;
                }
                waiter.unpark();
            }
```

with

```rust
            None => {
                let _ = (misses, waiter);
                loop {
                    thread::park();
                }
            }
```

Run: `cargo test -p photon-ui --lib waits_for_unbuilt_thumbnails_do_not_hold_up_a_cached_one`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

`loader blocks` makes a decoder wait on a miss, which is the design this module exists to avoid. The test fails by its ten-second deadline, so this probe takes that long.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/thumbs/loader.rs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): thumbnails read on a small pool, and waited for on one thread

A cached thumbnail is read and decoded by a decoder, most wanted first. One not built yet
is never blocked on: every such wait is a future, polled by one thread, bounded by a
deadline, and dropped when the grid stops wanting it. protocol.rs records the alternative:
a thread per wait, hundreds parked by one fast scroll, cached thumbnails queued behind.

Probed: a wait held for a tile no longer wanted, the deadline ignored, and a decoder made
to wait on a miss.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: One line of text in any mix of scripts

**What was found while this plan was written.** epaint 0.36 shapes text - Arabic letters join - but does not run the Unicode bidirectional algorithm. It cuts a line into stretches of one font face each, shapes every stretch in the direction of its first strong letter, and sets the stretches down left to right in the order of the text. Read out of its galleys' glyph positions:

| Text | Drawn by egui alone | Right |
| --- | --- | --- |
| `אב 12` | digits reversed: `21`, then the letters | `12`, then the letters |
| `رحلة الصيف` | the first word on the left | the first word on the right |
| `שלום עולם abc` | `cba` | `abc` |

This module cuts a line into pieces egui is right about, in the order they are read. It was verified by the same glyph positions, and by eye in Task 14's screenshots. It covers lines photon paints (headers here; names, captions and menus later). **It does not cover a text field**: typing and the caret in mixed-direction text are a finding for the sub-project that brings the search box and renaming.

**Files:**
- Create: `crates/photon-ui/src/text.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: `unicode_bidi::BidiInfo`.
- Produces: `Run<'a> { pub text: &'a str, pub rtl: bool }`; `visual_runs(text: &str) -> Vec<Run<'_>>`; `paint_line(ui: &egui::Ui, painter: &egui::Painter, (left, middle): (f32, f32), room: f32, text: &str, font: FontId, tint: Color32) -> f32`, which answers the width taken.

- [ ] **Step 1: `crates/photon-ui/src/text.rs`**

```rust
//! One line of text in any mix of scripts.
//!
//! epaint 0.36 shapes text - Arabic joins, and a stretch of one direction is laid out in
//! it - but it does not run the Unicode bidirectional algorithm. It cuts a line into
//! stretches of one font face each, shapes every stretch in the direction of its first
//! strong letter, and sets the stretches down left to right in the order of the text.
//! Measured 2026-10-09 by reading glyph positions out of its galleys:
//!
//! - "אב 12" came out as "21 בא": the digits were shaped inside the right-to-left stretch
//!   and reversed with it.
//! - "رحلة الصيف" came out with its first word on the left: the space is drawn by another
//!   face than the Arabic letters, so the two words were two stretches, set down left to
//!   right.
//! - "שלום עולם abc" came out with "cba".
//!
//! So a line is cut here first, into pieces epaint is right about: the runs the
//! bidirectional algorithm finds, and inside a right-to-left run its words and the spaces
//! between them, each piece in the place it is read at. A word in one script has one
//! direction and, nearly always, one face.

use eframe::egui::{self, Color32, FontId, Pos2, TextWrapMode, WidgetText, pos2};
use unicode_bidi::BidiInfo;

/// A piece of a line that is laid out by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    pub text: &'a str,
    pub rtl: bool,
}

/// The pieces of `text`, left to right as they are drawn. Only the first paragraph: a name
/// or a caption on one line has no other.
pub fn visual_runs(text: &str) -> Vec<Run<'_>> {
    let info = BidiInfo::new(text, None);
    let Some(paragraph) = info.paragraphs.first() else {
        return Vec::new();
    };
    let (levels, runs) = info.visual_runs(paragraph, paragraph.range.clone());
    let mut pieces = Vec::new();
    for run in runs.into_iter().filter(|run| !run.is_empty()) {
        let rtl = levels[run.start].is_rtl();
        let text = &text[run];
        if rtl {
            // Read from the right: the last word of the run is its leftmost piece.
            let words = words_and_spaces(text);
            pieces.extend(words.into_iter().rev().map(|text| Run { text, rtl }));
        } else {
            pieces.push(Run { text, rtl });
        }
    }
    pieces
}

/// `text` cut where whitespace begins and ends: "ab  cd " is "ab", "  ", "cd", " ".
fn words_and_spaces(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut space = None;
    for (at, letter) in text.char_indices() {
        let is_space = letter.is_whitespace();
        if space.is_some_and(|was| was != is_space) {
            pieces.push(&text[start..at]);
            start = at;
        }
        space = Some(is_space);
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}

/// Paints `text` on one line from `left`, centred on `middle`, in at most `room`, cut
/// short where the room ends. Answers the width it took.
pub fn paint_line(
    ui: &egui::Ui,
    painter: &egui::Painter,
    (left, middle): (f32, f32),
    room: f32,
    text: &str,
    font: FontId,
    tint: Color32,
) -> f32 {
    let mut used = 0.0;
    for run in visual_runs(text) {
        let left_over = room - used;
        if left_over <= 0.0 {
            break;
        }
        let galley = WidgetText::from(run.text).into_galley(
            ui,
            Some(TextWrapMode::Truncate),
            left_over,
            font.clone(),
        );
        let size = galley.size();
        let at: Pos2 = pos2(left + used, middle - size.y / 2.0);
        painter.galley(at, galley, tint);
        used += size.x;
    }
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(text: &str) -> Vec<(&str, bool)> {
        visual_runs(text)
            .into_iter()
            .map(|run| (run.text, run.rtl))
            .collect()
    }

    #[test]
    fn left_to_right_text_is_one_piece() {
        assert_eq!(runs("2026-07 Coast"), [("2026-07 Coast", false)]);
        assert!(runs("").is_empty());
    }

    // Read from the right, so the first word is the rightmost piece. Words and not the
    // whole run, because epaint sets two words down left to right when the space between
    // them is drawn by another face.
    #[test]
    fn right_to_left_text_is_its_words_from_the_last_to_the_first() {
        assert_eq!(
            runs("שלום עולם"),
            [("עולם", true), (" ", true), ("שלום", true)]
        );
        assert_eq!(
            runs("رحلة الصيف"),
            [("الصيف", true), (" ", true), ("رحلة", true)]
        );
    }

    // The line starts right-to-left, so it is read from the right: the Latin word, last in
    // the text, is drawn first from the left, and its letters keep their order.
    #[test]
    fn a_latin_word_after_hebrew_is_a_piece_of_its_own_at_the_left() {
        assert_eq!(
            runs("שלום עולם abc"),
            [
                ("abc", false),
                (" ", true),
                ("עולם", true),
                (" ", true),
                ("שלום", true)
            ]
        );
    }

    // A year inside an Arabic name reads left to right, as digits do in any script.
    #[test]
    fn digits_inside_right_to_left_text_keep_their_order() {
        assert_eq!(
            runs("رحلة 2024 الصيف"),
            [
                ("الصيف", true),
                (" ", true),
                ("2024", false),
                (" ", true),
                ("رحلة", true)
            ]
        );
        assert_eq!(runs("אב 12"), [("12", false), (" ", true), ("אב", true)]);
    }

    // A path is left-to-right with a right-to-left folder name in it.
    #[test]
    fn a_right_to_left_name_in_a_path_is_a_piece_in_its_place() {
        assert_eq!(
            runs("/home/Pictures/שלום/a.jpg"),
            [
                ("/home/Pictures/", false),
                ("שלום", true),
                ("/a.jpg", false)
            ]
        );
    }

    /// The letters of `text` from left to right as `paint_line` sets them down: each piece
    /// laid out by itself, the pieces side by side. With egui's own fonts, which every
    /// machine has, since they are compiled in: a letter they lack is still shaped in its
    /// script's direction.
    fn drawn(text: &str) -> String {
        let ctx = egui::Context::default();
        let mut drawn = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for run in visual_runs(text) {
                let galley = ui.painter().layout_no_wrap(
                    run.text.to_owned(),
                    FontId::proportional(15.0),
                    Color32::WHITE,
                );
                let mut glyphs: Vec<(f32, char)> = galley
                    .rows
                    .iter()
                    .flat_map(|row| row.glyphs.iter())
                    // A cluster of several letters has one glyph that is drawn and
                    // zero-width ones standing for the rest.
                    .filter(|glyph| glyph.advance_width > 0.0)
                    .map(|glyph| (glyph.pos.x, glyph.chr))
                    .collect();
                glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
                drawn.extend(glyphs.into_iter().map(|(_, letter)| letter));
            }
        });
        output.textures_delta.clear();
        drawn
    }

    // The three lines that were drawn wrong, as a reader now sees them from the left. A
    // right-to-left word reads from its right end, so its letters appear here reversed.
    #[test]
    fn a_line_is_set_down_in_the_order_it_is_read() {
        assert_eq!(drawn("ab cd"), "ab cd");
        assert_eq!(drawn("אב 12"), "12 בא");
        assert_eq!(drawn("رحلة الصيف"), "فيصلا ةلحر");
        assert_eq!(drawn("שלום עולם abc"), "abc םלוע םולש");
        assert_eq!(drawn("/p/שלום/a.jpg"), "/p/םולש/a.jpg");
    }

    #[test]
    fn text_is_cut_where_whitespace_begins_and_ends() {
        assert_eq!(words_and_spaces("ab  cd "), ["ab", "  ", "cd", " "]);
        assert_eq!(words_and_spaces(" x"), [" ", "x"]);
        assert_eq!(words_and_spaces("x"), ["x"]);
        assert!(words_and_spaces("").is_empty());
    }
}
```

- [ ] **Step 2: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod visible;
}
pub mod tasks;
pub mod text;
pub mod theme {
    pub mod tokens;
}
pub mod thumbs {
    pub mod loader;
    pub mod textures;
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-ui --lib text`
Expected: 7 tests pass. `a_line_is_set_down_in_the_order_it_is_read` lays text out with egui's own compiled-in fonts, so it does not depend on what is installed.

- [ ] **Step 4: Probe**

**`a_line_is_set_down_in_the_order_it_is_read`** - in `crates/photon-ui/src/text.rs` replace

```rust
            pieces.extend(words.into_iter().rev().map(|text| Run { text, rtl }));
```

with

```rust
            pieces.extend(words.into_iter().map(|text| Run { text, rtl }));
```

Run: `cargo test -p photon-ui --lib a_line_is_set_down_in_the_order_it_is_read`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

Two tests fail under it: the one named, and `right_to_left_text_is_its_words_from_the_last_to_the_first`.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/text.rs crates/photon-ui/src/lib.rs Cargo.lock
git commit -m "feat(ui): a line that mixes right-to-left and left-to-right text, drawn as read

epaint 0.36 shapes text but runs no bidirectional algorithm: it sets stretches of one font
face down left to right, each in the direction of its first strong letter. Measured from
its glyph positions, a year inside a Hebrew name came out reversed, a two-word Arabic name
with its first word on the left, and a Latin word after Hebrew backwards.

A line is cut first into the runs the algorithm finds (unicode-bidi), and a right-to-left
run into its words, each piece laid out by itself in the place it is read at. Painted
lines only: a text field is not covered.

Probed with the words of a right-to-left run left in the order of the text.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: The theme on egui, the faces, the icons

**Files:**
- Create: `crates/photon-ui/src/theme/apply.rs`, `crates/photon-ui/src/theme/fonts.rs`, `crates/photon-ui/src/icons.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: `theme::tokens::{Palette, Rgba, LIGHT, DARK}`; `photon_core::library::ThemeChoice`; `ui/src/lib/icons.ts`, read as text by a test.
- Produces, in `theme::apply`: `color(Rgba) -> egui::Color32`; `palette(&egui::Context) -> &'static Palette`; `install(&egui::Context)`; `choose(&egui::Context, ThemeChoice)`.
- Produces, in `theme::fonts`: `install(&egui::Context)`; `regular(size: f32) -> FontId`; `semibold(&egui::Context, size: f32) -> FontId`.
- Produces, in `icons`: `Icon { Copy, Play, Star, TriangleAlert }` with `Icon::ALL`, `svg(self, filled: bool) -> String`, `paint(self, ui: &egui::Ui, rect: Rect, size: f32, filled: bool, tint: Color32)`; `icons::install(&egui::Context)`.

`fonts::semibold` exists because asking egui for a font family that was never registered panics, and every headless test draws headers in a context `fonts::install` has not seen.

- [ ] **Step 1: `crates/photon-ui/src/theme/apply.rs`**

```rust
//! The tokens onto egui: its visuals for each theme, and which theme is in force.

use super::tokens::{DARK, LIGHT, Palette, Rgba};
use eframe::egui::{self, Color32, Theme, ThemePreference};
use photon_core::library::ThemeChoice;

pub fn color(Rgba(r, g, b, a): Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The palette of the theme egui is drawing in.
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

/// Sets both themes' visuals from the tokens, once, at startup.
pub fn install(ctx: &egui::Context) {
    for (theme, palette) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        let mut visuals = match theme {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
        };
        visuals.panel_fill = color(palette.surface);
        visuals.window_fill = color(palette.raised);
        visuals.extreme_bg_color = color(palette.surface);
        visuals.faint_bg_color = color(palette.hover);
        visuals.override_text_color = Some(color(palette.text));
        visuals.hyperlink_color = color(palette.accent);
        visuals.selection.bg_fill = color(palette.accent_soft);
        visuals.selection.stroke.color = color(palette.accent);
        ctx.set_visuals_of(theme, visuals);
    }
}

/// The user's choice. `System` is egui's own preference of that name and not the scheme
/// resolved here, which is what lets the window keep following the desktop while photon
/// runs - the reason `app.rs` gives for the Tauri title bar.
pub fn choose(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(match choice {
        ThemeChoice::System => ThemePreference::System,
        ThemeChoice::Light => ThemePreference::Light,
        ThemeChoice::Dark => ThemePreference::Dark,
    });
}
```

- [ ] **Step 2: `crates/photon-ui/src/theme/fonts.rs`**

```rust
//! The faces photon is set in: the platform's own interface face, as the Svelte UI's
//! `system-ui` was, and behind it an installed face for every script that one lacks.
//!
//! Found and registered by `fastframe-fonts` (MIT, pinned to a tag). Without its `inter`
//! feature nothing is bundled: where no platform face can be found or read, egui's own
//! fonts draw instead.

use eframe::egui::{self, FontId, Id};
use fastframe_fonts::{FontSetup, Primary, Weight};

/// Set in the context once `install` has registered the weights, so a context that never
/// had them - a test's - is not asked for a family it does not know, which panics.
fn marker() -> Id {
    Id::new("photon-fonts-installed")
}

/// Registers the faces. Once, at startup.
pub fn install(ctx: &egui::Context) {
    FontSetup::default()
        .primary(Primary::System)
        .weights(&[Weight::SemiBold])
        .install(ctx);
    ctx.data_mut(|data| data.insert_temp(marker(), true));
}

/// The regular weight at `size`.
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// The semibold weight at `size`; the regular one in a context `install` has not seen.
pub fn semibold(ctx: &egui::Context, size: f32) -> FontId {
    let installed = ctx.data(|data| data.get_temp::<bool>(marker()).unwrap_or(false));
    if installed {
        Weight::SemiBold.font_id(size)
    } else {
        regular(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every headless test draws headers in a context without the installed faces.
    #[test]
    fn a_context_without_the_faces_is_given_the_regular_weight() {
        let ctx = egui::Context::default();
        assert_eq!(semibold(&ctx, 15.0), regular(15.0));
    }
}
```

- [ ] **Step 3: `crates/photon-ui/src/icons.rs`**

```rust
//! Icons from Lucide (https://lucide.dev), lucide-static 1.47.0. ISC License, Copyright (c)
//! Lucide Icons and Contributors. The licence is reproduced in THIRD-PARTY-NOTICES.md.
//!
//! The same path data as `ui/src/lib/icons.ts`, on Lucide's 24-unit grid, drawn the way
//! `Icon.svelte` draws it: a 2-unit round stroke, filled or not. Each is rasterised by
//! egui's SVG loader in white and tinted with a token where it is painted.

use eframe::egui::{self, Color32, Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Copy,
    Play,
    Star,
    TriangleAlert,
}

impl Icon {
    pub const ALL: [Icon; 4] = [Icon::Copy, Icon::Play, Icon::Star, Icon::TriangleAlert];

    fn name(self) -> &'static str {
        match self {
            Icon::Copy => "copy",
            Icon::Play => "play",
            Icon::Star => "star",
            Icon::TriangleAlert => "triangle-alert",
        }
    }

    /// The inside of the icon's `<svg>`.
    fn inner(self) -> &'static str {
        match self {
            Icon::Copy => {
                r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#
            }
            Icon::Play => {
                r#"<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>"#
            }
            Icon::Star => {
                r#"<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/>"#
            }
            Icon::TriangleAlert => {
                r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
            }
        }
    }

    /// The whole document, in white: `filled` fills the outline as well as stroking it.
    pub fn svg(self, filled: bool) -> String {
        let fill = if filled { "white" } else { "none" };
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="{fill}" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
            self.inner()
        )
    }

    /// Paints the icon `size` points square, centred on `rect`, in `tint`.
    pub fn paint(self, ui: &egui::Ui, rect: Rect, size: f32, filled: bool, tint: Color32) {
        let uri = format!("bytes://photon-icon-{}-{filled}.svg", self.name());
        egui::Image::from_bytes(uri, self.svg(filled).into_bytes())
            .fit_to_exact_size(Vec2::splat(size))
            .tint(tint)
            .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(size)));
    }
}

/// The SVG loader the icons are drawn through. Once, at startup.
pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    // The path data is a hand copy of `icons.ts`, which stays the source until the
    // switch-over: a Lucide update there must not leave this file a version behind.
    #[test]
    fn every_icon_is_the_svelte_uis() {
        let source = include_str!("../../../ui/src/lib/icons.ts");
        for icon in Icon::ALL {
            assert!(
                source.contains(&format!("'{}'", icon.inner())),
                "{} differs from icons.ts",
                icon.name()
            );
        }
    }

    #[test]
    fn an_icon_is_a_whole_document_filled_or_not() {
        let outline = Icon::Star.svg(false);
        assert!(outline.starts_with("<svg ") && outline.ends_with("</svg>"));
        assert!(outline.contains(r#"fill="none""#));
        assert!(Icon::Star.svg(true).contains(r#"fill="white""#));
    }
}
```

- [ ] **Step 4: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod visible;
}
pub mod icons;
pub mod tasks;
pub mod text;
pub mod theme {
    pub mod apply;
    pub mod fonts;
    pub mod tokens;
}
pub mod thumbs {
    pub mod loader;
    pub mod textures;
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p photon-ui --lib icons && cargo test -p photon-ui --lib fonts`
Expected: 2 and 1 tests pass. `theme/apply.rs` has no test of its own: it sets fields on egui's visuals, and what it does is seen in Task 14's two PNGs.

- [ ] **Step 6: Probes**

**`every_icon_is_the_svelte_uis`** - in `crates/photon-ui/src/icons.rs` replace

```rust
<path d="M5 5a2 2 0 0 1 3.008
```

with

```rust
<path d="M5 6a2 2 0 0 1 3.008
```

Run: `cargo test -p photon-ui --lib every_icon_is_the_svelte_uis`
Expected: FAIL.

**`a_context_without_the_faces_is_given_the_regular_weight`** - in `crates/photon-ui/src/theme/fonts.rs` replace

```rust
    if installed {
```

with

```rust
    if installed || true {
```

Run: `cargo test -p photon-ui --lib a_context_without_the_faces_is_given_the_regular_weight`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/theme crates/photon-ui/src/icons.rs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): the tokens as egui's visuals, the platform's faces, four icons

The stored theme chooses between two sets of visuals made from the tokens; System is
egui's own preference of that name, so the window keeps following the desktop. Text is set
in the platform's interface face, as system-ui was, with an installed face behind it for
every script it lacks (fastframe-fonts, nothing bundled). The star, play, copy and warning
icons are icons.ts's path data, held to it by a test.

theme/apply.rs has no test: it is seen in the screenshots. Probed: an icon's path changed,
and the semibold family asked of a context that never registered it.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 12: The grid drawn

**Files:**
- Create: `crates/photon-ui/src/grid/tile.rs`, `crates/photon-ui/src/grid/header.rs`, `crates/photon-ui/src/thumbs/shown.rs`, `crates/photon-ui/src/grid/view.rs`
- Modify: `crates/photon-ui/src/lib.rs`

**Interfaces:**
- Consumes: everything Tasks 3 to 11 produce; `photon_core::grid::{GridIndex, GridEntry, Section}`, `photon_core::media::MediaKind`, `photon_core::library::{Folder, GridTile}`.
- Produces, in `thumbs::shown`: `Thumbs` with `Thumbs::new(loader: Loader) -> Self`, `frame(&mut self, ctx: &egui::Context, wanted: &[Want], defer: bool)`, `texture(&mut self, key: u64) -> Option<&egui::TextureHandle>`, `failed(&self, key: u64) -> bool`, `bytes(&self) -> usize`.
- Produces, in `grid::header`: `Heading { name, summary, path: String }`; `heading(&Section, &HashMap<i64, Folder>, &TimeZone) -> Heading`; `paint(ui: &egui::Ui, rect: Rect, &Heading, pinned: bool, &Palette)`.
- Produces, in `grid::tile`: `cover_uv(width: f32, height: f32) -> Rect`; `paint(ui: &egui::Ui, rect: Rect, &GridEntry, texture: Option<&TextureHandle>, failed: bool, &Palette)`.
- Produces, in `grid::view`: `GridData<'a> { layout_gen: u64, index: &'a GridIndex, folders: &'a HashMap<i64, Folder>, size: GridTile, zone: &'a TimeZone }`; `GridOutput { on_screen: Vec<i64>, pinned: Option<PinnedHeader>, rebuilt: bool, position: f64, settled: bool }`; `GridView` (`Default`) with `show(&mut self, ui: &mut egui::Ui, data: &GridData<'_>, thumbs: &mut Thumbs) -> GridOutput`, `scroll_to(f64)`, `scroll_by(f64)`, `max_position() -> f64`.

What is deliberately absent, so nobody adds it: selection, clicks, hover, the context menu, the rubber band, the fade-in, the drop shadow under the marks. The spec lists the sub-project each belongs to.

- [ ] **Step 1: `crates/photon-ui/src/grid/tile.rs`**

```rust
//! One tile: a square, the photo covering it, and its marks.

use super::labels::format_duration;
use crate::{
    icons::Icon,
    theme::{
        apply::color,
        fonts,
        tokens::{PHOTO_LINE, Palette, R, T},
    },
};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, TextureHandle, Vec2, pos2, vec2};
use photon_core::{grid::GridEntry, media::MediaKind};

/// How far a mark stands in from the tile's edges.
const INSET: f32 = 5.0;
const MARK: f32 = 14.0;
const PLAY: f32 = 12.0;
const PROBLEM: f32 = 28.0;

/// The part of a `width` by `height` picture that covers a square, centred: what
/// `object-fit: cover` shows. In texture coordinates, 0 to 1.
pub fn cover_uv(width: f32, height: f32) -> Rect {
    if width <= 0.0 || height <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    if width > height {
        let shown = height / width;
        Rect::from_min_max(
            pos2((1.0 - shown) / 2.0, 0.0),
            pos2((1.0 + shown) / 2.0, 1.0),
        )
    } else {
        let shown = width / height;
        Rect::from_min_max(
            pos2(0.0, (1.0 - shown) / 2.0),
            pos2(1.0, (1.0 + shown) / 2.0),
        )
    }
}

/// Paints `entry`'s tile in `rect`. `texture` is its thumbnail when it has arrived, and
/// `failed` whether it never will.
pub fn paint(
    ui: &egui::Ui,
    rect: Rect,
    entry: &GridEntry,
    texture: Option<&TextureHandle>,
    failed: bool,
    palette: &Palette,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, R[1], color(palette.field));
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        egui::Image::from_texture(texture)
            .uv(cover_uv(size.x, size.y))
            .corner_radius(R[1])
            .paint_at(ui, rect);
    }
    let video = entry.kind == MediaKind::Video;
    if failed {
        // A video with no frame reads as a video, not as broken: the play glyph is what
        // it will look like once it has one.
        let icon = if video {
            Icon::Play
        } else {
            Icon::TriangleAlert
        };
        icon.paint(ui, rect, PROBLEM, false, color(palette.text_dim));
    }
    let on_photo = color(PHOTO_LINE);
    if video {
        let corner = rect.min + Vec2::splat(INSET);
        let play = Rect::from_min_size(corner, Vec2::splat(PLAY));
        Icon::Play.paint(ui, play, PLAY, true, on_photo);
        if let Some(ms) = entry.duration_ms {
            painter.text(
                pos2(play.right() + 3.0, play.center().y),
                Align2::LEFT_CENTER,
                format_duration(ms),
                fonts::regular(T[0]),
                on_photo,
            );
        }
    }
    if entry.starred {
        let corner = rect.max - Vec2::splat(INSET + MARK);
        mark(ui, Icon::Star, corner, true, color(palette.star));
    }
    if entry.has_copies {
        let corner = pos2(rect.left() + INSET, rect.bottom() - INSET - MARK);
        mark(ui, Icon::Copy, corner, false, on_photo);
    }
}

fn mark(ui: &egui::Ui, icon: Icon, corner: Pos2, filled: bool, tint: Color32) {
    icon.paint(
        ui,
        Rect::from_min_size(corner, vec2(MARK, MARK)),
        MARK,
        filled,
        tint,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uv(width: f32, height: f32) -> (f32, f32, f32, f32) {
        let rect = cover_uv(width, height);
        (rect.min.x, rect.min.y, rect.max.x, rect.max.y)
    }

    #[test]
    fn a_square_picture_is_shown_whole() {
        assert_eq!(uv(256.0, 256.0), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn a_wide_picture_loses_its_sides_and_a_tall_one_its_top_and_bottom() {
        // 256x128: the middle half of the width.
        assert_eq!(uv(256.0, 128.0), (0.25, 0.0, 0.75, 1.0));
        // 128x256: the middle half of the height.
        assert_eq!(uv(128.0, 256.0), (0.0, 0.25, 1.0, 0.75));
    }

    #[test]
    fn a_picture_with_no_size_is_shown_as_it_is() {
        assert_eq!(uv(0.0, 100.0), (0.0, 0.0, 1.0, 1.0));
        assert_eq!(uv(100.0, 0.0), (0.0, 0.0, 1.0, 1.0));
    }
}
```

- [ ] **Step 2: `crates/photon-ui/src/grid/header.rs`**

```rust
//! A section's header: a folder's name, how many of its photos the view holds and since
//! when, and its path; or a period's name and count. Drawn in its row and, once that row
//! has scrolled away, pinned over the top of the grid - by this one function, so the two
//! cannot come to say different things.

use super::{
    labels::{folder_label, folder_summary, period_label, photo_count},
    layout::HEADER,
};
use crate::text::paint_line;
use crate::theme::{
    apply::color,
    fonts,
    tokens::{Palette, S, T},
};
use eframe::egui::{self, Color32, FontId, Rect};
use jiff::tz::TimeZone;
use photon_core::{grid::Section, library::Folder};
use std::collections::HashMap;

/// What a header says, left to right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub name: String,
    pub summary: String,
    /// A folder's path. Empty for a period, which belongs to no folder.
    pub path: String,
}

/// The heading of `section`. A folder not in `folders` yet - the list is read again a
/// moment after the grid that names it - has a blank name and no path, and its count, which
/// comes from the section.
pub fn heading(section: &Section, folders: &HashMap<i64, Folder>, zone: &TimeZone) -> Heading {
    if let Some(period) = section.period {
        return Heading {
            name: period_label(period),
            summary: photo_count(section.count),
            path: String::new(),
        };
    }
    let folder = section.folder_id.and_then(|id| folders.get(&id));
    Heading {
        name: folder.map(folder_label).unwrap_or_default().to_owned(),
        // From the section, not the folder: it counts the photos under this header, which
        // in a search are fewer.
        summary: folder_summary(section.count, section.taken_at_min, zone),
        path: folder.map(|folder| folder.path.clone()).unwrap_or_default(),
    }
}

/// Paints `heading` in `rect`, which is `HEADER` tall. `pinned` gives it the surface behind
/// and the line under it that set it off from the rows it lies over.
pub fn paint(ui: &egui::Ui, rect: Rect, heading: &Heading, pinned: bool, palette: &Palette) {
    let painter = ui.painter_at(rect);
    if pinned {
        painter.rect_filled(rect, 0.0, color(palette.surface));
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            (1.0, color(palette.line)),
        );
    }
    // One baseline for three sizes: the name's line box is the row less its top padding,
    // and the smaller texts are centred on the same line.
    let middle = rect.top() + 7.0 + (HEADER as f32 - 7.0) / 2.0;
    let right = rect.right() - S[1];
    // Draws `text` from `left`, in at most `share` of the room that is left, and answers
    // where the next text starts.
    let text = |left: f32, text: &str, font: FontId, tint: Color32, share: f32| -> f32 {
        let room = (right - left) * share;
        if text.is_empty() || room <= 0.0 {
            return left;
        }
        left + paint_line(ui, &painter, (left, middle), room, text, font, tint) + S[2]
    };
    // The name may take 70% of the row, so a long one cannot squeeze the path to nothing.
    let left = rect.left() + S[1];
    let dim = color(palette.text_dim);
    let name = fonts::semibold(ui.ctx(), T[3]);
    let left = text(left, &heading.name, name, color(palette.text), 0.7);
    let left = text(left, &heading.summary, fonts::regular(T[1]), dim, 1.0);
    text(left, &heading.path, fonts::regular(T[0]), dim, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::grid::Period;

    fn folders() -> HashMap<i64, Folder> {
        let folder = Folder {
            id: 7,
            watched_id: 1,
            parent_id: None,
            path: "/photos/2026-07 Coast".to_owned(),
            name: "2026-07 Coast".to_owned(),
            hidden: false,
            alias: Some("Coast".to_owned()),
        };
        HashMap::from([(7, folder)])
    }

    fn section(folder_id: Option<i64>, period: Option<Period>) -> Section {
        Section {
            folder_id,
            offset: 0,
            count: 23,
            // 2026-07-01 00:30 UTC.
            taken_at_min: 1_782_865_800,
            period,
        }
    }

    #[test]
    fn a_folders_header_names_it_counts_it_and_shows_its_path() {
        assert_eq!(
            heading(&section(Some(7), None), &folders(), &TimeZone::UTC),
            Heading {
                name: "Coast".to_owned(),
                summary: "23 photos · July 2026".to_owned(),
                path: "/photos/2026-07 Coast".to_owned(),
            }
        );
    }

    // The folder list is read by a task and the grid is not: a section can name a folder
    // the list does not hold yet.
    #[test]
    fn a_folder_not_listed_yet_has_its_count_and_a_blank_name() {
        let heading = heading(&section(Some(99), None), &folders(), &TimeZone::UTC);
        assert_eq!(heading.name, "");
        assert_eq!(heading.path, "");
        assert_eq!(heading.summary, "23 photos · July 2026");
    }

    #[test]
    fn a_periods_header_names_the_period_and_has_no_path() {
        let june = Period {
            year: 2024,
            month: Some(6),
            day: None,
        };
        assert_eq!(
            heading(&section(None, Some(june)), &folders(), &TimeZone::UTC),
            Heading {
                name: "June 2024".to_owned(),
                summary: "23 photos".to_owned(),
                path: String::new(),
            }
        );
    }
}
```

- [ ] **Step 3: `crates/photon-ui/src/thumbs/shown.rs`**

```rust
//! The thumbnails the grid draws: the loader's results uploaded as textures, within a
//! frame's budget, and the grid's wants handed back to the loader.

use super::{
    loader::{LoadError, Loader, Want},
    textures::{DEFAULT_LIMIT, Pixels, TexKey, Textures, UPLOADS_PER_FRAME},
};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashSet;

pub struct Thumbs {
    textures: Textures<TextureHandle>,
    loader: Loader,
}

impl Thumbs {
    pub fn new(loader: Loader) -> Self {
        Self {
            textures: Textures::new(DEFAULT_LIMIT),
            loader,
        }
    }

    /// Once per frame, before the tiles are drawn: takes what the loader has finished,
    /// uploads within the budget, and tells the loader what is wanted now - `wanted`, most
    /// wanted first, or nothing at all while `defer` holds, since a tile that will be gone
    /// before it settles must not cost a render.
    pub fn frame(&mut self, ctx: &egui::Context, wanted: &[Want], defer: bool) {
        self.textures.begin_frame();
        let keys: Vec<TexKey> = wanted.iter().map(|want| TexKey::grid(want.key)).collect();
        let wanted_keys: HashSet<TexKey> = keys.iter().copied().collect();
        let now = ctx.input(|input| input.time);

        for loaded in self.loader.poll() {
            let key = TexKey::grid(loaded.key);
            match loaded.result {
                Ok(pixels) => {
                    self.textures.offer(key, pixels, &wanted_keys);
                }
                Err(LoadError::Failed(_)) => self.textures.fail(key),
                Err(LoadError::Unavailable) => self.textures.put_off(key, now),
            }
        }
        let more = self
            .textures
            .upload(UPLOADS_PER_FRAME, &wanted_keys, |key, pixels| {
                upload(ctx, key, pixels)
            });
        if more {
            ctx.request_repaint();
        }

        let missing: HashSet<u64> = self
            .textures
            .missing(&keys, now)
            .into_iter()
            .map(|key| key.key)
            .collect();
        self.loader.want(if defer {
            Vec::new()
        } else {
            wanted
                .iter()
                .filter(|want| missing.contains(&want.key))
                .copied()
                .collect()
        });
    }

    /// The texture of the picture cached under `key`, if it has been uploaded.
    pub fn texture(&mut self, key: u64) -> Option<&TextureHandle> {
        self.textures.get(TexKey::grid(key))
    }

    /// Whether the picture under `key` could not be made.
    pub fn failed(&self, key: u64) -> bool {
        self.textures.failed(TexKey::grid(key))
    }

    /// Bytes of texture held.
    pub fn bytes(&self) -> usize {
        self.textures.bytes()
    }
}

fn upload(ctx: &egui::Context, key: TexKey, pixels: &Pixels) -> TextureHandle {
    ctx.load_texture(
        format!("thumb-{:016x}", key.key),
        ColorImage::from_rgba_unmultiplied(
            [pixels.width as usize, pixels.height as usize],
            &pixels.rgba,
        ),
        TextureOptions::LINEAR,
    )
}
```

- [ ] **Step 4: `crates/photon-ui/src/grid/view.rs`**

```rust
//! The grid: lays the published index out, moves through it, and draws what is in view.
//!
//! One function does all three in that order, every frame. That order is why several rules
//! of the Svelte grid have no successor here (`placeIn`, `restoredTo`, `viewTop`): they
//! bridged the render in which the tiles changed and the effect that moved the viewport
//! after it, and here the place is found again before anything is drawn.

use super::{
    header,
    layout::{
        GAP, HEADER, PinnedHeader, Row, RowKind, build_rows, columns_for, defers_thumbs,
        header_rows, pin_at, pin_top, pinned_header, row_width, tile_for, tile_row, tile_width,
        total_height, visible_range, wanted_range,
    },
    motion::{Direction, Motion, ScrollSpeed},
    scroll::{BAR_WIDTH, Scroll},
    tile,
};
use crate::{
    theme::apply::{color, palette},
    thumbs::{loader::Want, shown::Thumbs},
};
use eframe::egui::{self, Key, Rect, Sense, UiBuilder, Vec2, pos2, vec2};
use jiff::tz::TimeZone;
use photon_core::{
    grid::GridIndex,
    library::{Folder, GridTile},
};
use std::{collections::HashMap, time::Duration};

/// How far an arrow key moves the grid.
const ARROW_STEP: f64 = 40.0;

/// What the grid draws this frame.
pub struct GridData<'a> {
    /// Moves when a publish changes the sections (`Engine::published`): what the rows are
    /// rebuilt on, where a new version alone - a star, a new thumbnail - is not.
    pub layout_gen: u64,
    pub index: &'a GridIndex,
    pub folders: &'a HashMap<i64, Folder>,
    pub size: GridTile,
    /// The viewer's zone, for the month a folder's header names.
    pub zone: &'a TimeZone,
}

/// What a frame of the grid came to.
#[derive(Clone, Debug, PartialEq)]
pub struct GridOutput {
    /// The photos in view, in grid order: what the engine's thumbnail queue is told to
    /// put first.
    pub on_screen: Vec<i64>,
    pub pinned: Option<PinnedHeader>,
    /// Whether the rows were built again this frame.
    pub rebuilt: bool,
    pub position: f64,
    /// Whether every tile in view has its picture, or the mark that it will never have one.
    pub settled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LaidOut {
    layout_gen: u64,
    columns: usize,
    tile: f64,
}

#[derive(Default)]
pub struct GridView {
    scroll: Scroll,
    speed: ScrollSpeed,
    rows: Vec<Row>,
    headers: Vec<usize>,
    laid_out: Option<LaidOut>,
    /// Where on the thumb the pointer took hold, while the scrollbar is held.
    grab: Option<f64>,
}

impl GridView {
    /// Moves the grid to `position`, as the probe's programme does.
    pub fn scroll_to(&mut self, position: f64) {
        self.scroll.set(position);
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.scroll.scroll_by(delta);
    }

    pub fn max_position(&self) -> f64 {
        self.scroll.max()
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &GridData<'_>,
        thumbs: &mut Thumbs,
    ) -> GridOutput {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let bar = Rect::from_min_max(pos2(rect.right() - BAR_WIDTH as f32, rect.top()), rect.max);
        let area = Rect::from_min_max(rect.min, pos2(bar.left(), rect.bottom()));
        let viewport = f64::from(area.height());

        let rebuilt = self.lay_out(data, f64::from(area.width()), viewport);

        // Measured after the layout: a place restored across a resize is not a scroll.
        let before = self.scroll.position();
        self.take_input(ui, rect, bar, viewport);
        let now = ui.input(|input| input.time) * 1000.0;
        if self.scroll.position() != before {
            self.speed.sample(self.scroll.position(), now, viewport);
        }
        self.speed.tick(now);
        if let Some(at) = self.speed.settles_at() {
            // Nothing else would draw the frame in which the scroll counts as over.
            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
            ui.ctx().request_repaint_after(wait);
        }

        let top = self.scroll.position();
        let motion = self.speed.motion();
        let (first, last) = visible_range(&self.rows, top, viewport, 0.0);
        let (wanted, on_screen) = self.wanted(data.index, top, viewport, motion, (first, last));
        thumbs.frame(ui.ctx(), &wanted, defers_thumbs(motion, viewport));

        let settled = self.draw(ui, area, data, thumbs, (first, last));
        let pinned = pinned_header(&self.rows, &self.headers, top);
        if let Some(pinned) = pinned
            && let Some(section) = data.index.sections().get(pinned.section)
        {
            let heading = header::heading(section, data.folders, data.zone);
            let at = pos2(area.left(), area.top() + pinned.y as f32);
            let rect = Rect::from_min_size(at, vec2(area.width(), HEADER as f32));
            let clipped = ui.new_child(UiBuilder::new().max_rect(area));
            header::paint(
                &clipped,
                rect.intersect(area),
                &heading,
                true,
                palette(ui.ctx()),
            );
        }
        self.draw_scrollbar(ui, bar);

        GridOutput {
            on_screen,
            pinned,
            rebuilt,
            position: top,
            settled,
        }
    }

    /// Builds the rows again when the sections, the column count or the tile changed.
    ///
    /// Tiles fill the row, so every row's height moves with every pixel of a resize and a
    /// position kept as a number would name another photo: across a change of the tile or
    /// the columns the place is read from the rows as they were (`pin_at`) and found in the
    /// rows as they are (`pin_top`). Across a rebuilt index it is kept as the number it
    /// was, held to the new end, as the Svelte grid keeps it.
    fn lay_out(&mut self, data: &GridData<'_>, width: f64, viewport: f64) -> bool {
        let nominal = tile_width(data.size);
        let row = row_width(width);
        let now = LaidOut {
            layout_gen: data.layout_gen,
            columns: columns_for(row, nominal),
            tile: tile_for(row, nominal),
        };
        let rebuilt = self.laid_out != Some(now);
        let mut pin = None;
        if rebuilt {
            let resized = self
                .laid_out
                .is_some_and(|was| was.columns != now.columns || was.tile != now.tile);
            if resized {
                pin = pin_at(&self.rows, self.scroll.position());
            }
            self.rows = build_rows(data.index.sections(), now.columns, now.tile);
            self.headers = header_rows(&self.rows);
            self.laid_out = Some(now);
        }
        self.scroll.set_extent(total_height(&self.rows), viewport);
        if let Some(top) = pin.and_then(|pin| pin_top(&self.rows, pin)) {
            self.scroll.set(top);
        }
        rebuilt
    }

    fn take_input(&mut self, ui: &egui::Ui, rect: Rect, bar: Rect, viewport: f64) {
        if ui.rect_contains_pointer(rect) {
            // Positive moves the content down, which is towards the top of the grid.
            let delta = ui.input(|input| input.smooth_scroll_delta.y);
            if delta != 0.0 {
                self.scroll.scroll_by(-f64::from(delta));
            }
        }

        let row = self.laid_out.map_or(0.0, |laid| tile_row(laid.tile));
        let page = (viewport - row).max(ARROW_STEP);
        let scroll = &mut self.scroll;
        ui.input(|input| {
            if input.key_pressed(Key::Home) {
                scroll.set(0.0);
            }
            if input.key_pressed(Key::End) {
                scroll.set(f64::INFINITY);
            }
            if input.key_pressed(Key::PageDown) {
                scroll.scroll_by(page);
            }
            if input.key_pressed(Key::PageUp) {
                scroll.scroll_by(-page);
            }
            if input.key_pressed(Key::ArrowDown) {
                scroll.scroll_by(ARROW_STEP);
            }
            if input.key_pressed(Key::ArrowUp) {
                scroll.scroll_by(-ARROW_STEP);
            }
        });

        // A press on the thumb holds it where it was taken; a press on the track brings
        // the thumb's middle under the pointer. Either way the grid follows the pointer
        // for as long as the button is down.
        let response = ui.interact(bar, ui.id().with("grid-scrollbar"), Sense::click_and_drag());
        let track = f64::from(bar.height());
        let held = response
            .is_pointer_button_down_on()
            .then(|| response.interact_pointer_pos())
            .flatten();
        match (held, self.scroll.thumb(track)) {
            (Some(pointer), Some((start, length))) => {
                let y = f64::from(pointer.y - bar.top());
                let on_thumb = (start..=start + length).contains(&y);
                let grab =
                    *self
                        .grab
                        .get_or_insert(if on_thumb { y - start } else { length / 2.0 });
                self.scroll
                    .set(self.scroll.position_for_thumb(track, y - grab));
            }
            _ => self.grab = None,
        }
    }

    /// The thumbnails to ask for, most wanted first - what is in view, then what the scroll
    /// is heading for, nearest first, then what it has just left - and the photos in view.
    fn wanted(
        &self,
        index: &GridIndex,
        top: f64,
        viewport: f64,
        motion: Motion,
        (first, last): (usize, usize),
    ) -> (Vec<Want>, Vec<i64>) {
        let (from, to) = wanted_range(&self.rows, top, viewport, motion);
        let mut wanted = Vec::new();
        let take = |wanted: &mut Vec<Want>, row: &Row| {
            if row.kind == RowKind::Tiles {
                let entries = index.rows(row.first, row.count).iter();
                wanted.extend(entries.map(|entry| Want {
                    id: entry.id,
                    key: entry.thumb_key,
                }));
            }
        };
        for row in &self.rows[first..last] {
            take(&mut wanted, row);
        }
        let on_screen = wanted.len();
        let below = &self.rows[last.min(to)..to];
        let above = &self.rows[from..first.max(from)];
        let upward = matches!(
            motion,
            Motion::Scroll {
                direction: Direction::Up,
                ..
            }
        );
        if upward {
            above.iter().rev().for_each(|row| take(&mut wanted, row));
            below.iter().for_each(|row| take(&mut wanted, row));
        } else {
            below.iter().for_each(|row| take(&mut wanted, row));
            above.iter().rev().for_each(|row| take(&mut wanted, row));
        }
        let ids = wanted[..on_screen].iter().map(|want| want.id).collect();
        (wanted, ids)
    }

    /// Draws the rows in view. `true` when every tile drawn has its picture or its mark.
    fn draw(
        &self,
        ui: &mut egui::Ui,
        area: Rect,
        data: &GridData<'_>,
        thumbs: &mut Thumbs,
        (first, last): (usize, usize),
    ) -> bool {
        let Some(laid) = self.laid_out else {
            return true;
        };
        let palette = palette(ui.ctx());
        // A child whose clip is the grid's own area: a row half scrolled out is cut at the
        // edge and not drawn over what lies beside the grid.
        let clipped = ui.new_child(UiBuilder::new().max_rect(area));
        let top = snapped(self.scroll.position(), f64::from(ui.pixels_per_point()));
        let mut settled = true;
        for row in &self.rows[first..last] {
            let y = area.top() + (row.top - top) as f32;
            match row.kind {
                RowKind::Header => {
                    // The rows are built from the sections they index; one that is not
                    // there means an index was published without its layout generation
                    // moving, and a missing header is the least harm.
                    let Some(section) = data.index.sections().get(row.section) else {
                        continue;
                    };
                    let heading = header::heading(section, data.folders, data.zone);
                    let rect = Rect::from_min_size(
                        pos2(area.left(), y),
                        vec2(area.width(), HEADER as f32),
                    );
                    header::paint(&clipped, rect.intersect(area), &heading, false, palette);
                }
                RowKind::Tiles => {
                    let entries = data.index.rows(row.first, row.count);
                    for (column, entry) in entries.iter().enumerate() {
                        let x = area.left() + (GAP + column as f64 * tile_row(laid.tile)) as f32;
                        let rect = Rect::from_min_size(pos2(x, y), Vec2::splat(laid.tile as f32));
                        let failed = thumbs.failed(entry.thumb_key);
                        let texture = thumbs.texture(entry.thumb_key);
                        settled &= failed || texture.is_some();
                        tile::paint(&clipped, rect, entry, texture, failed, palette);
                    }
                }
            }
        }
        settled
    }

    fn draw_scrollbar(&self, ui: &egui::Ui, bar: Rect) {
        let Some((start, length)) = self.scroll.thumb(f64::from(bar.height())) else {
            return;
        };
        let thumb = Rect::from_min_size(
            pos2(bar.left() + 3.0, bar.top() + start as f32),
            vec2(bar.width() - 6.0, length as f32),
        );
        let palette = palette(ui.ctx());
        let tint = if self.grab.is_some() {
            palette.text_dim
        } else {
            palette.field_hover
        };
        ui.painter_at(bar)
            .rect_filled(thumb, thumb.width() / 2.0, color(tint));
    }
}

/// `position` on a whole device pixel at `scale` device pixels a point. Rows drawn at a
/// fraction of one have every picture and every letter resampled, differently on each
/// frame of a scroll.
fn snapped(position: f64, scale: f64) -> f64 {
    if scale > 0.0 {
        (position * scale).round() / scale
    } else {
        position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        grid::layout::{row_index_at, row_of_item},
        thumbs::{
            loader::{Building, LoadError, Loader, ThumbSource},
            textures::Pixels,
        },
    };
    use eframe::egui::{
        Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, TouchPhase,
    };
    use photon_core::{
        grid::{GridEntry, Layout},
        media::MediaKind,
    };
    use std::{sync::Arc, time::Instant};

    /// Every thumbnail is cached, a 2x1 picture.
    struct AllCached;

    impl ThumbSource for AllCached {
        fn cached(&self, _key: u64) -> Option<Pixels> {
            Some(Pixels {
                width: 2,
                height: 1,
                rgba: vec![255; 8],
            })
        }
        fn build(&self, _id: i64) -> Building<'_> {
            Box::pin(async { Err(LoadError::Unavailable) })
        }
    }

    /// An index of folders holding `counts` photos each. A photo's id is its place plus
    /// one, and its thumbnail key its id.
    fn index(counts: &[usize]) -> GridIndex {
        let mut entries = Vec::new();
        for (folder, count) in counts.iter().enumerate() {
            for _ in 0..*count {
                let id = entries.len() as i64 + 1;
                entries.push(GridEntry {
                    id,
                    folder_id: folder as i64 + 1,
                    taken_at: id,
                    aspect: 1.5,
                    kind: MediaKind::Image,
                    duration_ms: None,
                    starred: false,
                    has_copies: false,
                    thumb_key: id as u64,
                    size: 0,
                    mtime_ms: 0,
                });
            }
        }
        GridIndex::build(entries, Layout::Folders)
    }

    struct Fixture {
        ctx: egui::Context,
        view: GridView,
        thumbs: Thumbs,
        index: GridIndex,
        layout_gen: u64,
        size: Vec2,
        time: f64,
    }

    /// No thumbnail can be made.
    struct NoneCanBeMade;

    impl ThumbSource for NoneCanBeMade {
        fn cached(&self, _key: u64) -> Option<Pixels> {
            None
        }
        fn build(&self, _id: i64) -> Building<'_> {
            Box::pin(async { Err(LoadError::Failed("not a picture".to_owned())) })
        }
    }

    /// 800x600: the 12px bar and the two gutters leave a row of 772px, which is four medium
    /// tiles widened to 187px, in rows of 195px.
    fn fixture(counts: &[usize]) -> Fixture {
        fixture_from(counts, AllCached)
    }

    fn fixture_from(counts: &[usize], source: impl ThumbSource) -> Fixture {
        let loader = Loader::spawn(Arc::new(source), 1, Duration::from_secs(1), || {});
        Fixture {
            ctx: egui::Context::default(),
            view: GridView::default(),
            thumbs: Thumbs::new(loader),
            index: index(counts),
            layout_gen: 1,
            size: vec2(800.0, 600.0),
            time: 0.0,
        }
    }

    impl Fixture {
        fn frame(&mut self, events: Vec<Event>) -> GridOutput {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let folders = HashMap::new();
            let data = GridData {
                layout_gen: self.layout_gen,
                index: &self.index,
                folders: &folders,
                size: GridTile::Medium,
                zone: &TimeZone::UTC,
            };
            let (view, thumbs) = (&mut self.view, &mut self.thumbs);
            let mut output = None;
            let mut full = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| output = Some(view.show(ui, &data, thumbs)));
            });
            full.textures_delta.clear();
            output.unwrap()
        }

        fn key(&mut self, key: Key) -> GridOutput {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }])
        }

        /// The first photo of the row at the top of the grid.
        fn top_photo(&self) -> usize {
            let rows = &self.view.rows;
            rows[row_index_at(rows, self.view.scroll.position())].first
        }
    }

    #[test]
    fn the_wheel_moves_the_grid_by_what_it_was_turned() {
        let mut f = fixture(&[200]);
        let over = Pos2::new(300.0, 300.0);
        f.frame(vec![Event::PointerMoved(over)]);
        f.frame(vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -120.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }]);
        // egui spreads a wheel step over a few frames.
        let mut position = 0.0;
        for _ in 0..60 {
            position = f.frame(Vec::new()).position;
        }
        assert!((position - 120.0).abs() < 0.5, "moved {position}");
    }

    #[test]
    fn end_shows_the_last_row_and_home_the_first() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        let end = f.key(Key::End);
        assert_eq!(end.position, f.view.max_position());
        assert!(end.position > 0.0);
        assert_eq!(
            *end.on_screen.last().unwrap(),
            200,
            "the last photo is in view"
        );
        assert_eq!(f.key(Key::Home).position, 0.0);
    }

    #[test]
    fn a_page_key_moves_a_viewport_less_a_row_and_an_arrow_forty_pixels() {
        let mut f = fixture(&[200]);
        f.frame(Vec::new());
        // Rows of 195px: a 600px viewport less a row is 405.
        assert_eq!(f.key(Key::PageDown).position, 405.0);
        assert_eq!(f.key(Key::ArrowDown).position, 445.0);
        assert_eq!(f.key(Key::ArrowUp).position, 405.0);
        assert_eq!(f.key(Key::PageUp).position, 0.0);
    }

    // Tiles fill the row, so a narrower window is other rows at other heights: the grid
    // must still be on the photo it was on.
    #[test]
    fn narrowing_the_window_keeps_the_photo_at_the_top() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        f.view.scroll_to(5000.0);
        f.frame(Vec::new());
        let photo = f.top_photo();
        assert!(photo > 0);

        f.size = vec2(560.0, 600.0);
        let after = f.frame(Vec::new());
        assert!(after.rebuilt);
        let rows = &f.view.rows;
        assert_eq!(
            row_of_item(rows, photo),
            Some(row_index_at(rows, after.position)),
            "the row at the top holds the photo that was at the top"
        );
    }

    #[test]
    fn the_rows_are_built_again_for_a_new_layout_and_not_for_a_new_frame() {
        let mut f = fixture(&[50, 50]);
        assert!(f.frame(Vec::new()).rebuilt);
        assert!(!f.frame(Vec::new()).rebuilt);
        // A publish that moved a photo between sections.
        f.index = index(&[49, 51]);
        f.layout_gen = 2;
        assert!(f.frame(Vec::new()).rebuilt);
        assert_eq!(f.view.rows.iter().map(|row| row.count).sum::<usize>(), 100);
    }

    #[test]
    fn a_rebuilt_index_keeps_the_position_and_holds_it_to_the_new_end() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        f.view.scroll_to(5000.0);
        assert_eq!(f.frame(Vec::new()).position, 5000.0);
        // The same place in a library that grew.
        f.index = index(&[800]);
        f.layout_gen = 2;
        assert_eq!(f.frame(Vec::new()).position, 5000.0);
        // And the end of one that shrank under it.
        f.index = index(&[8]);
        f.layout_gen = 3;
        let shrunk = f.frame(Vec::new());
        assert_eq!(shrunk.position, f.view.max_position());
        assert!(shrunk.position < 5000.0);
    }

    #[test]
    fn deep_in_a_folder_its_header_is_pinned_and_the_next_one_pushes_it_out() {
        // The second folder is long enough that the grid can scroll to its header.
        let mut f = fixture(&[8, 40]);
        assert_eq!(f.frame(Vec::new()).pinned, None);
        f.view.scroll_to(200.0);
        let deep = f.frame(Vec::new()).pinned.unwrap();
        assert_eq!((deep.section, deep.y), (0, 0.0));
        // Folder 1 is a header and two rows: 32 + 2 * 195 = 422, then the gap; folder 2's
        // header is at 446. Ten pixels short of it, the pinned header is pushed up by 22.
        f.view.scroll_to(436.0);
        let pushed = f.frame(Vec::new()).pinned.unwrap();
        assert_eq!((pushed.section, pushed.y), (0, -22.0));
    }

    #[test]
    fn a_press_on_the_scrollbars_track_takes_the_grid_there() {
        let mut f = fixture(&[400]);
        f.frame(Vec::new());
        // The bar is the last 12px of the width; its bottom is the end of the grid.
        let bottom = Pos2::new(794.0, 598.0);
        f.frame(vec![Event::PointerMoved(bottom)]);
        let pressed = f.frame(vec![Event::PointerButton {
            pos: bottom,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        let held = f.frame(Vec::new());
        assert_eq!(pressed.position.max(held.position), f.view.max_position());
    }

    #[test]
    fn what_is_in_view_is_reported_and_gets_its_pictures() {
        let mut f = fixture(&[40]);
        let first = f.frame(Vec::new());
        // Four columns; the header and three rows are in a 600px viewport, the fourth row
        // begins at 32 + 3 * 195 = 617.
        assert_eq!(first.on_screen, (1..=12).collect::<Vec<i64>>());
        assert!(!first.settled, "nothing has been read yet");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f.frame(Vec::new()).settled {
            assert!(Instant::now() < deadline, "the pictures never came");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(f.thumbs.texture(1).is_some());
    }

    // A first run, and every launch until the engine's first build: the index is empty.
    #[test]
    fn an_empty_library_draws_nothing_and_still_takes_its_keys() {
        let mut f = fixture(&[]);
        let first = f.frame(Vec::new());
        assert!(first.on_screen.is_empty());
        assert_eq!(
            (first.position, first.pinned, first.settled),
            (0.0, None, true)
        );
        for key in [
            Key::End,
            Key::PageDown,
            Key::ArrowDown,
            Key::Home,
            Key::PageUp,
        ] {
            assert_eq!(f.key(key).position, 0.0, "{key:?}");
        }
    }

    // A window being dragged shut, or minimised: the grid is given no room, or less than a
    // tile, or less than the scrollbar is wide.
    #[test]
    fn a_window_with_no_room_is_drawn_without_a_panic() {
        let mut f = fixture(&[40]);
        f.frame(Vec::new());
        f.view.scroll_to(500.0);
        for size in [
            vec2(0.0, 0.0),
            vec2(5.0, 5.0),
            vec2(800.0, 3.0),
            vec2(8.0, 600.0),
        ] {
            f.size = size;
            f.frame(Vec::new());
            f.key(Key::End);
        }
        // And it is a grid again when the room comes back.
        f.size = vec2(800.0, 600.0);
        assert!(!f.frame(Vec::new()).on_screen.is_empty());
    }

    // The rows are built for the sections of one index. An index swapped in without its
    // layout generation moving is the engine's contract broken, and must cost a wrong
    // frame, not the application.
    #[test]
    fn rows_that_outlive_their_index_are_drawn_as_far_as_it_goes() {
        let mut f = fixture(&[50, 50]);
        f.frame(Vec::new());
        // The second folder's header is at 2591: a header and thirteen rows of 195, and
        // the gap. In view at 2500, and pinned at the end.
        for position in [2500.0, f64::INFINITY] {
            f.index = index(&[50, 50]);
            f.view.scroll_to(position);
            f.frame(Vec::new());
            // One folder of three photos, under rows built for two folders of fifty.
            f.index = index(&[3]);
            let after = f.frame(Vec::new());
            assert!(!after.rebuilt);
            assert!(after.on_screen.iter().all(|id| (1..=3).contains(id)));
        }
    }

    // An unreadable file, or one that is no picture: the tile shows its mark, is not
    // asked for again, and counts as having what it will ever have.
    #[test]
    fn a_thumbnail_that_cannot_be_made_settles_as_its_mark() {
        let mut f = fixture_from(&[4], NoneCanBeMade);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f.frame(Vec::new()).settled {
            assert!(Instant::now() < deadline, "the failures never came back");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(f.thumbs.failed(1));
        assert!(f.thumbs.texture(1).is_none());
    }

    // A laptop at 150%, a 4K screen at 175%: a row drawn between two device pixels blurs
    // every picture in it.
    #[test]
    fn rows_are_drawn_on_whole_device_pixels_at_any_scale() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
            for position in [0.0, 100.3, 100.5, 33_554_431.7] {
                let device = snapped(position, scale) * scale;
                assert!(
                    (device - device.round()).abs() < 1e-6,
                    "{position} at {scale}"
                );
                assert!((snapped(position, scale) - position).abs() <= 0.5 / scale + 1e-9);
            }
        }
        assert_eq!(snapped(100.3, 1.0), 100.0);
        assert_eq!(snapped(100.3, 0.0), 100.3);
    }
}
```

- [ ] **Step 5: `lib.rs`'s modules after this task**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod grid {
    pub mod header;
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod tile;
    pub mod view;
    pub mod visible;
}
pub mod icons;
pub mod tasks;
pub mod text;
pub mod theme {
    pub mod apply;
    pub mod fonts;
    pub mod tokens;
}
pub mod thumbs {
    pub mod loader;
    pub mod shown;
    pub mod textures;
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p photon-ui --lib grid::view && cargo test -p photon-ui --lib grid::tile && cargo test -p photon-ui --lib grid::header`
Expected: 14, 3 and 3 tests pass. The view's tests run whole frames of egui without a window (`Context::run_ui`), with a loader whose source is a fake.

- [ ] **Step 7: Probes**

**`narrowing_the_window_keeps_the_photo_at_the_top`** - in `crates/photon-ui/src/grid/view.rs` replace

```rust
            if resized {
                pin = pin_at(&self.rows, self.scroll.position());
```

with

```rust
            if false && resized {
                pin = pin_at(&self.rows, self.scroll.position());
```

Run: `cargo test -p photon-ui --lib narrowing_the_window_keeps_the_photo_at_the_top`
Expected: FAIL.

**`the_rows_are_built_again_for_a_new_layout_and_not_for_a_new_frame`** - in `crates/photon-ui/src/grid/view.rs` replace

```rust
        let rebuilt = self.laid_out != Some(now);
```

with

```rust
        let rebuilt = true || self.laid_out != Some(now);
```

Run: `cargo test -p photon-ui --lib the_rows_are_built_again_for_a_new_layout_and_not_for_a_new_frame`
Expected: FAIL.

**`the_wheel_moves_the_grid_by_what_it_was_turned`** - in `crates/photon-ui/src/grid/view.rs` replace

```rust
                self.scroll.scroll_by(-f64::from(delta));
```

with

```rust
                self.scroll.scroll_by(f64::from(delta));
```

Run: `cargo test -p photon-ui --lib the_wheel_moves_the_grid_by_what_it_was_turned`
Expected: FAIL.

**`a_press_on_the_scrollbars_track_takes_the_grid_there`** - in `crates/photon-ui/src/grid/view.rs` replace

```rust
.get_or_insert(if on_thumb { y - start } else { length / 2.0 });
```

with

```rust
.get_or_insert(if on_thumb { y - start } else { y });
```

Run: `cargo test -p photon-ui --lib a_press_on_the_scrollbars_track_takes_the_grid_there`
Expected: FAIL.

**`a_wide_picture_loses_its_sides_and_a_tall_one_its_top_and_bottom`** - in `crates/photon-ui/src/grid/tile.rs` replace

```rust
    if width > height {
        let shown = height / width;
```

with

```rust
    if width < height {
        let shown = height / width;
```

Run: `cargo test -p photon-ui --lib a_wide_picture_loses_its_sides_and_a_tall_one_its_top_and_bottom`
Expected: FAIL.

**`a_folder_not_listed_yet_has_its_count_and_a_blank_name`** - in `crates/photon-ui/src/grid/header.rs` replace

```rust
        summary: folder_summary(section.count, section.taken_at_min, zone),
```

with

```rust
        summary: folder_summary(0, section.taken_at_min, zone),
```

Run: `cargo test -p photon-ui --lib a_folder_not_listed_yet_has_its_count_and_a_blank_name`
Expected: FAIL.

**`rows_that_outlive_their_index_are_drawn_as_far_as_it_goes`** - in `crates/photon-ui/src/grid/view.rs` replace

```rust
                    let Some(section) = data.index.sections().get(row.section) else {
                        continue;
                    };
```

with

```rust
                    let section = &data.index.sections()[row.section];
```

Run: `cargo test -p photon-ui --lib rows_that_outlive_their_index_are_drawn_as_far_as_it_goes`
Expected: FAIL.

**`rows_that_outlive_their_index_are_drawn_as_far_as_it_goes`** - in `crates/photon-ui/src/grid/view.rs` replace

```rust
            && let Some(section) = data.index.sections().get(pinned.section)
        {
```

with

```rust
            && let Some(section) = Some(&data.index.sections()[pinned.section])
        {
```

Run: `cargo test -p photon-ui --lib rows_that_outlive_their_index_are_drawn_as_far_as_it_goes`
Expected: FAIL.

**`rows_are_drawn_on_whole_device_pixels_at_any_scale`** - in `crates/photon-ui/src/grid/view.rs` replace

```rust
        (position * scale).round() / scale
    } else {
```

with

```rust
        position * 1.0
    } else {
```

Run: `cargo test -p photon-ui --lib rows_are_drawn_on_whole_device_pixels_at_any_scale`
Expected: FAIL.

**`a_thumbnail_that_cannot_be_made_settles_as_its_mark`** - in `crates/photon-ui/src/thumbs/shown.rs` replace

```rust
                Err(LoadError::Failed(_)) => self.textures.fail(key),
```

with

```rust
                Err(LoadError::Failed(_)) => self.textures.put_off(key, now),
```

Run: `cargo test -p photon-ui --lib a_thumbnail_that_cannot_be_made_settles_as_its_mark`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

- [ ] **Step 8: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/src/grid crates/photon-ui/src/thumbs/shown.rs crates/photon-ui/src/lib.rs
git commit -m "feat(ui): the grid laid out, moved through and drawn, read-only

One function lays the published index out, takes the wheel, the keys and the scrollbar,
asks for the thumbnails the scroll is heading for, and draws the rows in view with the
header of the section at the top pinned over them. The place is kept across a resize as
the row at the top and the share of it scrolled past, found again before anything is
drawn - which is why placeIn, restoredTo and viewTop have no successor.

Tested in whole frames without a window. Ten rules probed, among them an index swapped
under rows built for another, a thumbnail that cannot be made, and a fractional display
scale.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 13: The application

**Files:**
- Create: `crates/photon-ui/src/events.rs`, `crates/photon-ui/src/thumbs/source.rs`, `crates/photon-ui/src/app.rs`, `crates/photon-ui/src/main.rs`, `crates/photon-ui/tests/app.rs`
- Modify: `crates/photon-ui/src/lib.rs`, `crates/photon-ui/Cargo.toml`

**Interfaces:**
- Consumes: `photon_engine::engine::{Engine, EngineConfig}` (`Engine::open(config, Arc<dyn Events>) -> Result<Arc<Engine>>`, `startup(self: &Arc<Self>, pictures: Option<PathBuf>)`, `published() -> (u64, Arc<GridIndex>, Option<String>, u64)`, `shutdown()`, the field `thumbs`); `photon_engine::events::{Events, LibraryChanged, ScanProgressEvent, FolderStatus, ExportProgress, FaceProgress}`; `photon_engine::commands::{list_folders, theme, grid_tile, set_visible}`; `ThumbService::{decoded, decode_file, request_async}` (Task 1); everything Tasks 2 to 12 produce.
- Produces: `events::Event`, `events::UiEvents::new(ctx) -> (UiEvents, Receiver<Event>)`; `thumbs::source::EngineThumbs(pub Arc<Engine>)`; `app::App` with `App::new(cc: &eframe::CreationContext<'_>, dirs: Dirs, pictures: Option<PathBuf>) -> Result<Self, String>`, `photos() -> usize`, `last_frame() -> Option<&GridOutput>`, `folders() -> &HashMap<i64, Folder>`; the binary `photon-native`.

`App::new` takes the Pictures folder as an argument because `Engine::startup` watches it by itself in a library that watches nothing, and a test must never scan the real one.

- [ ] **Step 1: `crates/photon-ui/src/events.rs`**

```rust
//! The engine's events on their way to the UI thread.
//!
//! The engine reports from its own threads - a scan, a rebuild, a pass. Each report is
//! put on a channel and the UI is asked for a frame, in which it takes them. The channel
//! has no bound and the UI empties it every frame, so a sender never waits.

use eframe::egui;
use photon_engine::events::{
    Events, ExportProgress, FaceProgress, FolderStatus, LibraryChanged, ScanProgressEvent,
};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Library(LibraryChanged),
    Scan(ScanProgressEvent),
    Folder(FolderStatus),
    Export(ExportProgress),
    Face(FaceProgress),
}

pub struct UiEvents {
    events: Sender<Event>,
    ctx: egui::Context,
}

impl UiEvents {
    pub fn new(ctx: egui::Context) -> (Self, Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        (Self { events, ctx }, receiver)
    }

    fn send(&self, event: Event) {
        // A UI that has gone has nobody to draw for.
        if self.events.send(event).is_ok() {
            self.ctx.request_repaint();
        }
    }
}

impl Events for UiEvents {
    fn library_changed(&self, event: LibraryChanged) {
        self.send(Event::Library(event));
    }
    fn scan_progress(&self, event: ScanProgressEvent) {
        self.send(Event::Scan(event));
    }
    fn folder_status(&self, event: FolderStatus) {
        self.send(Event::Folder(event));
    }
    fn export_progress(&self, event: ExportProgress) {
        self.send(Event::Export(event));
    }
    fn face_progress(&self, event: FaceProgress) {
        self.send(Event::Face(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    /// A context that counts how often it is asked for a frame.
    fn counting() -> (egui::Context, Arc<AtomicUsize>) {
        let ctx = egui::Context::default();
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = asked.clone();
        ctx.set_request_repaint_callback(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        (ctx, asked)
    }

    #[test]
    fn an_event_reaches_the_ui_thread_and_asks_for_a_frame() {
        let (ctx, asked) = counting();
        let (events, receiver) = UiEvents::new(ctx);
        let changed = LibraryChanged {
            version: 3,
            len: 10,
            data_changed: true,
        };
        events.library_changed(changed);
        assert_eq!(receiver.try_recv(), Ok(Event::Library(changed)));
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_event_for_a_ui_that_has_gone_is_dropped() {
        let (ctx, asked) = counting();
        let (events, receiver) = UiEvents::new(ctx);
        drop(receiver);
        events.folder_status(FolderStatus {
            watched_id: 1,
            online: true,
            degraded: false,
        });
        assert_eq!(asked.load(Ordering::SeqCst), 0);
    }
}
```

- [ ] **Step 2: `crates/photon-ui/src/thumbs/source.rs`**

*The two calls into Task 1's functions were not compiled as written;* the scratch build stood in for them with a local decode. Everything else here was.

```rust
//! The engine as the place thumbnails come from.

use super::{
    loader::{Building, LoadError, ThumbSource},
    textures::Pixels,
};
use photon_core::thumbs::{ThumbService, ThumbSize};
use photon_engine::engine::Engine;
use std::sync::Arc;

pub struct EngineThumbs(pub Arc<Engine>);

impl ThumbSource for EngineThumbs {
    fn cached(&self, key: u64) -> Option<Pixels> {
        let image = self.0.thumbs.decoded(key, ThumbSize::Grid).ok()?;
        Some(pixels(image.width(), image.height(), image.into_raw()))
    }

    fn build(&self, id: i64) -> Building<'_> {
        Box::pin(async move {
            let thumbs = &self.0.thumbs;
            let path = thumbs
                .request_async(id, ThumbSize::Grid)
                .await
                .map_err(load_error)?;
            let image = ThumbService::decode_file(&path).map_err(load_error)?;
            Ok(pixels(image.width(), image.height(), image.into_raw()))
        })
    }
}

fn pixels(width: u32, height: u32, rgba: Vec<u8>) -> Pixels {
    Pixels {
        width,
        height,
        rgba,
    }
}

/// A thumbnail that cannot be made is a failure; everything else - not built in time, a
/// photo gone since the grid named it, a cache file that will not read - may be there next
/// time.
fn load_error(err: photon_core::Error) -> LoadError {
    match err {
        photon_core::Error::ThumbFailed(message) => LoadError::Failed(message),
        _ => LoadError::Unavailable,
    }
}
```

- [ ] **Step 3: `crates/photon-ui/src/app.rs`**

```rust
//! The application: the engine, the grid, and the order of a frame.

use crate::{
    dirs::Dirs,
    events::{Event, UiEvents},
    grid::{
        view::{GridData, GridOutput, GridView},
        visible::VisibleReport,
    },
    icons,
    tasks::Latest,
    theme::{
        self,
        apply::{color, palette},
    },
    thumbs::{loader::Loader, shown::Thumbs, source::EngineThumbs},
};
use eframe::egui;
use jiff::tz::TimeZone;
use photon_core::{
    grid::GridIndex,
    library::{Folder, GridTile, ThemeChoice},
};
use photon_engine::{
    commands,
    engine::{Engine, EngineConfig},
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, sync::mpsc::Receiver, time::Duration};

/// How long a thumbnail that has not been built is waited for, as `protocol.rs`'s
/// `THUMB_TIMEOUT` bounds the same wait.
const THUMB_TIMEOUT: Duration = Duration::from_secs(30);
/// Threads decoding cached thumbnails. A decode is a tenth of a millisecond; two keep up
/// with a scroll and leave the cores to the engine's renders.
const DECODERS: usize = 2;

/// What is read from the settings table at launch.
#[derive(Clone, Copy, Debug, Default)]
struct Settings {
    theme: ThemeChoice,
    tile: GridTile,
}

pub struct App {
    engine: Arc<Engine>,
    events: Receiver<Event>,
    view: GridView,
    thumbs: Thumbs,
    /// The grid as last published. Read from the engine when it says the library changed,
    /// never per frame: `published` takes the engine's lock.
    index: Arc<GridIndex>,
    layout_gen: u64,
    folders: HashMap<i64, Folder>,
    folder_list: Latest<(), Option<Vec<Folder>>>,
    settings: Latest<(), Settings>,
    size: GridTile,
    zone: TimeZone,
    visible: VisibleReport,
    last: Option<GridOutput>,
    closed: bool,
}

impl App {
    /// Opens the library in `dirs` and starts the engine. `pictures` is the folder a
    /// library that watches nothing starts by watching (`Engine::startup`).
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        dirs: Dirs,
        pictures: Option<PathBuf>,
    ) -> Result<Self, String> {
        let ctx = cc.egui_ctx.clone();
        theme::apply::install(&ctx);
        icons::install(&ctx);
        theme::fonts::install(&ctx);

        let (events, receiver) = UiEvents::new(ctx.clone());
        let config = EngineConfig {
            db_path: dirs.db_path,
            cache_dir: dirs.cache_dir,
            workers: photon_core::thumbs::default_workers(),
        };
        let engine = Engine::open(config, Arc::new(events))
            .map_err(|err| format!("could not open the photon library: {err}"))?;
        engine.startup(pictures);

        let repaint = |ctx: &egui::Context| {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        let loader = Loader::spawn(
            Arc::new(EngineThumbs(engine.clone())),
            DECODERS,
            THUMB_TIMEOUT,
            repaint(&ctx),
        );
        let mut folder_list = Latest::spawn(
            "folders",
            {
                let engine = engine.clone();
                move |()| {
                    commands::list_folders(&engine)
                        .ok()
                        .map(|list| list.folders)
                }
            },
            repaint(&ctx),
        );
        let mut settings = Latest::spawn(
            "settings",
            {
                let engine = engine.clone();
                move |()| Settings {
                    theme: commands::theme(&engine).unwrap_or_default(),
                    tile: commands::grid_tile(&engine).unwrap_or_default(),
                }
            },
            repaint(&ctx),
        );
        folder_list.ask(());
        settings.ask(());

        let (_, index, _, layout_gen) = engine.published();
        Ok(Self {
            engine,
            events: receiver,
            view: GridView::default(),
            thumbs: Thumbs::new(loader),
            index,
            layout_gen,
            folders: HashMap::new(),
            folder_list,
            settings,
            size: GridTile::default(),
            zone: TimeZone::system(),
            visible: VisibleReport::default(),
            last: None,
            closed: false,
        })
    }

    /// How many photos the grid holds.
    pub fn photos(&self) -> usize {
        self.index.len()
    }

    /// What the last frame of the grid came to.
    pub fn last_frame(&self) -> Option<&GridOutput> {
        self.last.as_ref()
    }

    /// The folders the headers are named from.
    pub fn folders(&self) -> &HashMap<i64, Folder> {
        &self.folders
    }

    /// Takes what the engine has reported since the last frame. Only a changed library
    /// is acted on in this slice; the rest is taken so the channel stays empty.
    fn take_events(&mut self) {
        let mut changed = false;
        let mut data_changed = false;
        for event in self.events.try_iter() {
            if let Event::Library(library) = event {
                changed = true;
                data_changed |= library.data_changed;
            }
        }
        if changed {
            let (_, index, _, layout_gen) = self.engine.published();
            self.index = index;
            self.layout_gen = layout_gen;
        }
        // A folder renamed, added or given an alias: the headers are named from the list.
        if data_changed {
            self.folder_list.ask(());
        }
    }

    fn take_answers(&mut self, ctx: &egui::Context) {
        if let Some(Ok(Some(folders))) = self.folder_list.answer() {
            self.folders = folders
                .into_iter()
                .map(|folder| (folder.id, folder))
                .collect();
        }
        if let Some(Ok(settings)) = self.settings.answer() {
            self.size = settings.tile;
            theme::apply::choose(ctx, settings.theme);
        }
    }

    fn close(&mut self) {
        if !std::mem::replace(&mut self.closed, true) {
            self.engine.shutdown();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.take_events();
        self.take_answers(ui.ctx());

        let data = GridData {
            layout_gen: self.layout_gen,
            index: &self.index,
            folders: &self.folders,
            size: self.size,
            zone: &self.zone,
        };
        let surface = color(palette(ui.ctx()).surface);
        let (view, thumbs) = (&mut self.view, &mut self.thumbs);
        let mut output = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(surface))
            .show(ui, |ui| output = Some(view.show(ui, &data, thumbs)));

        let now = ui.input(|input| input.time) * 1000.0;
        if let Some(output) = &output {
            if let Some(ids) = self.visible.update(&output.on_screen, now) {
                commands::set_visible(&self.engine, ids);
            }
            if let Some(at) = self.visible.due_at() {
                let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
                ui.ctx().request_repaint_after(wait);
            }
        }
        self.last = output;
    }

    /// What shows where nothing is painted, for the frame before the first one of ours.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self) {
        self.close();
    }
}

/// eframe calls `on_exit` when the window closes; a test, or a start that fails after the
/// engine opened, only drops. Either way the engine is shut down once.
impl Drop for App {
    fn drop(&mut self) {
        self.close();
    }
}
```

- [ ] **Step 4: `crates/photon-ui/src/main.rs`, and the binary**

```rust
// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use photon_ui::{
    app::App,
    args::{Args, USAGE},
    dirs::IDENTIFIER,
};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let Some(dirs) = args.dirs() else {
        eprintln!("photon could not find a directory to keep its library in");
        return ExitCode::FAILURE;
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("photon")
            // What a Wayland compositor and a taskbar know the window by.
            .with_app_id(IDENTIFIER)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_fullscreen(args.fullscreen),
        ..Default::default()
    };
    let run = eframe::run_native(
        "photon",
        options,
        Box::new(move |cc| {
            tracing::info!(
                adapter = ?cc.wgpu_render_state.as_ref().map(|state| state.adapter.get_info()),
                "started"
            );
            match App::new(cc, dirs, ::dirs::picture_dir()) {
                Ok(app) => Ok(Box::new(app) as Box<dyn eframe::App>),
                Err(message) => Err(message.into()),
            }
        }),
    );
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "photon could not start");
            eprintln!("photon could not start: {err}");
            ExitCode::FAILURE
        }
    }
}
```

In `crates/photon-ui/Cargo.toml`, after `[package]`'s fields:

```toml
# `photon` is photon-app's binary until the switch-over; this one is named apart so both
# can be built side by side.
[[bin]]
name = "photon-native"
path = "src/main.rs"
```

- [ ] **Step 5: `crates/photon-ui/src/lib.rs`, whole, with the tripwire**

```rust
//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod app;
pub mod args;
pub mod dirs;
pub mod events;
pub mod grid {
    pub mod header;
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod tile;
    pub mod view;
    pub mod visible;
}
pub mod icons;
pub mod tasks;
pub mod text;
pub mod theme {
    pub mod apply;
    pub mod fonts;
    pub mod tokens;
}
pub mod thumbs {
    pub mod loader;
    pub mod shown;
    pub mod source;
    pub mod textures;
}

#[cfg(test)]
mod tests {
    /// The state modules, with their source.
    const STATE_MODULES: [(&str, &str); 11] = [
        ("args.rs", include_str!("args.rs")),
        ("dirs.rs", include_str!("dirs.rs")),
        ("tasks.rs", include_str!("tasks.rs")),
        ("theme/tokens.rs", include_str!("theme/tokens.rs")),
        ("grid/labels.rs", include_str!("grid/labels.rs")),
        ("grid/layout.rs", include_str!("grid/layout.rs")),
        ("grid/motion.rs", include_str!("grid/motion.rs")),
        ("grid/scroll.rs", include_str!("grid/scroll.rs")),
        ("grid/visible.rs", include_str!("grid/visible.rs")),
        ("thumbs/loader.rs", include_str!("thumbs/loader.rs")),
        ("thumbs/textures.rs", include_str!("thumbs/textures.rs")),
    ];

    // A state module that reaches for egui can no longer be tested without a context, and
    // the next one copies it. Comments may name egui; code may not.
    #[test]
    fn state_modules_name_no_egui_type() {
        for (name, source) in STATE_MODULES {
            for (number, line) in source.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                assert!(
                    !code.contains("egui") && !code.contains("eframe"),
                    "{name}:{}: a state module names egui: {code}",
                    number + 1
                );
            }
        }
    }
}
```

- [ ] **Step 6: `crates/photon-ui/tests/app.rs`**

```rust
//! The whole slice without a window: a library on disk, the engine, and frames of the app.

use eframe::egui::vec2;
use photon_ui::{app::App, dirs};
use std::{
    io::Cursor,
    path::Path,
    time::{Duration, Instant},
};

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        height,
        image::Rgb([90, 120, 200]),
    ));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .unwrap();
    bytes
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Steps the app until `done`, or panics after thirty seconds saying what it waited for.
fn until(harness: &mut egui_kittest::Harness<'_, App>, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        harness.step();
        if done(harness.state()) {
            return;
        }
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_library_is_opened_scanned_and_shown_with_its_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("Pictures");
    // Different sizes, so the three files differ in bytes and none is another's copy.
    write(&photos.join("coast").join("a.jpg"), &jpeg(64, 32));
    write(&photos.join("coast").join("b.jpg"), &jpeg(48, 32));
    write(&photos.join("hills").join("c.jpg"), &jpeg(32, 64));
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));

    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(800.0, 600.0))
        .build_eframe(|cc| App::new(cc, dirs.clone(), Some(photos.clone())).unwrap());

    // The engine watches the folder it was given, scans it and publishes a grid; the app
    // hears of it and reads it.
    until(&mut harness, "three photos in the grid", |app| {
        app.photos() == 3
    });
    until(&mut harness, "the folder list read", |app| {
        app.folders().len() >= 2
    });
    let names: Vec<&str> = {
        let mut names: Vec<&str> = harness
            .state()
            .folders()
            .values()
            .map(|folder| folder.name.as_str())
            .collect();
        names.sort_unstable();
        names
    };
    assert!(
        names.contains(&"coast") && names.contains(&"hills"),
        "{names:?}"
    );

    // Nothing was cached: each thumbnail is waited for, built by the engine and uploaded.
    until(&mut harness, "every tile has its picture", |app| {
        app.last_frame()
            .is_some_and(|frame| frame.settled && frame.on_screen.len() == 3)
    });
    assert!(dirs.db_path.is_file());
}
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p photon-ui`
Expected: 119 unit tests pass (2 in `events` and the tripwire are new), `tests/app.rs` passes in under a second, and `cargo build -p photon-ui` produces `photon-native`. **Build it; do not run it.**

If `source.rs` does not compile, the cause is in the two lines that call `decoded` and `decode_file`: check their names and signatures against what Task 1 landed.

- [ ] **Step 8: Probes**

**`an_event_reaches_the_ui_thread_and_asks_for_a_frame`** - in `crates/photon-ui/src/events.rs` replace

```rust
            self.ctx.request_repaint();
```

with

```rust
            let _ = &self.ctx;
```

Run: `cargo test -p photon-ui --lib an_event_reaches_the_ui_thread_and_asks_for_a_frame`
Expected: FAIL.

**`state_modules_name_no_egui_type`** - in `crates/photon-ui/src/grid/scroll.rs` replace

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
```

with

```rust
#[allow(unused_imports)]
use eframe::egui as _egui;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
```

Run: `cargo test -p photon-ui --lib state_modules_name_no_egui_type`
Expected: FAIL.

**`a_library_is_opened_scanned_and_shown_with_its_pictures`** - in `crates/photon-ui/src/app.rs` replace

```rust
            self.index = index;
            self.layout_gen = layout_gen;
```

with

```rust
            let _ = (index, layout_gen);
```

Run: `cargo test -p photon-ui --test app a_library_is_opened_scanned_and_shown_with_its_pictures`
Expected: FAIL.

After each: put the original back, `touch` the file, and when all are done run the task's tests again. Expected: PASS.

`app publish` fails by the end-to-end test's thirty-second deadline, so this probe takes that long.

- [ ] **Step 9: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
git add crates/photon-ui Cargo.lock
git commit -m "feat(ui): photon-native opens the library and shows its grid

The application owns the engine and orders a frame: take the engine's events, take the
tasks' answers, draw the grid, tell the thumbnail queue what is on screen. The published
index is read when the engine says the library changed and never per frame; the folder
list and the stored theme and tile size are read by tasks. The engine is shut down once,
by on_exit or by Drop.

tests/app.rs runs the whole of it without a window: a library on disk, the engine's own
scan and thumbnail renders, frames of the application until every tile has its picture.
A test reads the state modules' source and fails on one that names egui.

Probed: an event that asks for no frame, a state module importing egui, and a changed
library whose index is not taken.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 14: The grid as a picture

**Files:**
- Create: `crates/photon-ui/tests/screenshots.rs`, `crates/xtask/src/native_shot.rs`
- Modify: `crates/xtask/src/main.rs`

**Interfaces:**
- Consumes: `app::App`, `dirs::within`, `photon_core::library::{Library, ThemeChoice}`; the CC0 photos in `crates/xtask/screenshots/photos`.
- Produces: `cargo run -p xtask -- native-shot`, which writes `target/screenshots/native-grid-light.png` and `native-grid-dark.png`.

`egui_kittest`'s wgpu renderer draws off screen with no display; this was tried on this machine while the plan was written and works. The test is `#[ignore]`d because a CI runner need not have a GPU adapter.

- [ ] **Step 1: `crates/photon-ui/tests/screenshots.rs`**

```rust
//! The grid as it is drawn, written to PNG without a window: the application itself, over
//! a library made of the CC0 photos the Svelte screenshots use (credited in their
//! `CREDITS.md`), rendered off screen through wgpu.
//!
//! Ignored by default - it needs a GPU adapter, which a CI runner need not have - and run
//! by `cargo run -p xtask -- native-shot`. Like `xtask screenshots` it replaces no item of
//! the smoke checklist: it is one renderer's picture, read by whoever runs it.
//!
//! Three of the folders are named in other scripts on purpose. What their headers show is
//! what the pull request reports about text (`src/text.rs`).

use eframe::egui::vec2;
use photon_core::library::{Library, ThemeChoice};
use photon_ui::{app::App, dirs};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The folders of the library, and how many of the photos each takes.
const FOLDERS: [(&str, usize); 4] = [
    ("2026-07 Coast", 7),
    ("東京 2024 桜", 5),
    ("رحلة 2024 الصيف", 5),
    ("🎉 Party שלום abc", 6),
];

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Copies the photos into the four folders under `pictures`.
fn library(pictures: &Path) {
    let source = manifest().join("../xtask/screenshots/photos");
    let mut photos: Vec<PathBuf> = std::fs::read_dir(&source)
        .unwrap_or_else(|err| panic!("{}: {err}", source.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    photos.sort();
    let mut photos = photos.into_iter();
    for (folder, count) in FOLDERS {
        let folder = pictures.join(folder);
        std::fs::create_dir_all(&folder).unwrap();
        for photo in photos.by_ref().take(count) {
            std::fs::copy(&photo, folder.join(photo.file_name().unwrap())).unwrap();
        }
    }
}

fn shot(theme: ThemeChoice, name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    library(&pictures);
    let dirs = dirs::within(&dir.path().join("data"), &dir.path().join("cache"));
    // The theme is the stored one, read the way the application reads it: a harness has
    // no desktop to follow.
    std::fs::create_dir_all(dirs.db_path.parent().unwrap()).unwrap();
    Library::open(&dirs.db_path)
        .unwrap()
        .set_theme(theme)
        .unwrap();

    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(1280.0, 1000.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(|cc| App::new(cc, dirs, Some(pictures)).unwrap());

    let total: usize = FOLDERS.iter().map(|(_, count)| count).sum();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        harness.step();
        let app = harness.state();
        let shown = app.last_frame().is_some_and(|frame| frame.settled);
        if app.photos() == total && app.folders().len() >= FOLDERS.len() && shown {
            break;
        }
        assert!(Instant::now() < deadline, "the library never came up");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The icons are rasterised a frame after they are first asked for.
    for _ in 0..5 {
        harness.step();
    }

    let out = manifest().join("../../target/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join(name);
    harness.render().unwrap().save(&path).unwrap();
    println!("wrote {}", path.display());
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_grid_light() {
    shot(ThemeChoice::Light, "native-grid-light.png");
}

#[test]
#[ignore = "renders through a GPU adapter: cargo run -p xtask -- native-shot"]
fn native_grid_dark() {
    shot(ThemeChoice::Dark, "native-grid-dark.png");
}
```

- [ ] **Step 2: `crates/xtask/src/native_shot.rs`**

```rust
//! `cargo run -p xtask -- native-shot`: the native grid as two PNGs, light and dark, in
//! `target/screenshots/`.
//!
//! The rendering is a test of photon-ui (`tests/screenshots.rs`), ignored by default and
//! run from here, so that xtask does not itself depend on a GPU stack. Like `screenshots`
//! it is not run in CI and replaces no item of the smoke checklist.

use std::{
    path::Path,
    process::{Command, ExitCode},
};

pub fn run(root: &Path) -> ExitCode {
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "test",
            "-p",
            "photon-ui",
            "--test",
            "screenshots",
            "--",
            "--ignored",
            "--nocapture",
        ])
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("{}", root.join("target").join("screenshots").display());
            ExitCode::SUCCESS
        }
        Ok(_) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("cannot run cargo: {err}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 3: Wire it into `crates/xtask/src/main.rs`**

Add to the usage comment at the top, after the `scroll-probe` line:

```rust
//!   cargo run -p xtask -- native-shot
```

Add `mod native_shot;` after `mod checks;`, the arm

```rust
        Some("native-shot") => native_shot::run(&repo_root()),
```

after the `scroll-probe` arm, and name the command in the message for an unknown one:

```rust
                "unknown command {other:?}; expected `versions`, `metadata`, `screenshots`, `scroll-probe` or `native-shot`"
```

- [ ] **Step 4: Render, and read the pictures**

Run: `cargo run -p xtask -- native-shot`
Expected: two `wrote ...png` lines and the directory's path, in a few seconds.

**Open both PNGs and look at them** (the Read tool shows an image). Check, and write what you find into the report for the pull request:

- the light one is light and the dark one dark;
- four headers, each with its count, a month, and the path;
- `2026-07 Coast` is in a heavier weight than its count;
- `東京 2024 桜` is drawn in Japanese, not as boxes;
- `رحلة 2024 الصيف` reads, from the left: `الصيف`, `2024`, `رحلة`, with the letters of each word joined;
- `🎉 Party שלום abc` reads, from the left: the emoji, `Party`, `שלום`, `abc`;
- every tile is a square with a photo covering it, and rounded corners.

A header that is boxes means this machine has no font for that script: say which, and do not call it a failure of the code. The emoji are drawn in one colour; that is egui's own emoji font, and a finding to report, not to fix here.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-ui/tests/screenshots.rs crates/xtask/src/native_shot.rs crates/xtask/src/main.rs
git commit -m "feat(xtask): native-shot writes the native grid to PNG without a display

The application itself, over a library of the CC0 photos in folders named in Latin,
Japanese, Arabic and a mix with Hebrew and an emoji, rendered off screen through wgpu.
An ignored test of photon-ui that xtask runs, so xtask takes no GPU stack. Not in CI, and
no replacement for any item of the smoke checklist.

No test of its own: it is the thing that is looked at.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 15: Say what is here

**Files:**
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: everything above.
- Produces: the section a later sub-project's implementer reads first.

- [ ] **Step 1: Add the section to `CLAUDE.md`**

After the "Styling" section and before "Conventions", add:

```markdown
### The native UI (branch `native-ui`)

`crates/photon-ui` is the interface being rebuilt in Rust on egui and wgpu, in place of
`ui/` and the Tauri shell (spec `2026-10-09-photon-native-ui-design.md`). Its binary is
`photon-native` until the switch-over. **It is not launched to verify a change either**:
`cargo run -p xtask -- native-shot` writes the grid to `target/screenshots/native-grid-*.png`
off screen, and those are read.

Two kinds of module, and `state_modules_name_no_egui_type` holds the line between them:
*state modules* are plain Rust tested without a context (`grid/layout.rs`, `motion.rs`,
`scroll.rs`, `thumbs/loader.rs`, `textures.rs`, `tasks.rs`), and *views* draw one and turn
input into calls on it, tested in whole frames without a window (`Context::run_ui`, or
`egui_kittest` where the application itself is run, as `tests/app.rs` does).

`eframe` is pinned to an exact version and its API moves between minors: read the pinned
version's source, never an older egui from memory. `eframe::App` is `fn ui(&mut self, ui:
&mut egui::Ui, ..)`.

**Nothing that reads SQLite runs on the UI thread.** It goes through `tasks::Latest`, which
answers only the latest question. `Engine::published` and the `GridIndex` are in memory and
are read directly - `published` when the engine says the library changed, not per frame.

**The grid has no scroll map.** Its position is an `f64` in the layout's coordinates and
the rows are drawn at `row.top - position`; egui's `ScrollArea` is not used for it, because
its offset is an `f32`. Several rules of the Svelte grid have no successor (`placeIn`,
`viewTop`, pages, mounting): the spec of the slice lists them so they are not ported.

**A thumbnail not built yet is waited for, never blocked on** (`thumbs/loader.rs`): one
thread polls every pending `request_async`, as `protocol.rs` learned to.

**Text goes through `text::paint_line`**, not through egui directly, wherever it can be in
another script: egui shapes text but runs no bidirectional algorithm, and draws a name that
mixes directions wrong. A text *field* is not covered.

A screenshot's theme is the stored setting, written into the library before the
application opens it: a harness has no desktop to follow.
```

- [ ] **Step 2: Gate and commit**

```bash
cargo fmt --all --check
cargo test --workspace
git add CLAUDE.md
git commit -m "docs: the native UI's rules, where the next sub-project will look for them

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 3: Report for the pull request**

Hand the controller, for the pull request into `native-ui`:

- what Task 14's two pictures show, item by item;
- the new direct dependencies and their licences, since `THIRD-PARTY-NOTICES.md` is not touched until `photon-native` ships (sub-project 7): `eframe`, `egui_extras`, `egui_kittest` (MIT OR Apache-2.0), `fastframe-fonts` (MIT, a git dependency pinned to a tag), `unicode-bidi` (MIT OR Apache-2.0), and that `eframe`'s `default_fonts` compiles four fonts into the binary whose licences will have to ship;
- the three known differences from the Svelte grid a person would notice: dates in English, emoji in one colour, no shadow under a tile's marks;
- that a text field in mixed-direction text is untested and unsolved;
- that the gate has not been run and is the next plan.
