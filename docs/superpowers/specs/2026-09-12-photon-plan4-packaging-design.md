# photon v1 — Plan 4: Packaging and Release Design

**Date:** 2026-09-12
**Status:** Approved design, pending implementation plan
**Parent spec:** `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`
**Builds on:** Plan 1 (`1e7d510`), Plan 2 (`c98cfea`), Plan 3 (`009284f`)

## 1. Scope

photon builds and runs from a checkout. This plan makes it something a person can
download and install on Linux, macOS or Windows, and closes the last v1 gaps that a
release would otherwise ship with.

### In scope

- An MIT licence and the crate metadata that Debian and Windows installers embed.
- A `release.yml` workflow that builds unsigned installers on all three platforms and
  attaches them to a draft GitHub Release.
- One source of truth for the version, enforced in CI.
- Install and troubleshooting documentation, including what "unsigned" costs the user.
- Two deferred watcher follow-ups whose absence is user-visible in a release build.

### Out of scope

- **Code signing and notarization.** Deliberate: an Apple Developer account, a Windows
  certificate and CI secrets are a separate decision. §6 says what users see without it.
- **An auto-updater.** Tauri's updater needs a signing key and an update endpoint, which
  is a distribution system, not packaging. Users download a new installer.
- **Publishing to repositories** — Flathub, Homebrew, winget, AUR. Each carries its own
  review and manifest process.
- **Linux `.rpm`.** `bundle.targets` is narrowed to the formats named below, so setting it
  to `"all"` no longer silently adds an untested target.

### Why the watcher follow-ups belong here

Plan 3's reviews deferred several findings into this plan. Two are one-line fixes whose
symptom only matters once real users run a release:

- A watcher thread that dies degrades its roots but emits no `folder-status`, so the UI
  keeps claiming live updates work for up to five minutes. `Engine::emit_folder_status`
  already exists.
- `enqueue_pending` is skipped alongside `refresh_grid` when a scan reports no change, so
  a transient render failure is never re-primed without a restart.

Both ship in this plan. The remainder — bounding `stop()` against a dead network mount,
reaping watcher thread handles, the foreign-key log noise in the remove/scan race, and the
two untested-by-choice cases — stay deferred and are recorded, not silently dropped.

## 2. Licence and metadata

- `LICENSE` at the repository root: MIT, `Copyright (c) 2026 David Henning`.
- `[workspace.package]` in the root `Cargo.toml` gains `license = "MIT"`,
  `description`, `repository = "https://github.com/bsg62/photon"` and `authors`. Both
  crates inherit them with `license.workspace = true` and friends.
- `photon-app`'s description is the one users read: it becomes the `.deb` package
  description and the Windows installer's publisher metadata.
- `README.md` gains a licence section.

This is not bookkeeping. `cargo` and the Tauri bundler both source installer fields from
these manifests, and a missing `description` produces a `.deb` whose package description
is empty.

## 3. The version, in one place

Three files carry a version today: `crates/photon-app/tauri.conf.json`, the workspace
`Cargo.toml`, and `ui/package.json`. **`tauri.conf.json` is authoritative** — it is what
the bundler stamps onto every installer.

A `version-check` job runs first and fails the release when:

1. the other two versions disagree with `tauri.conf.json`, or
2. the triggering tag is not exactly `v<that version>`.

Two version numbers drifting apart is the classic packaging bug, and it is cheap to
prevent. The check is the `xtask` crate (`cargo run -p xtask -- versions`), so it can be run
locally before tagging rather than only discovered in CI.

## 4. The release workflow

A new `.github/workflows/release.yml`, separate from `ci.yml`. CI stays fast on every pull
request; building installers for three platforms is minutes of work that has no place on
each push.

**Triggers:** a `v*` tag, plus `workflow_dispatch` so a release can be rehearsed without
tagging. A dispatch run builds and uploads artifacts but creates no Release. Changes to
`release.yml` itself also build and verify on the pull request, without publishing — a
tag-only workflow cannot be verified before the tag that needs it.

**Jobs**, all of them after `version-check`, with `fail-fast: false` so one platform's
failure still leaves the others' artifacts to inspect:

| Job | Runner | Targets |
|---|---|---|
| `linux` | ubuntu-latest | `appimage`, `deb` |
| `macos` | macos-latest | `dmg`, for `aarch64-apple-darwin` and `x86_64-apple-darwin` |
| `windows` | windows-latest | `msi` |

Each names its targets explicitly with `tauri build --target`/`--bundles` rather than
relying on `"all"`, so the set of artifacts is predictable and reviewable.

Two macOS builds, not one: Intel Macs are still widespread, and an arm64-only build
excludes them silently — the `.dmg` mounts and the app refuses to open.

The `.deb` declares its real runtime dependencies (webkit2gtk-4.1, libsoup-3) through
`bundle.linux.deb.depends`. That is the difference between a package that installs and one
that installs and starts.

**Publication:** every job uploads its artifacts. A final `release` job — tag runs only —
downloads them, writes a `SHA256SUMS` file, and creates a **draft** GitHub Release for the
tag with all of it attached. Draft, not published: a human looks before the world does.

## 5. What "tested" can honestly mean

No agent in this project can verify an installer works. Installing a `.dmg` or `.msi`
means running it, and agents do not launch the GUI. So verification splits, and the split
is stated rather than papered over.

**What CI proves, and will:**

- every expected artifact exists at its expected path and is non-empty;
- the packaged binary resolves every library it links, and genuinely links the webkit2gtk-4.1 and
  libsoup-3 the `.deb` declares. This runs on a runner that has the build dependencies installed,
  so it proves the declaration matches the binary — not that a clean machine can satisfy it. The
  `.deb` install on a clean Ubuntu stays a human checklist item.
- the AppImage is executable and its embedded desktop entry names photon;
- the `.msi` and `.dmg` carry the expected version string;
- `SHA256SUMS` matches the uploaded files.

These catch the realistic failures: a missing runtime dependency, an empty bundle, an
artifact path that moved under a Tauri upgrade.

**What only a human proves**, as a per-platform install checklist added to the README:
the installer runs; the app starts from the installed location rather than a checkout; it
appears in the applications menu with the right name and icon; it watches Pictures on
first launch. Plus the two packaging-specific ones:

- On macOS, the permission prompt for the Pictures folder appears, and denying it surfaces
  as the "live updates limited" notice rather than silence. A hardened-runtime build needs
  TCC approval to read `~/Pictures`; the denial path lands on the existing degraded route,
  so the behaviour should be right — but it has never been seen.
- On Linux, a library large enough to exhaust `max_user_watches` shows that same notice.

**Artifact-path uncertainty is a verification step, not an assertion.** The Tauri bundler
is delivered by the npm CLI and is not vendored in this checkout, so the exact output paths
and the AppImage build's tooling requirements cannot be confirmed from a developer machine.
The plan's first workflow task discovers them from a real CI run and pins them; no step
asserts a path this design has not seen.

## 6. Error handling and what unsigned costs

**In the workflow:** deliberately dull. No retries and no fallback bundling. A failed job
leaves the draft release holding whatever succeeded and a red check pointing at the log.
A release that half-worked should look half-worked.

**For the user**, documented in the README because it is the first thing an unsigned build
does to someone:

- **macOS** refuses to open the app from an unidentified developer. The documented path is
  System Settings → Privacy & Security → Open Anyway (right-click → Open still works on macOS 14
  and earlier). This is expected, not a bug.
- **Windows** SmartScreen shows "Windows protected your PC" with a "More info" → "Run
  anyway" path.
- **Linux** has no equivalent gate. The AppImage needs its executable bit; the `.deb`
  installs normally.

The README says why photon is unsigned — certificates cost money and are tied to a
personal identity — so the warnings read as an expected consequence rather than a defect.

## 7. Testing

- **The version check** is the one piece of real logic here and is unit-tested: agreeing
  versions pass; each of the two disagreements fails; a tag that does not match fails; a
  dispatch run with no tag skips the tag comparison only.
- **The artifact assertions** are shell steps in the workflow, each written to fail loudly
  on a missing or empty file rather than letting a later step paper over it.
- **The watcher follow-ups** each get a test that is demonstrated to fail with the change
  reverted: a dying watcher emits `folder-status` with the degraded flag, and a no-change
  scan still enqueues pending render work.
- **CI gained a version gate** — a Linux-guarded step on every pull request and push to
  `main` runs `cargo run -p xtask -- versions` and `-- metadata`, so version drift
  fails on the pull request rather than after a release tag exists. The unbundled ubuntu
  release build stays: it is the fast signal, and the release workflow does not run on
  pull requests. The stale comment in `release-build`'s note about Plan 3 was corrected.

## 8. Success criteria

- A `v0.1.0` tag produces a draft GitHub Release carrying an AppImage, a `.deb`, two
  `.dmg` files and an `.msi`, with checksums.
- A version bumped in `tauri.conf.json` alone fails CI with a message naming the files that
  disagree.
- Installing the `.deb` on a clean Ubuntu pulls in the webview dependencies rather than
  failing at launch.
- The README tells a user how to install on their OS and what security warning to expect.
- photon carries an MIT licence, and every installer states it.
