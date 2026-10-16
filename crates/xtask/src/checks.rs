//! The checks `xtask` runs. Pure functions over file contents, so they are unit-tested
//! without touching the repository.
//!
//! The version is the workspace `Cargo.toml`'s alone: photon's installers are built by
//! `xtask package` from it. Until the switch-over the Tauri shell's `tauri.conf.json` and
//! `ui/package.json` said it too, and this held the three to each other.

/// The version the workspace `Cargo.toml` states.
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

/// Checks that the tag that triggered a run, when one did, is exactly `v<version>`.
/// Answers the version.
pub fn check_versions(version: &str, tag: Option<&str>) -> Result<String, Vec<String>> {
    if let Some(tag) = tag {
        let expected = format!("v{version}");
        if tag != expected {
            return Err(vec![format!(
                "tag is {tag}, but Cargo.toml expects {expected}"
            )]);
        }
    }
    Ok(version.to_owned())
}

/// Checks what the installers embed is all there: the licence file, and the workspace
/// package's licence, description, repository and authors (`package.rs` writes the
/// description into the .deb and the desktop entry). Returns every problem rather than
/// the first, so one CI run tells you everything to fix.
pub fn check_metadata(cargo_toml: &str, license_file_exists: bool) -> Result<(), Vec<String>> {
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
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_tag_passes() {
        assert_eq!(check_versions("0.1.0", Some("v0.1.0")).unwrap(), "0.1.0");
    }

    #[test]
    fn a_tag_that_does_not_match_fails() {
        let errs = check_versions("0.1.0", Some("v0.2.0")).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(
            errs[0].contains("v0.2.0") && errs[0].contains("v0.1.0"),
            "got {errs:?}"
        );
    }

    #[test]
    fn a_tag_missing_its_v_prefix_fails() {
        assert!(check_versions("0.1.0", Some("0.1.0")).is_err());
    }

    #[test]
    fn no_tag_passes_the_version_through() {
        assert_eq!(check_versions("0.1.0", None).unwrap(), "0.1.0");
    }

    #[test]
    fn the_version_is_read_from_the_workspace_package() {
        let cargo = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        assert_eq!(parse_cargo_version(cargo).unwrap(), "0.1.0");
    }

    #[test]
    fn a_missing_version_field_is_an_error_not_a_panic() {
        assert!(parse_cargo_version("[workspace]\nmembers = []\n").is_err());
        assert!(parse_cargo_version("not toml at all [").is_err());
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

    #[test]
    fn complete_metadata_passes() {
        assert!(check_metadata(GOOD_CARGO, true).is_ok());
    }

    #[test]
    fn a_missing_license_file_is_reported() {
        let errs = check_metadata(GOOD_CARGO, false).unwrap_err();
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
            let errs = check_metadata(&stripped, true).unwrap_err();
            assert!(errs.iter().any(|e| e.contains(field)), "{field}: {errs:?}");
        }
    }
}
