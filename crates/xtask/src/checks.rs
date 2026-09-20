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

/// Both JSON manifests keep the version in the same place, so they are read the same way;
/// `file` only names the one that was wrong in the error.
fn parse_json_version(json: &str, file: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("{file} is not valid JSON: {e}"))?;
    v.get("version")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{file} has no string `version` field"))
}

pub fn parse_tauri_version(json: &str) -> Result<String, String> {
    parse_json_version(json, "tauri.conf.json")
}

pub fn parse_pkg_version(json: &str) -> Result<String, String> {
    parse_json_version(json, "package.json")
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
            problems.push(format!(
                "tag is {tag}, but tauri.conf.json expects {expected}"
            ));
        }
    }
    if problems.is_empty() {
        Ok(v.tauri.clone())
    } else {
        Err(problems)
    }
}

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
            // The installed executable's name. Left unset it silently takes the *crate*
            // name, which is how photon shipped `photon-app` in every installer up to
            // 0.19.1. It must also match the `[[bin]]` name in the crate, or the bundler
            // cannot find the binary it is told to ship.
            match v.get("mainBinaryName").and_then(|n| n.as_str()) {
                Some(name) if Some(name) == v.get("productName").and_then(|n| n.as_str()) => {}
                Some(name) => problems.push(format!(
                    "tauri.conf.json `mainBinaryName` is `{name}`, which is not the productName"
                )),
                None => problems.push(
                    "tauri.conf.json has no `mainBinaryName`; the installed binary would take \
                     the crate name"
                        .to_owned(),
                ),
            }
            if bundle
                .and_then(|b| b.get("createUpdaterArtifacts"))
                .is_some()
            {
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

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

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
        assert!(
            errs[0].contains("v0.2.0") && errs[0].contains("v0.1.0"),
            "got {:?}",
            errs
        );
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
      "productName": "photon",
      "mainBinaryName": "photon",
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

    /// The installed executable was `photon-app` in every installer up to 0.19.1, because
    /// `mainBinaryName` was unset and it silently took the crate name. Nothing failed - the
    /// app worked - so only a check like this one keeps it named.
    #[test]
    fn a_binary_name_that_is_missing_or_not_the_product_name_is_reported() {
        let missing = GOOD_CONF.replace(r#""mainBinaryName": "photon","#, "");
        let errs = check_metadata(GOOD_CARGO, &missing, true).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("mainBinaryName")),
            "got {errs:?}"
        );

        let wrong = GOOD_CONF.replace(
            r#""mainBinaryName": "photon""#,
            r#""mainBinaryName": "photon-app""#,
        );
        let errs = check_metadata(GOOD_CARGO, &wrong, true).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("photon-app")),
            "got {errs:?}"
        );
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
        assert!(
            errs.iter().any(|e| e.contains("createUpdaterArtifacts")),
            "got {errs:?}"
        );
    }
}
