# The engine in a crate of its own

Date: 2026-10-09. Sub-project 0 of `2026-10-09-photon-native-ui-design.md`, whose
architecture section was approved the same day.

## The problem

`Engine`, the command layer and the watcher live in `photon-app`, the crate that depends on
Tauri. A second UI that wants them has to depend on `photon-app` and so on Tauri, webkit2gtk
and the rest. They do not need Tauri themselves: `engine.rs`, `commands.rs`, `watch.rs`,
`events.rs`, `memory.rs`, `error.rs` and `testutil.rs` do not mention it.

## Decision

A new crate, `crates/photon-engine`, holding those seven files. `photon-app` depends on it
and keeps only what exists because of the webview: `app.rs`, `ipc.rs`, `protocol.rs`,
`media_server.rs`, `webkit.rs`, the Tauri configuration and the installer template.

It is a move. No behaviour changes, no public name the UI or a test uses changes, and no
release is cut for it.

**It lands on main**, from a branch of main, not on `native-ui`: every later fix to the
engine on main then merges into `native-ui` without a conflict. `native-ui` merges main
once it is in.

Rejected: leaving the files where they are and putting the Tauri parts behind a feature of
`photon-app`. A feature cannot be forgotten in one direction only: a default build would
still pull Tauri into the native UI's lockfile and CI, and "which half am I in" would be a
`cfg` on every item instead of a crate boundary the compiler holds.

## What is not a plain move

Three references point from the files that move into the files that stay. Each is settled
so the dependency runs one way, from `photon-app` to `photon-engine`.

1. **`protocol::RENDERING`** (the lock that makes full-size renders one at a time) is taken
   by `engine.rs` (the export) and `commands.rs` as well as by `protocol.rs`. It moves to
   `photon-engine` with its comment, as a `pub static` in `engine.rs`; `protocol.rs`
   imports it. One lock, as now: two would let an export and the viewer render at once,
   which is what it exists to prevent.
2. **`protocol::parse_key`** (a thumbnail key from its hex) is used by `commands.rs` for the
   two video-frame commands. It moves into `photon-engine`'s `commands.rs` as a `pub fn`,
   with its comment and whatever tests it has; `protocol.rs` imports it.
3. **`commands::media_base(&MediaServer)`** takes the webview's media server. It is one
   line (`server.base_url()`), so it is deleted and `ipc::media_base` calls the server
   itself. It is the only command that stays out of `commands.rs`; the native UI has no
   media server.

## The rest of the move

- **Paths inside `photon-app` do not change.** Its `lib.rs` re-exports the moved modules
  (`pub use photon_engine::{commands, engine, error, events, watch};`), so
  `crate::engine::Engine` and `commands::…` in `ipc.rs`, `app.rs`, `protocol.rs` and
  `media_server.rs` read as before.
- **Visibility.** The crate-private items of the moved files (`FullScan`, `hold_ini_write`,
  `last_full_scan`, `record_full_scan`, `watcher_service`, `emit_folder_status`,
  `occupy_scan_slot_for_test`, `TestScanSlot`, `watch::join_within`) are used only among
  those files and stay `pub(crate)`. The files that stay use only what is already `pub`:
  `Engine::open`, `startup`, `shutdown`, and the fields `lib` and `thumbs`. If the compiler
  finds one more, it is listed in the pull request, not widened in passing.
- **`testutil`.** `protocol.rs` and `media_server.rs` use `testutil::Fixture` in their
  tests. It moves to `photon-engine` behind a `test-support` feature, which `photon-app`
  turns on as a dev-dependency. Its own dependencies (`tempfile`, `image`) are optional and
  belong to that feature, and so does `events::Recorder`, which it uses and which is
  `#[cfg(test)]` today: every such item it needs becomes
  `#[cfg(any(test, feature = "test-support"))]`.
- **Dependencies are divided, not copied.** `photon-engine` takes `photon-core`,
  `parking_lot`, `serde`, `tracing`, and `libc`/`windows-sys` for `memory.rs`, plus what the
  compiler asks for. `tauri` and its plugins, `tiny_http`, `getrandom`, `tokio` and
  `tracing-subscriber` stay in `photon-app`. The `avif-asm` feature of `photon-app` still
  passes straight to `photon-core`.
- **Docs.** CLAUDE.md's Architecture section names four crates, and "IPC is three files per
  command" names `commands.rs` at its new path. The comment at the top of `ui/src/lib/api.ts`
  names the files it mirrors and is corrected.

## Tests

- **The same tests run.** `cargo test --workspace -- --list` names the same tests before and
  after, apart from the crate each is in; the two lists, with the crate stripped, are
  compared in the pull request.
- **One new test**, in `photon-engine`: its `Cargo.toml` names none of `tauri`, `tokio`,
  `tiny_http`, `wry` or `tao` as a dependency. It reads the manifest as text, in the way
  `no-literals.test.ts` reads components. Shown to fail by adding `tauri` to the manifest.
- The Rust gate and the UI gate, and `cargo run -p xtask -- versions` and `metadata`, which
  read `photon-app`'s Tauri configuration and must find it where it was.

## Not in this change

- No command gains or loses a parameter, and `serde`'s camelCase stays on every struct: the
  Svelte UI still reads them.
- `protocol.rs`'s `image()` and `face()` keep their logic. Moving the decode and the crop
  out of the HTTP handlers is the viewer's and the People page's sub-project.
