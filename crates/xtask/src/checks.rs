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
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| format!("tauri.conf.json is not valid JSON: {e}"))?;
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
}
