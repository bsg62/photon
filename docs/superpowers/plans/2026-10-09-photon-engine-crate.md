# The engine in a crate of its own - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `Engine`, the command layer and the watcher live in a new crate, `photon-engine`, that does not depend on Tauri; `photon-app` depends on it and behaves exactly as before.

**Architecture:** Seven files move from `crates/photon-app/src/` to `crates/photon-engine/src/` with `git mv`. `photon-app`'s `lib.rs` re-exports the moved modules, so no path inside the files that stay changes. Three references that point from the moved files into the staying ones are turned round. A test reads the new crate's manifest and fails if a UI runtime appears in it.

**Tech Stack:** Rust (edition 2024), Cargo workspaces and features. No new dependency.

**Spec:** `docs/superpowers/specs/2026-10-09-photon-engine-crate-design.md` - read it first. It lives on the `native-ui` branch; read it there (`git show native-ui:docs/superpowers/specs/2026-10-09-photon-engine-crate-design.md`).

## Global Constraints

- Read `CLAUDE.md` before starting. Its rules bind every task.
- **This work is on a branch of `main`, not on `native-ui`.** Start with `git switch main && git pull && git switch -c refactor/engine-crate`.
- **It is a move.** No behaviour changes. No command gains or loses a parameter. `serde`'s camelCase stays on every struct. No release is cut for it.
- **The Rust gate before every commit:** `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate before the last commit:** `npm run check`, `npm test` (a comment in `api.ts` changes).
- `photon-engine` names none of `tauri`, `tauri-*`, `tokio`, `tiny_http`, `wry`, `tao` as a dependency.
- Crate-private items stay crate-private. If the compiler shows a staying file needs an item that is not `pub` today, do not widen it in passing: stop, and list it in the pull request description with the reason.
- **Every new test is shown to fail with its change reverted**, and the commit message says how.
- Never launch the GUI.
- Comments carry the reasoning, in the surrounding code's density and voice. No em dashes in comments or docs; the codebase uses " - ".
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Do not push; the controller does.

## Review Focus

What a move can break without a test noticing; each has its check in the task named.

1. **A test silently lost in the move** (a `mod tests` no longer compiled, a `#[cfg(test)]` that no longer holds): the list of test names is the same before and after. Task 1, steps 1 and 9.
2. **`photon-engine` built on its own, without the `test-support` feature**, has unused test helpers or missing items that the workspace build hides, because Cargo unifies features across the workspace. Task 1, step 8: `cargo clippy -p photon-engine -- -D warnings` and `cargo test -p photon-engine`, each alone.
3. **`photon-app` tested on its own** does not get `testutil`. Task 1, step 8: `cargo test -p photon-app` alone.
4. **Two render locks instead of one** (a second `RENDERING` left behind in `protocol.rs`): an export and the viewer would render at once. Task 1, step 6 ends with a grep that finds exactly one definition.
5. **The release build path** (`tauri build`, with the `custom-protocol` feature) and the other two platforms' `cfg` dependencies for `memory.rs`. Not runnable here; Task 2's last step names the CI jobs that cover them, and the pull request waits for them.

## File Structure

- `crates/photon-engine/Cargo.toml` (new) - the crate, its `test-support` feature, its share of the dependencies.
- `crates/photon-engine/src/lib.rs` (new) - the module list, and the manifest test.
- `crates/photon-engine/src/{engine,commands,watch,events,memory,error,testutil}.rs` - moved, with three edits.
- `crates/photon-app/Cargo.toml` - depends on `photon-engine`; loses what only the moved files used.
- `crates/photon-app/src/lib.rs` - re-exports.
- `crates/photon-app/src/protocol.rs` - imports `RENDERING` and `parse_key` instead of defining them.
- `crates/photon-app/src/ipc.rs` - `media_base` calls the server itself.
- `Cargo.toml` - the workspace member.
- `CLAUDE.md`, `ui/src/lib/api.ts` - the paths they name.

---

### Task 1: Move the seven files

One commit: no intermediate state compiles.

**Files:**
- Create: `crates/photon-engine/Cargo.toml`, `crates/photon-engine/src/lib.rs`
- Move: `crates/photon-app/src/{engine,commands,watch,events,memory,error,testutil}.rs` to `crates/photon-engine/src/`
- Modify: `Cargo.toml`, `crates/photon-app/Cargo.toml`, `crates/photon-app/src/lib.rs`, `crates/photon-app/src/protocol.rs`, `crates/photon-app/src/ipc.rs`, and in their new place `engine.rs`, `commands.rs`, `events.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: the crate `photon-engine` with `pub mod commands, engine, error, events, watch`; `photon_engine::engine::RENDERING` (`pub static RENDERING: parking_lot::Mutex<()>`); `photon_engine::commands::parse_key` (`pub fn parse_key(key: &str) -> Option<u64>`); `photon_engine::testutil` behind `#[cfg(any(test, feature = "test-support"))]`. `sub-project 1` (the native UI) depends on exactly these names.

- [ ] **Step 1: Record the tests that exist now**

```bash
cargo test --workspace -- --list 2>/dev/null | grep ': test$' | sort > /tmp/tests-before.txt
wc -l /tmp/tests-before.txt
```

Expected: a count in the thousands. Keep the file; step 9 compares against it.

- [ ] **Step 2: Move the files**

```bash
mkdir -p crates/photon-engine/src
git mv crates/photon-app/src/engine.rs crates/photon-app/src/commands.rs \
       crates/photon-app/src/watch.rs crates/photon-app/src/events.rs \
       crates/photon-app/src/memory.rs crates/photon-app/src/error.rs \
       crates/photon-app/src/testutil.rs crates/photon-engine/src/
```

- [ ] **Step 3: Write `crates/photon-engine/Cargo.toml`**

Copy every version and feature list from `crates/photon-app/Cargo.toml` as it stands; the values below are the ones there on 2026-10-09. If one differs by the time this runs, the one in `photon-app` wins.

```toml
[package]
name = "photon-engine"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description.workspace = true
repository.workspace = true
authors.workspace = true

[features]
# `testutil` and the event recorder, for another crate's tests: photon-app's protocol and
# media-server tests build their fixtures with them. Inside this crate `cfg(test)` is enough.
test-support = ["dep:tempfile", "dep:image"]

[dependencies]
photon-core = { path = "../photon-core" }
parking_lot = "0.12.5"
serde = { version = "1.0.229", features = ["derive"] }
tracing = "0.1.44"
tempfile = { version = "3.27.0", optional = true }
image = { version = "0.25.10", default-features = false, features = ["jpeg"], optional = true }

# Settings' memory figure (`memory.rs`): the page size on Linux, the physical footprint on
# macOS; the process table and private working set on Windows.
[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = [
  "Win32_Foundation",
  "Win32_System_Diagnostics_ToolHelp",
  "Win32_System_ProcessStatus",
  "Win32_System_Threading",
] }

[dev-dependencies]
image = { version = "0.25.10", default-features = false, features = ["jpeg"] }
# Only to break a library on purpose: a rebuild that fails is the one thing a rollback test
# cannot get from the engine's own API. The same version and features photon-core builds.
rusqlite = { version = "0.40.2", features = ["bundled"] }
tempfile = "3.27.0"
serde_json = "1"
```

- [ ] **Step 4: Write `crates/photon-engine/src/lib.rs`**

```rust
//! photon-engine: `Engine`, the commands over it and the folder watcher. No UI is named
//! here, and no UI runtime is a dependency (`the_engine_depends_on_no_ui_runtime`), so more
//! than one shell can stand on it.

pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
mod memory;
pub mod watch;

#[cfg(any(test, feature = "test-support"))]
pub mod testutil;
```

- [ ] **Step 5: Add the member and rewire `photon-app`**

In the workspace `Cargo.toml`:

```toml
members = ["crates/photon-core", "crates/photon-engine", "crates/photon-app", "crates/xtask"]
```

In `crates/photon-app/Cargo.toml`, under `[dependencies]`, add after the `photon-core` line:

```toml
photon-engine = { path = "../photon-engine" }
```

and under `[dev-dependencies]`:

```toml
# `testutil::Fixture` and the event recorder, for the protocol's and the media server's tests.
photon-engine = { path = "../photon-engine", features = ["test-support"] }
```

Move the two `[target.'cfg(unix)'.dependencies]` and `[target.'cfg(windows)'.dependencies]` tables, with their comment, out of `photon-app` (they are in `photon-engine` now, step 3) **only if** `grep -rn "libc::\|windows_sys::" crates/photon-app/src` finds nothing. Leave every other dependency of `photon-app` where it is: `cargo build` does not say which are now unused, and removing one that a `cfg` hides breaks another platform.

Replace `crates/photon-app/src/lib.rs` with:

```rust
//! photon-app: the Tauri shell around photon-engine.

mod app;
mod ipc;
pub mod media_server;
pub mod protocol;
#[cfg(target_os = "linux")]
mod webkit;

// The engine's modules under the names they had while they lived here, so `crate::engine`,
// `crate::commands` and the rest read the same in every file of this crate.
pub use photon_engine::{commands, engine, error, events, watch};
#[cfg(test)]
pub(crate) use photon_engine::testutil;

pub use app::run;
```

- [ ] **Step 6: Turn the three references round**

**`RENDERING`.** Cut the static and its whole doc comment from `crates/photon-app/src/protocol.rs` (the block beginning `/// One full-size render at a time.` and ending with the `static RENDERING` line) and paste it into `crates/photon-engine/src/engine.rs`, directly above `pub struct EngineConfig`, changing only its visibility:

```rust
pub static RENDERING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
```

Then:

- `crates/photon-engine/src/engine.rs`: `crate::protocol::RENDERING.lock()` becomes `RENDERING.lock()`, and in the doc comment of the export, `` `protocol::RENDERING` `` becomes `` `RENDERING` ``.
- `crates/photon-engine/src/commands.rs`: `crate::protocol::RENDERING.lock()` becomes `crate::engine::RENDERING.lock()`.
- `crates/photon-app/src/protocol.rs`: change `use crate::engine::Engine;` to `use crate::engine::{Engine, RENDERING};`.

**`parse_key`.** Cut the function and its doc comment from `protocol.rs` and paste it into `crates/photon-engine/src/commands.rs`, directly above `pub fn media_base`, as:

```rust
/// A thumbnail key from its hex, only in the exact spelling `hex_key` gives it. A looser
/// parse would read `+1` as key 1 and cache key 1's picture under a URL the UI never asks for.
pub fn parse_key(key: &str) -> Option<u64> {
    u64::from_str_radix(key, 16)
        .ok()
        .filter(|&parsed| hex_key(parsed) == key)
}
```

(`hex_key` is already imported in `commands.rs`.) In `commands.rs` both `crate::protocol::parse_key(key)` become `parse_key(key)`. In `protocol.rs` add `use crate::commands::parse_key;`.

**`media_base`.** Delete `pub fn media_base` from `commands.rs`. In `crates/photon-app/src/ipc.rs` the wrapper becomes:

```rust
pub fn media_base(server: State<'_, crate::media_server::MediaServer>) -> Result<String, AppError> {
    Ok(server.base_url())
}
```

keeping its attribute and any comment above it.

Check there is one lock:

```bash
grep -rn "static RENDERING" crates/
```

Expected: exactly one line, in `crates/photon-engine/src/engine.rs`.

- [ ] **Step 7: Open the test helpers to the feature**

In `crates/photon-engine/src/events.rs`, each of the four `#[cfg(test)]` attributes on `Recorded`, `Recorder`, `impl Recorder` and `impl Events for Recorder` becomes:

```rust
#[cfg(any(test, feature = "test-support"))]
```

Nothing else in the moved files changes its `cfg`: `hold_ini_write`, `occupy_scan_slot_for_test`, `TestScanSlot`, `counts_computed`, `pending_dirs` and `pending_ini` are used only by tests inside `photon-engine` and stay `#[cfg(test)]`.

- [ ] **Step 8: Build each crate alone, then together**

```bash
cargo fmt --all
cargo clippy -p photon-engine -- -D warnings
cargo test -p photon-engine
cargo test -p photon-app
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all pass. The first three are the ones the workspace build cannot stand in for: Cargo unifies features across a workspace build, so `test-support` is on there whoever asked for it.

If the compiler reports a private item used from `photon-app`, follow the Global Constraint: do not widen it. Report it.

- [ ] **Step 9: Show no test was lost**

```bash
cargo test --workspace -- --list 2>/dev/null | grep ': test$' | sort > /tmp/tests-after.txt
diff /tmp/tests-before.txt /tmp/tests-after.txt && echo SAME
```

Expected: `SAME`. Test names carry their module path and not their crate, so a moved test has the name it had. Any line in the diff is a test lost or renamed: find out which before going on.

- [ ] **Step 10: Run the gate and commit**

```bash
cargo fmt --all --check
cargo bench -p photon-core --bench grid --no-run
cargo run -p xtask -- versions
cargo run -p xtask -- metadata
git add -A crates/photon-engine crates/photon-app Cargo.toml Cargo.lock
git commit -m "refactor: the engine, its commands and the watcher in a crate of their own

photon-engine holds what no UI runtime is needed for; photon-app re-exports its modules
under the names they had, so no path in the Tauri shell changes. Three references pointed
the other way: RENDERING and parse_key move with the engine, and media_base, one line over
the webview's media server, stays behind in ipc.rs.

A move: the workspace's test names are the same before and after (compared by list).

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Keep it free of UI runtimes, and say where things are

**Files:**
- Modify: `crates/photon-engine/src/lib.rs`, `CLAUDE.md`, `ui/src/lib/api.ts`

**Interfaces:**
- Consumes: the crate from Task 1.
- Produces: the test `the_engine_depends_on_no_ui_runtime`.

- [ ] **Step 1: Write the test**

Append to `crates/photon-engine/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    /// The names a dependency table gives its entries: what stands before the `=`, or
    /// between `[dependencies.` and `]`.
    fn dependency_names(manifest: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut in_dependencies = false;
        for line in manifest.lines().map(str::trim) {
            if let Some(header) = line.strip_prefix('[') {
                let header = header.trim_end_matches(']');
                in_dependencies = header.ends_with("dependencies");
                if let Some((table, name)) = header.rsplit_once('.')
                    && table.ends_with("dependencies")
                {
                    names.push(name.to_owned());
                }
                continue;
            }
            if in_dependencies && let Some((name, _)) = line.split_once('=') {
                names.push(name.trim().to_owned());
            }
        }
        names
    }

    /// The reason this crate exists: a shell that is not Tauri can depend on it. One of
    /// these in the manifest and every such shell builds a webview's runtime again.
    #[test]
    fn the_engine_depends_on_no_ui_runtime() {
        let names = dependency_names(include_str!("../Cargo.toml"));
        assert!(names.iter().any(|name| name == "photon-core"), "{names:?}");
        for name in &names {
            let ui_runtime = name.starts_with("tauri")
                || ["tokio", "tiny_http", "wry", "tao"].contains(&name.as_str());
            assert!(!ui_runtime, "photon-engine must not depend on {name}");
        }
    }

    #[test]
    fn dependency_names_reads_both_spellings_of_a_table() {
        let manifest = "[package]\nname = \"x\"\n\n[dependencies]\na = \"1\"\n\n\
                        [target.'cfg(unix)'.dependencies]\nb = \"1\"\n\n\
                        [dev-dependencies.c]\nversion = \"1\"\n";
        assert_eq!(dependency_names(manifest), ["a", "b", "c"]);
    }
}
```

- [ ] **Step 2: Run it, and show it fails for the right reason**

```bash
cargo test -p photon-engine the_engine_depends_on_no_ui_runtime
cargo test -p photon-engine dependency_names_reads_both_spellings_of_a_table
```

Expected: both PASS.

Then the probe. Add this line under `[dependencies]` in `crates/photon-engine/Cargo.toml`:

```toml
tokio = { version = "1", features = ["sync"] }
```

```bash
cargo test -p photon-engine the_engine_depends_on_no_ui_runtime
```

Expected: FAIL with `photon-engine must not depend on tokio`. Remove the line again, run `cargo test -p photon-engine the_engine_depends_on_no_ui_runtime`, expected PASS, and check `git diff --stat crates/photon-engine/Cargo.toml Cargo.lock` shows nothing.

- [ ] **Step 3: Correct the places that name the old paths**

In `CLAUDE.md`, the Architecture section's list becomes four crates. Replace the line `Three crates plus the UI:` and the `photon-app` bullet under it with:

```markdown
Four crates plus the UI:

- **`photon-core`** — headless. SQLite library, scanning, thumbnails, metadata. Knows nothing
  about Tauri.
- **`photon-engine`** — `Engine`, the commands over it (`commands.rs`) and the folder
  watcher. No UI runtime is a dependency, and `the_engine_depends_on_no_ui_runtime` reads
  its manifest to keep it so: it is what a second shell stands on.
- **`photon-app`** — the Tauri shell. Owns the IPC surface and the custom protocol that
  serves thumbnails, and re-exports the engine's modules under the names they had there.
```

(keeping the `photon-core` bullet's existing text if it differs from the above, and leaving the `xtask` and `ui/` bullets as they are).

In "IPC is three files per command", item 1 becomes:

```markdown
1. `commands.rs` (in `photon-engine`) — a plain `pub fn` taking `&Engine`, returning
   `CmdResult<T>`. The logic.
```

In "TypeScript mirrors are hand-written and unchecked", `` `commands.rs` and `events.rs` `` becomes `` `photon-engine`'s `commands.rs` and `events.rs` ``.

Then find what else names a moved file by its old place:

```bash
grep -n "photon-app/src/\(engine\|commands\|watch\|events\|memory\|error\|testutil\)" -r CLAUDE.md README.md docs/smoke-checklist.md ui/src crates --include=* 2>/dev/null
```

Fix each hit outside `docs/superpowers/` (specs and plans are records and are left alone). One is known: the comment at the top of `ui/src/lib/api.ts`, which becomes:

```ts
/** The only module that talks to the Rust side. Types mirror the serde structs in
 *  crates/photon-engine/src/commands.rs and events.rs (camelCase). */
```

- [ ] **Step 4: Both gates, and commit**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
git add crates/photon-engine/src/lib.rs CLAUDE.md ui/src/lib/api.ts
git commit -m "test(engine): the engine's manifest names no UI runtime

A second shell is the reason photon-engine exists, and one tauri or tokio line in its
manifest would have every such shell build a webview's runtime. The test reads the manifest;
shown to fail by adding tokio to it.

CLAUDE.md and api.ts name the files where they are now.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 5: Hand over for the pull request**

Report to the controller, for the pull request's description:

- the number of tests in `/tmp/tests-before.txt`, and that the two lists were the same;
- any item whose visibility had to change, with the reason (expected: none);
- that these CI jobs are what cover Review Focus 5 and the pull request waits for all of them: the Rust job on Linux, macOS and Windows (the `cfg` dependencies of `memory.rs`), and the job that runs `npm run tauri build -- --no-bundle` (the `custom-protocol` build).

After it merges, `native-ui` merges `main`.
