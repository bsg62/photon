# photon Packaging and Release (Plan 4 of 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A version tag produces unsigned installers for Linux, macOS and Windows in a draft GitHub Release, and photon carries an MIT licence.

**Architecture:** A new `xtask` workspace crate holds the one piece of real logic — the version consistency check — so it is unit-tested by the existing `cargo test --workspace`. A new `release.yml` builds each platform's installers on its own runner, asserts each artifact exists and is well-formed, and a final job publishes them as a draft Release. Licence and metadata move into `[workspace.package]` and the Tauri bundle config, which is where installers read them from.

**Tech Stack:** Rust (edition 2024, rust-version 1.88), `@tauri-apps/cli` 2.11 bundler, GitHub Actions, Node 24.

**Spec:** `docs/superpowers/specs/2026-09-12-photon-plan4-packaging-design.md`. Its parent, `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`, stays binding (as amended: v1 is JPEG, PNG, GIF and WebP; no video).

## Global Constraints

- **`crates/photon-app/tauri.conf.json` is the authoritative version.** The workspace `Cargo.toml` and `ui/package.json` must agree with it, and a release tag must be exactly `v<that version>`.
- Current version is `0.1.0` everywhere; do not bump it in this plan.
- Licence is **MIT**, `Copyright (c) 2026 David Henning`. Repository is `https://github.com/bsg62/photon`.
- Installers are **unsigned**. Never add signing identities, certificates, notarization or `createUpdaterArtifacts`.
- Bundle targets are exactly: Linux `appimage` + `deb`, macOS `dmg` (two architectures), Windows `msi`. **No `rpm`, no `nsis`.** Legal `BundleType` values are `deb`, `rpm`, `appimage`, `msi`, `nsis`, `app`, `dmg` (lowercase).
- The CLI flags are `-b, --bundles <list>`, `-t, --target <triple>`, `--ci`. The set `--bundles` accepts is filtered by the host OS, so each job passes only its own platform's values.
- Every task ends with `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` clean, and `cargo test --workspace` passing. Tasks touching the UI also need `npm run check` at 0 errors and 0 warnings plus `npm test`.
- **A new test must be demonstrated to fail with its change reverted.** Plan 3 produced three tests that asserted something already true.
- Never launch the GUI. Never push. Never create a tag. Conventional Commits.

## File Structure

```
LICENSE                                   NEW: MIT, 2026 David Henning
Cargo.toml                                + workspace.package metadata, + xtask member
crates/xtask/Cargo.toml                   NEW
crates/xtask/src/main.rs                  NEW: CLI entry (versions | metadata)
crates/xtask/src/checks.rs                NEW: pure parse + check functions, unit-tested
crates/photon-core/Cargo.toml             inherits licence/description metadata
crates/photon-app/Cargo.toml              inherits licence/description metadata
crates/photon-app/tauri.conf.json         bundle metadata, narrowed targets, deb depends
crates/photon-app/src/watch.rs            watcher_died / degrade_failed_roots emit folder-status
crates/photon-app/src/engine.rs           enqueue_pending moves outside the no-change guard
.github/workflows/release.yml             NEW: build, verify, draft release
.github/workflows/ci.yml                  stale "Plan 3" comment corrected
README.md                                 Install, unsigned warnings, licence, smoke checklist
```

---

### Task 1: The `xtask` crate and the version check

**Files:**
- Create: `crates/xtask/Cargo.toml`, `crates/xtask/src/main.rs`, `crates/xtask/src/checks.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Produces:
  - `checks::Versions { tauri: String, cargo: String, ui: String }`
  - `checks::parse_tauri_version(json: &str) -> Result<String, String>`
  - `checks::parse_cargo_version(toml_src: &str) -> Result<String, String>`
  - `checks::parse_pkg_version(json: &str) -> Result<String, String>`
  - `checks::check_versions(v: &Versions, tag: Option<&str>) -> Result<String, Vec<String>>`
- Consumes: nothing from earlier tasks.

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`, change the members line to:

```toml
members = ["crates/photon-core", "crates/photon-app", "crates/xtask"]
```

- [ ] **Step 2: Create `crates/xtask/Cargo.toml`**

```toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
serde_json = "1"
toml = "0.9"
```

Both are already in `Cargo.lock` (`serde_json` 1.0.151, `toml` 0.9.12), so this pulls in no
new crate.

- [ ] **Step 3: Write the failing tests**

Create `crates/xtask/src/checks.rs` containing ONLY this test module for now (the functions come in Step 5):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn versions(tauri: &str, cargo: &str, ui: &str) -> Versions {
        Versions {
            tauri: tauri.into(),
            cargo: cargo.into(),
            ui: ui.into(),
        }
    }

    #[test]
    fn agreeing_versions_pass() {
        let v = versions("0.1.0", "0.1.0", "0.1.0");
        assert_eq!(check_versions(&v, None).unwrap(), "0.1.0");
    }

    #[test]
    fn a_disagreeing_cargo_version_fails_and_names_the_file() {
        let v = versions("0.1.0", "0.2.0", "0.1.0");
        let errs = check_versions(&v, None).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("Cargo.toml"), "got {:?}", errs);
        assert!(errs[0].contains("0.2.0"), "got {:?}", errs);
    }

    #[test]
    fn a_disagreeing_ui_version_fails_and_names_the_file() {
        let v = versions("0.1.0", "0.1.0", "9.9.9");
        let errs = check_versions(&v, None).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("ui/package.json"), "got {:?}", errs);
    }

    #[test]
    fn both_disagreeing_are_both_reported() {
        let v = versions("0.1.0", "0.2.0", "9.9.9");
        assert_eq!(check_versions(&v, None).unwrap_err().len(), 2);
    }

    #[test]
    fn a_matching_tag_passes() {
        let v = versions("0.1.0", "0.1.0", "0.1.0");
        assert_eq!(check_versions(&v, Some("v0.1.0")).unwrap(), "0.1.0");
    }

    #[test]
    fn a_tag_that_does_not_match_fails() {
        let v = versions("0.1.0", "0.1.0", "0.1.0");
        let errs = check_versions(&v, Some("v0.2.0")).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("v0.2.0") && errs[0].contains("v0.1.0"), "got {:?}", errs);
    }

    #[test]
    fn a_tag_missing_its_v_prefix_fails() {
        let v = versions("0.1.0", "0.1.0", "0.1.0");
        assert!(check_versions(&v, Some("0.1.0")).is_err());
    }

    #[test]
    fn no_tag_skips_only_the_tag_comparison() {
        // A workflow_dispatch run has no tag. Version disagreement must still fail.
        let v = versions("0.1.0", "0.2.0", "0.1.0");
        assert!(check_versions(&v, None).is_err());
    }

    #[test]
    fn versions_are_read_from_each_real_file_format() {
        let tauri = r#"{ "productName": "photon", "version": "0.1.0", "bundle": {} }"#;
        let cargo = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        let pkg = r#"{ "name": "photon-ui", "version": "0.1.0" }"#;
        assert_eq!(parse_tauri_version(tauri).unwrap(), "0.1.0");
        assert_eq!(parse_cargo_version(cargo).unwrap(), "0.1.0");
        assert_eq!(parse_pkg_version(pkg).unwrap(), "0.1.0");
    }

    #[test]
    fn a_missing_version_field_is_an_error_not_a_panic() {
        assert!(parse_tauri_version(r#"{"productName":"photon"}"#).is_err());
        assert!(parse_cargo_version("[workspace]\nmembers = []\n").is_err());
        assert!(parse_pkg_version(r#"{"name":"photon-ui"}"#).is_err());
        assert!(parse_tauri_version("not json at all").is_err());
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p xtask`
Expected: FAIL — compilation errors, `Versions`, `check_versions`, `parse_tauri_version`, `parse_cargo_version` and `parse_pkg_version` are not defined.

- [ ] **Step 5: Write the implementation**

Insert ABOVE the test module in `crates/xtask/src/checks.rs`:

```rust
//! The checks `xtask` runs. Pure functions over file contents, so they are unit-tested
//! without touching the repository.

/// The version as each of the three files states it.
#[derive(Debug, Clone)]
pub struct Versions {
    /// `crates/photon-app/tauri.conf.json` — the authoritative one.
    pub tauri: String,
    /// The workspace `Cargo.toml`.
    pub cargo: String,
    /// `ui/package.json`.
    pub ui: String,
}

pub fn parse_tauri_version(json: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("tauri.conf.json is not valid JSON: {e}"))?;
    v.get("version")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "tauri.conf.json has no string `version` field".to_owned())
}

pub fn parse_pkg_version(json: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("package.json is not valid JSON: {e}"))?;
    v.get("version")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "package.json has no string `version` field".to_owned())
}

pub fn parse_cargo_version(toml_src: &str) -> Result<String, String> {
    let v: toml::Value =
        toml::from_str(toml_src).map_err(|e| format!("Cargo.toml is not valid TOML: {e}"))?;
    v.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml has no `[workspace.package] version`".to_owned())
}

/// Checks that all three versions agree with `tauri.conf.json`, and — when a tag triggered
/// the run — that the tag is exactly `v<version>`.
///
/// Returns every problem rather than the first, so one CI run tells you everything to fix.
pub fn check_versions(v: &Versions, tag: Option<&str>) -> Result<String, Vec<String>> {
    let mut problems = Vec::new();
    if v.cargo != v.tauri {
        problems.push(format!(
            "Cargo.toml says {}, but tauri.conf.json says {}",
            v.cargo, v.tauri
        ));
    }
    if v.ui != v.tauri {
        problems.push(format!(
            "ui/package.json says {}, but tauri.conf.json says {}",
            v.ui, v.tauri
        ));
    }
    if let Some(tag) = tag {
        let expected = format!("v{}", v.tauri);
        if tag != expected {
            problems.push(format!("tag is {tag}, but tauri.conf.json expects {expected}"));
        }
    }
    if problems.is_empty() {
        Ok(v.tauri.clone())
    } else {
        Err(problems)
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p xtask`
Expected: PASS (10 tests).

- [ ] **Step 7: Write the CLI entry point**

Create `crates/xtask/src/main.rs`:

```rust
//! Repository chores that need to run identically in CI and on a developer machine.
//!
//! Usage:
//!   cargo run -p xtask -- versions [--tag v0.1.0]

mod checks;

use checks::{Versions, check_versions, parse_cargo_version, parse_pkg_version, parse_tauri_version};
use std::{path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str);
    let tag = tag_arg(&args);
    match command {
        Some("versions") => run_versions(tag.as_deref()),
        other => {
            eprintln!("unknown command {other:?}; expected `versions`");
            ExitCode::FAILURE
        }
    }
}

/// Reads `--tag <value>`. An empty value is treated as absent, because a
/// `workflow_dispatch` run passes an empty string rather than omitting the flag.
fn tag_arg(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "--tag")?;
    let value = args.get(i + 1)?;
    if value.is_empty() {
        None
    } else {
        Some(value.clone())
    }
}

fn run_versions(tag: Option<&str>) -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask lives at <root>/crates/xtask")
        .to_path_buf();

    let read = |rel: &str| -> Result<String, String> {
        std::fs::read_to_string(root.join(rel)).map_err(|e| format!("cannot read {rel}: {e}"))
    };

    let parsed = (|| -> Result<Versions, String> {
        Ok(Versions {
            tauri: parse_tauri_version(&read("crates/photon-app/tauri.conf.json")?)?,
            cargo: parse_cargo_version(&read("Cargo.toml")?)?,
            ui: parse_pkg_version(&read("ui/package.json")?)?,
        })
    })();

    let versions = match parsed {
        Ok(v) => v,
        Err(err) => {
            eprintln!("version check could not run: {err}");
            return ExitCode::FAILURE;
        }
    };

    match check_versions(&versions, tag) {
        Ok(version) => {
            println!("version check passed: {version}");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            eprintln!("version check failed:");
            for p in &problems {
                eprintln!("  - {p}");
            }
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 8: Verify the CLI runs green against the real repository**

Run: `cargo run -p xtask -- versions`
Expected: prints `version check passed: 0.1.0`, exit code 0.

Run: `cargo run -p xtask -- versions --tag v9.9.9`
Expected: fails, naming `v9.9.9` and `v0.1.0`.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add Cargo.toml Cargo.lock crates/xtask
git commit -m "feat(xtask): version consistency check across the three version files"
```

---

### Task 2: MIT licence and installer metadata

**Files:**
- Create: `LICENSE`
- Modify: `Cargo.toml`, `crates/photon-core/Cargo.toml`, `crates/photon-app/Cargo.toml`, `crates/photon-app/tauri.conf.json`
- Test: `crates/xtask/src/checks.rs` (new `check_metadata`), `crates/xtask/src/main.rs` (new `metadata` command)

**Interfaces:**
- Consumes: `checks.rs` and the `xtask` CLI shape from Task 1.
- Produces: `checks::check_metadata(cargo_toml: &str, tauri_conf: &str, license_file_exists: bool) -> Result<(), Vec<String>>`, plus a `metadata` subcommand.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/xtask/src/checks.rs`:

```rust
    const GOOD_CARGO: &str = r#"
[workspace]
members = []

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "A fast, local photo manager."
repository = "https://github.com/bsg62/photon"
authors = ["David Henning"]
"#;

    const GOOD_CONF: &str = r#"{
      "version": "0.1.0",
      "bundle": {
        "license": "MIT",
        "licenseFile": "../../LICENSE",
        "copyright": "Copyright (c) 2026 David Henning",
        "publisher": "David Henning",
        "homepage": "https://github.com/bsg62/photon",
        "shortDescription": "A fast, local photo manager.",
        "longDescription": "photon watches folders in place.",
        "category": "Photography",
        "targets": ["appimage", "deb", "dmg", "msi"],
        "linux": { "deb": { "depends": ["libwebkit2gtk-4.1-0", "libsoup-3.0-0"] } }
      }
    }"#;

    #[test]
    fn complete_metadata_passes() {
        assert!(check_metadata(GOOD_CARGO, GOOD_CONF, true).is_ok());
    }

    #[test]
    fn a_missing_license_file_is_reported() {
        let errs = check_metadata(GOOD_CARGO, GOOD_CONF, false).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("LICENSE")), "got {errs:?}");
    }

    #[test]
    fn each_missing_cargo_field_is_reported() {
        for field in ["license", "description", "repository", "authors"] {
            let stripped: String = GOOD_CARGO
                .lines()
                .filter(|l| !l.starts_with(field))
                .collect::<Vec<_>>()
                .join("\n");
            let errs = check_metadata(&stripped, GOOD_CONF, true).unwrap_err();
            assert!(
                errs.iter().any(|e| e.contains(field)),
                "removing {field} was not reported: {errs:?}"
            );
        }
    }

    #[test]
    fn a_deb_without_webview_dependencies_is_reported() {
        let conf = GOOD_CONF.replace(r#""libwebkit2gtk-4.1-0", "libsoup-3.0-0""#, "");
        let errs = check_metadata(GOOD_CARGO, &conf, true).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("depends")), "got {errs:?}");
    }

    #[test]
    fn an_unexpected_bundle_target_is_reported() {
        // rpm and nsis are deliberately out of scope: they would ship untested installers.
        let conf = GOOD_CONF.replace(r#""appimage", "deb""#, r#""appimage", "deb", "rpm""#);
        let errs = check_metadata(GOOD_CARGO, &conf, true).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("rpm")), "got {errs:?}");
    }

    #[test]
    fn bundle_targets_set_to_all_is_reported() {
        let conf = GOOD_CONF.replace(r#"["appimage", "deb", "dmg", "msi"]"#, r#""all""#);
        let errs = check_metadata(GOOD_CARGO, &conf, true).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("targets")), "got {errs:?}");
    }

    #[test]
    fn signing_configuration_is_reported_because_photon_ships_unsigned() {
        let conf = GOOD_CONF.replace(
            r#""category": "Photography""#,
            r#""category": "Photography", "createUpdaterArtifacts": true"#,
        );
        let errs = check_metadata(GOOD_CARGO, &conf, true).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("createUpdaterArtifacts")), "got {errs:?}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p xtask`
Expected: FAIL — `check_metadata` is not defined.

- [ ] **Step 3: Write the implementation**

Add to `crates/xtask/src/checks.rs`, above the test module:

```rust
/// The bundle targets photon ships. `rpm` and `nsis` are deliberately excluded: nothing in
/// CI verifies them, and `targets: "all"` would add them silently.
const EXPECTED_TARGETS: [&str; 4] = ["appimage", "deb", "dmg", "msi"];

/// Runtime libraries the `.deb` must declare. Without these the package installs and then
/// fails to start, which is the failure this check exists to prevent.
const REQUIRED_DEB_DEPENDS: [&str; 2] = ["libwebkit2gtk-4.1-0", "libsoup-3.0-0"];

/// Checks the metadata that ends up inside installers: the licence, the fields `cargo` and
/// the Tauri bundler read, and the bundle target list.
pub fn check_metadata(
    cargo_toml: &str,
    tauri_conf: &str,
    license_file_exists: bool,
) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();

    if !license_file_exists {
        problems.push("LICENSE is missing from the repository root".to_owned());
    }

    match toml::from_str::<toml::Value>(cargo_toml) {
        Err(e) => problems.push(format!("Cargo.toml is not valid TOML: {e}")),
        Ok(v) => {
            let pkg = v.get("workspace").and_then(|w| w.get("package"));
            for field in ["license", "description", "repository", "authors"] {
                let present = pkg
                    .and_then(|p| p.get(field))
                    .is_some_and(|f| !f.is_str() || !f.as_str().unwrap_or("").is_empty());
                if !present {
                    problems.push(format!(
                        "Cargo.toml [workspace.package] is missing `{field}`; installers embed it"
                    ));
                }
            }
        }
    }

    match serde_json::from_str::<serde_json::Value>(tauri_conf) {
        Err(e) => problems.push(format!("tauri.conf.json is not valid JSON: {e}")),
        Ok(v) => {
            let bundle = v.get("bundle");
            for field in ["license", "copyright", "publisher", "shortDescription"] {
                if bundle.and_then(|b| b.get(field)).is_none() {
                    problems.push(format!("tauri.conf.json bundle is missing `{field}`"));
                }
            }
            if bundle.and_then(|b| b.get("createUpdaterArtifacts")).is_some() {
                problems.push(
                    "tauri.conf.json sets `createUpdaterArtifacts`; photon ships no updater"
                        .to_owned(),
                );
            }
            match bundle.and_then(|b| b.get("targets")) {
                Some(serde_json::Value::Array(items)) => {
                    let names: Vec<&str> = items.iter().filter_map(|i| i.as_str()).collect();
                    for name in &names {
                        if !EXPECTED_TARGETS.contains(name) {
                            problems.push(format!("unexpected bundle target `{name}`"));
                        }
                    }
                    for expected in EXPECTED_TARGETS {
                        if !names.contains(&expected) {
                            problems.push(format!("bundle target `{expected}` is missing"));
                        }
                    }
                }
                _ => problems.push(
                    "tauri.conf.json bundle `targets` must be an explicit list, not \"all\""
                        .to_owned(),
                ),
            }
            let depends: Vec<&str> = bundle
                .and_then(|b| b.get("linux"))
                .and_then(|l| l.get("deb"))
                .and_then(|d| d.get("depends"))
                .and_then(|d| d.as_array())
                .map(|a| a.iter().filter_map(|i| i.as_str()).collect())
                .unwrap_or_default();
            for required in REQUIRED_DEB_DEPENDS {
                if !depends.iter().any(|d| d.starts_with(required)) {
                    problems.push(format!("deb `depends` does not declare `{required}`"));
                }
            }
        }
    }

    if problems.is_empty() { Ok(()) } else { Err(problems) }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p xtask`
Expected: PASS.

- [ ] **Step 5: Create `LICENSE`**

```
MIT License

Copyright (c) 2026 David Henning

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 6: Add the workspace metadata**

In the root `Cargo.toml`, extend `[workspace.package]` so it reads:

```toml
[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.88"
license = "MIT"
description = "A fast, local photo manager for Linux, macOS and Windows."
repository = "https://github.com/bsg62/photon"
authors = ["David Henning"]
```

- [ ] **Step 7: Inherit it in both crates**

Add these four lines to the `[package]` section of BOTH `crates/photon-core/Cargo.toml` and `crates/photon-app/Cargo.toml`, directly under `rust-version.workspace = true`:

```toml
license.workspace = true
description.workspace = true
repository.workspace = true
authors.workspace = true
```

- [ ] **Step 8: Add the bundle metadata**

In `crates/photon-app/tauri.conf.json`, replace the whole `"bundle"` object with:

```json
  "bundle": {
    "active": true,
    "targets": ["appimage", "deb", "dmg", "msi"],
    "license": "MIT",
    "licenseFile": "../../LICENSE",
    "copyright": "Copyright (c) 2026 David Henning",
    "publisher": "David Henning",
    "homepage": "https://github.com/bsg62/photon",
    "shortDescription": "A fast, local photo manager.",
    "longDescription": "photon is a fast, local photo manager and a spiritual successor to Picasa 3. It watches folders in place and never moves or changes your files.",
    "category": "Photography",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "linux": {
      "deb": {
        "depends": ["libwebkit2gtk-4.1-0", "libsoup-3.0-0"],
        "section": "graphics"
      }
    },
    "macOS": {
      "minimumSystemVersion": "10.15"
    }
  }
```

- [ ] **Step 9: Wire the `metadata` command into the CLI**

In `crates/xtask/src/main.rs`, add `check_metadata` to the `use checks::{...}` list, add the match arm `Some("metadata") => run_metadata(),` above the `other =>` arm, update the `unknown command` message to `expected `versions` or `metadata``, and append:

```rust
fn run_metadata() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask lives at <root>/crates/xtask")
        .to_path_buf();

    let cargo_toml = match std::fs::read_to_string(root.join("Cargo.toml")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read Cargo.toml: {e}");
            return ExitCode::FAILURE;
        }
    };
    let conf_path = root.join("crates").join("photon-app").join("tauri.conf.json");
    let tauri_conf = match std::fs::read_to_string(&conf_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read tauri.conf.json: {e}");
            return ExitCode::FAILURE;
        }
    };

    match check_metadata(&cargo_toml, &tauri_conf, root.join("LICENSE").is_file()) {
        Ok(()) => {
            println!("metadata check passed");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            eprintln!("metadata check failed:");
            for p in &problems {
                eprintln!("  - {p}");
            }
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 10: Verify against the real repository**

Run: `cargo run -p xtask -- metadata`
Expected: prints `metadata check passed`, exit code 0.

- [ ] **Step 11: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add LICENSE Cargo.toml Cargo.lock crates/photon-core/Cargo.toml crates/photon-app/Cargo.toml crates/photon-app/tauri.conf.json crates/xtask
git commit -m "feat: MIT licence and the metadata installers embed"
```

---

### Task 3: The two deferred watcher follow-ups

**Files:**
- Modify: `crates/photon-app/src/watch.rs` (`watcher_died` ~line 261, `degrade_failed_roots` ~line 282), `crates/photon-app/src/engine.rs` (`run_scan`, the `if touched_rows || online_changed` guard ~line 483)
- Test: the `tests` module in `crates/photon-app/src/watch.rs`, and the `tests` module in `crates/photon-app/src/engine.rs`

**Interfaces:**
- Consumes: `Engine::emit_folder_status(&self, watched_id: i64, degraded: bool)` (engine.rs:197), `Fixture { engine, photos, events: Arc<Recorder>, .. }` and `fixture(&[(&str, &[u8])])` from `crate::testutil`, `Recorder::all() -> Vec<Recorded>`, `Recorded::Folder(FolderStatus { watched_id, online, degraded })`, `ThumbService::queued() -> usize`, `Library::set_thumb_state(&self, id: i64, state: ThumbState, error: Option<&str>) -> Result<()>` (items.rs:241), `photon_core::media::ThumbState`. Both `Engine::lib` and `Engine::thumbs` are public fields.
- Produces: nothing later tasks consume.

Background: `watcher_died` and `degrade_failed_roots` mark roots degraded but emit no `folder-status`, so the status bar keeps claiming live updates work until some later scan happens — up to five minutes. The registration path at watch.rs:90 already emits; these two do not.

- [ ] **Step 1: Write the failing watcher test**

Add to the `tests` module in `crates/photon-app/src/watch.rs`:

```rust
    use crate::events::Recorded;

    #[test]
    fn a_dead_watcher_tells_the_ui_its_folders_are_degraded() {
        let f = fixture(&[]);
        let watched = f.add_photos();
        let degraded = Mutex::new(Vec::new());
        let slot: Mutex<Option<Watcher>> = Mutex::new(None);

        watcher_died(&f.engine, &degraded, &slot);

        assert_eq!(degraded.lock().clone(), vec![watched.id], "the root is degraded");
        let told = last_degraded(&f, watched.id);
        assert!(
            told,
            "a dead watcher must emit folder-status immediately; without it the status bar \
             claims live updates work until the next five-minute tick"
        );
    }
```

Add this helper to the same `tests` module (a free function, not an inherent `impl` on a
type from another module):

```rust
    /// True if the last `folder-status` recorded for `id` reported degraded.
    fn last_degraded(f: &crate::testutil::Fixture, id: i64) -> bool {
        f.events
            .all()
            .iter()
            .filter_map(|e| match e {
                Recorded::Folder(s) if s.watched_id == id => Some(s.degraded),
                _ => None,
            })
            .next_back()
            .unwrap_or(false)
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p photon-app a_dead_watcher_tells_the_ui`
Expected: FAIL on the `told` assertion — the root is degraded but no `folder-status` was emitted.

- [ ] **Step 3: Make it pass**

In `crates/photon-app/src/watch.rs`, in `watcher_died`, replace the final loop with:

```rust
    let mut degraded = degraded.lock();
    let newly: Vec<i64> = watched
        .iter()
        .filter(|w| w.online)
        .map(|w| {
            mark_degraded(&mut degraded, w.id);
            w.id
        })
        .collect();
    drop(degraded);
    // Tell the UI now. `folder-status` is otherwise only emitted at the tail of a scan, so
    // without this the status bar claims live updates are fine until the five-minute tick.
    for id in newly {
        engine.emit_folder_status(id, true);
    }
```

In `degrade_failed_roots`, replace the trailing `let mut degraded = degraded.lock(); for id in affected { ... }` block with:

```rust
    let mut degraded = degraded.lock();
    for id in &affected {
        mark_degraded(&mut degraded, *id);
    }
    drop(degraded);
    for id in affected {
        engine.emit_folder_status(id, true);
    }
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p photon-app a_dead_watcher_tells_the_ui`
Expected: PASS.

- [ ] **Step 5: Write the failing thumbnail test**

Add to the `tests` module in `crates/photon-app/src/engine.rs`. That module already has
`use super::*; use crate::events::Recorded; use crate::testutil::{fixture, jpeg};`, so
`fixture` and `jpeg` are in scope — do not re-qualify them. Add one import at the top of
the module: `use photon_core::media::ThumbState;`

No new library method is needed: `Library::set_thumb_state` already exists at
`crates/photon-core/src/library/items.rs:241`. Note that `thumb_state` is an INTEGER column
(`Pending` = 0, `Ready` = 1, `Failed` = 2), not a string.

```rust
    #[test]
    fn a_scan_that_changed_nothing_still_re_primes_pending_thumbnails() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        f.engine.wait_for_scans();

        // Simulate a render that failed transiently: the item is pending again, but a
        // rescan finds nothing changed on disk.
        let id = f.ids()[0];
        f.engine.lib.set_thumb_state(id, ThumbState::Pending, None).unwrap();

        assert!(f.engine.start_scan(watched.clone()));
        f.engine.wait_for_scans();

        assert!(
            f.engine.thumbs.queued() > 0,
            "a no-change scan must still queue pending thumbnails; otherwise a transient \
             render failure is never retried without restarting photon"
        );
    }
```

**Do not break the sibling test.** `a_scan_that_changes_nothing_does_not_rebuild_the_grid`
in the same module asserts that a no-change scan does NOT refresh the grid. `refresh_grid`
must therefore stay inside the `touched_rows || online_changed` guard; only
`enqueue_pending` moves out. Both tests must pass.

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p photon-app a_scan_that_changed_nothing_still_re_primes`
Expected: FAIL — `queued()` is 0, because `enqueue_pending` sits inside the `touched_rows || online_changed` guard.

- [ ] **Step 7: Make it pass**

In `crates/photon-app/src/engine.rs`, in `run_scan`, change the guard block to:

```rust
        if touched_rows || online_changed {
            if let Err(err) = self.refresh_grid() {
                tracing::warn!(%err, "grid refresh failed");
            }
        }
        // Outside the guard: `refresh_grid` is the expensive half (it reads every grid row
        // and makes the UI refetch), but re-queueing pending thumbnails is cheap and is the
        // only thing that retries an item whose render failed transiently. Leaving it inside
        // meant such an item waited for an unrelated change, or a restart.
        if let Err(err) = self.thumbs.enqueue_pending() {
            tracing::warn!(%err, "could not queue pending thumbnails");
        }
```

- [ ] **Step 8: Run it to verify it passes**

Run: `cargo test -p photon-app a_scan_that_changed_nothing_still_re_primes`
Expected: PASS.

- [ ] **Step 9: Prove both tests fail on revert**

Temporarily revert each change (restore the loop in `watcher_died`; move `enqueue_pending` back inside the guard), confirm the corresponding test fails, then restore the fix. Record in the commit message that both were verified this way.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/photon-app/src/watch.rs crates/photon-app/src/engine.rs
git commit -m "fix(watch): emit folder-status when a watcher dies, and re-prime thumbs on no-change scans"
```

---

### Task 4: The release workflow

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `.github/workflows/ci.yml:32-33` (the comment saying packaging is Plan 3)

**Interfaces:**
- Consumes: `cargo run -p xtask -- versions --tag <tag>` and `cargo run -p xtask -- metadata` from Tasks 1 and 2; the bundle targets from Task 2.
- Produces: build artifacts named `photon-linux`, `photon-macos-aarch64`, `photon-macos-x86_64`, `photon-windows`.

**Why there is a `pull_request` trigger:** a workflow that only runs on tags cannot be verified before the tag that needs it. Changes to `release.yml` itself therefore build and verify on pull requests, without publishing anything. This is a refinement of spec §4, recorded in the spec's trigger list.

- [ ] **Step 1: Create the workflow**

Create `.github/workflows/release.yml`:

```yaml
name: release

on:
  push:
    tags: ['v*']
  workflow_dispatch:
  # A tag-only workflow cannot be tested before the tag that needs it, so changes to this
  # file build and verify on the pull request. The publish job stays tag-only.
  pull_request:
    paths: ['.github/workflows/release.yml']

permissions:
  contents: write

jobs:
  version-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Versions agree, and match the tag
        run: |
          cargo run -p xtask -- versions --tag "${{ startsWith(github.ref, 'refs/tags/') && github.ref_name || '' }}"
          cargo run -p xtask -- metadata

  linux:
    needs: version-check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - name: Install Linux webview dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev libxdo-dev libssl-dev
      - uses: actions/setup-node@v5
        with:
          node-version: 24
          cache: npm
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: npm ci
      - run: npm run tauri build -- --bundles deb,appimage --ci
      - name: Verify the artifacts
        run: |
          set -euo pipefail
          deb=$(ls target/release/bundle/deb/*.deb)
          app=$(ls target/release/bundle/appimage/*.AppImage)
          test -s "$deb" || { echo "::error::deb is missing or empty"; exit 1; }
          test -s "$app" || { echo "::error::AppImage is missing or empty"; exit 1; }
          test -x "$app" || { echo "::error::AppImage is not executable"; exit 1; }
          echo "deb:      $deb"
          echo "appimage: $app"
          # The dependency check this whole job exists for: a .deb that installs and then
          # fails to start is the classic packaging bug.
          depends=$(dpkg-deb --field "$deb" Depends)
          echo "Depends: $depends"
          for lib in libwebkit2gtk-4.1 libsoup-3.0; do
            echo "$depends" | grep -q "$lib" || { echo "::error::deb does not depend on $lib"; exit 1; }
          done
          dpkg-deb --field "$deb" Description | grep -q . || { echo "::error::deb description is empty"; exit 1; }
      - uses: actions/upload-artifact@v5
        with:
          name: photon-linux
          path: |
            target/release/bundle/deb/*.deb
            target/release/bundle/appimage/*.AppImage
          if-no-files-found: error

  macos:
    needs: version-check
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: aarch64-apple-darwin
            name: photon-macos-aarch64
          - target: x86_64-apple-darwin
            name: photon-macos-x86_64
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v5
      - uses: actions/setup-node@v5
        with:
          node-version: 24
          cache: npm
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
      - run: npm ci
      - run: npm run tauri build -- --target ${{ matrix.target }} --bundles dmg --ci
      - name: Verify the artifacts
        run: |
          set -euo pipefail
          dmg=$(ls target/${{ matrix.target }}/release/bundle/dmg/*.dmg)
          test -s "$dmg" || { echo "::error::dmg is missing or empty"; exit 1; }
          echo "dmg: $dmg"
          case "$dmg" in
            *0.1.0*) ;;
            *) echo "::error::dmg name does not carry the version: $dmg"; exit 1 ;;
          esac
      - uses: actions/upload-artifact@v5
        with:
          name: ${{ matrix.name }}
          path: target/${{ matrix.target }}/release/bundle/dmg/*.dmg
          if-no-files-found: error

  windows:
    needs: version-check
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v5
      - uses: actions/setup-node@v5
        with:
          node-version: 24
          cache: npm
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: npm ci
      - run: npm run tauri build -- --bundles msi --ci
      - name: Verify the artifacts
        shell: bash
        run: |
          set -euo pipefail
          msi=$(ls target/release/bundle/msi/*.msi)
          test -s "$msi" || { echo "::error::msi is missing or empty"; exit 1; }
          echo "msi: $msi"
          case "$msi" in
            *0.1.0*) ;;
            *) echo "::error::msi name does not carry the version: $msi"; exit 1 ;;
          esac
      - uses: actions/upload-artifact@v5
        with:
          name: photon-windows
          path: target/release/bundle/msi/*.msi
          if-no-files-found: error

  release:
    # Tag runs only: a pull request or a dispatch rehearsal builds and verifies, but
    # publishes nothing.
    if: startsWith(github.ref, 'refs/tags/')
    needs: [linux, macos, windows]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: actions/download-artifact@v5
        with:
          path: artifacts
          merge-multiple: true
      - name: Checksums
        run: |
          set -euo pipefail
          cd artifacts
          ls -la
          sha256sum * > SHA256SUMS
          cat SHA256SUMS
      - name: Draft release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          set -euo pipefail
          gh release create "${{ github.ref_name }}" \
            --draft \
            --title "photon ${{ github.ref_name }}" \
            --notes "Unsigned installers. See the README for the security warnings each OS shows, and how to get past them." \
            artifacts/*
```

- [ ] **Step 2: Validate the YAML parses**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('ok')"`
Expected: `ok`. (If PyYAML is unavailable, use `npx --yes js-yaml .github/workflows/release.yml > /dev/null && echo ok`.)

- [ ] **Step 3: Correct the stale comment in `ci.yml`**

In `.github/workflows/ci.yml`, replace the comment above the `release-build` job (lines 32-33) with:

```yaml
  # Builds the release path (frontend embedded, `tauri::is_dev()` false), which the debug
  # builds above never exercise. Ubuntu only and unbundled: installers are built by
  # release.yml, which runs on tags rather than on every pull request.
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml .github/workflows/ci.yml
git commit -m "ci: build unsigned installers for all three platforms into a draft release"
```

---

### Task 5: Install documentation and the release checklist

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-09-12-photon-plan4-packaging-design.md`

**Interfaces:**
- Consumes: the artifact names and security warnings established in Tasks 2 and 4.
- Produces: nothing later tasks consume.

- [ ] **Step 1: Add an Install section**

In `README.md`, insert between the intro paragraph (ending `...user data and cache directories.`) and `## Development`:

````markdown
## Install

Download the installer for your system from the [latest release](https://github.com/bsg62/photon/releases/latest).

| System | File | Install |
|---|---|---|
| Linux | `.AppImage` | `chmod +x photon_*.AppImage && ./photon_*.AppImage` |
| Linux (Debian, Ubuntu) | `.deb` | `sudo apt install ./photon_*.deb` — pulls in webkit2gtk-4.1 and libsoup-3 |
| macOS | `.dmg` | Open it and drag photon to Applications. Take the `aarch64` file for Apple Silicon, `x64` for Intel. |
| Windows | `.msi` | Run it. |

### photon is not code-signed

Signing certificates cost money and are tied to a personal identity, so photon's installers
are unsigned. Every system says so in its own way, and none of it means the download is
broken:

- **macOS** refuses to open an app from an unidentified developer. Right-click photon in
  Applications and choose **Open**, then confirm. You only do this once.
- **Windows** shows "Windows protected your PC". Choose **More info**, then **Run anyway**.
- **Linux** shows nothing; the AppImage just needs its executable bit.

Verify a download against the `SHA256SUMS` file attached to the release:
`sha256sum -c SHA256SUMS --ignore-missing`.
````

- [ ] **Step 2: Add the licence section**

Append to the end of `README.md`:

```markdown
## Licence

MIT. See [LICENSE](LICENSE).
```

- [ ] **Step 3: Extend the smoke checklist**

In `README.md`, change the `## Manual smoke checklist` heading line `Run this before each release, on each OS:` to:

```markdown
Run this before each release, on each OS. The installed-app checks at the end can only be
done from a real installer, so run them against the draft release's artifacts before
publishing it.
```

Then append these items to the end of that list:

```markdown
- [ ] The downloaded installer runs, and photon starts from the installed location — not from a checkout.
- [ ] photon appears in the applications menu (Start menu, Launchpad) with the right name and icon.
- [ ] A freshly installed photon watches the Pictures folder on first launch, with no folder added by hand.
- [ ] On macOS, the permission prompt for the Pictures folder appears. Denying it shows "Live updates limited" in the status bar rather than silently indexing nothing.
- [ ] On Linux, `sudo apt install ./photon_*.deb` pulls in the webview dependencies on a clean machine, and photon starts rather than failing on a missing library.
- [ ] The security warning each OS shows matches what the README's "photon is not code-signed" section says to expect.
- [ ] `sha256sum -c SHA256SUMS --ignore-missing` passes against the downloaded files.
```

- [ ] **Step 4: Record the trigger refinement in the spec**

In `docs/superpowers/specs/2026-09-12-photon-plan4-packaging-design.md` §4, replace the `**Triggers:**` paragraph with:

```markdown
**Triggers:** a `v*` tag, plus `workflow_dispatch` so a release can be rehearsed without
tagging. A dispatch run builds and uploads artifacts but creates no Release. Changes to
`release.yml` itself also build and verify on the pull request, without publishing — a
tag-only workflow cannot be verified before the tag that needs it.
```

- [ ] **Step 5: Commit**

```bash
git add README.md docs/superpowers/specs/2026-09-12-photon-plan4-packaging-design.md
git commit -m "docs: install instructions, unsigned-build warnings and release checklist"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| §2 Licence and metadata | Task 2 |
| §3 The version, in one place | Task 1 (check), Task 4 (runs it in CI) |
| §4 The release workflow | Task 4 |
| §5 What CI proves | Task 4 verify steps |
| §5 What only a human proves | Task 5 checklist |
| §6 What unsigned costs | Task 5 README |
| §7 Version-check tests | Task 1 |
| §7 Watcher follow-ups | Task 3 |
| §7 CI stale comment | Task 4 Step 3 |
| §8 Success criteria | Tasks 2, 4, 5 |

**Type consistency:** `check_versions`, `check_metadata`, `parse_tauri_version`, `parse_cargo_version` and `parse_pkg_version` are defined in Task 1 and Task 2 and used with those exact names in `main.rs` and in the workflow's `cargo run -p xtask --` invocations. `Recorder::all()`, `Recorded::Folder` and `ThumbService::queued()` in Task 3 match the definitions in `events.rs` and `thumbs/service.rs`.

**Known unverifiable-until-CI facts**, flagged rather than asserted:
- Bundle output paths (`target/release/bundle/{deb,appimage,msi}/`, `target/<triple>/release/bundle/dmg/`). The bundler is not vendored in this checkout. Task 4's verify steps glob rather than hardcode filenames and fail loudly, so a wrong path surfaces as a red job naming the missing artifact.
- Whether the AppImage build needs extra tooling on the runner. If it does, the `linux` job fails at the bundling step; the fix is an `apt-get install` line in that job.
- The exact Debian package names in `deb.depends` (`libwebkit2gtk-4.1-0`, `libsoup-3.0-0`). The verify step checks the declared Depends against these prefixes, so a rename shows up as a failed check rather than a broken installer.
