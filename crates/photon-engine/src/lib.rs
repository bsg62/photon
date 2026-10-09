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
