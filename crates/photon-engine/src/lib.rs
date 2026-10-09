//! photon-engine: `Engine`, the commands over it and the folder watcher. It depends on no
//! UI runtime (`the_engine_depends_on_no_ui_runtime`), so more than one shell can stand on
//! it. The shell it has today is photon-app, which its comments name where they say why.

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
    /// The crates a manifest depends on, by every name it gives them: an entry's key, and
    /// the `package` it stands for when it is renamed. Text and not `cargo metadata`, so the
    /// test needs no cargo at run time; what it reads is every spelling Cargo accepts for a
    /// direct dependency, which `dependency_names_reads_every_spelling_of_an_entry` lists.
    fn dependency_names(manifest: &str) -> Vec<String> {
        let unquoted = |text: &str| text.trim().trim_matches(['"', '\'']).to_owned();
        let mut names = Vec::new();
        // In `[dependencies]`, where each line is an entry; or in `[dependencies.name]`,
        // where each line is a field of one.
        let (mut entries, mut fields) = (false, false);
        for line in manifest.lines().map(str::trim) {
            if line.starts_with('#') {
                continue;
            }
            if let Some(header) = line.strip_prefix('[') {
                // Up to the bracket, not to the end of the line: a comment may follow.
                let header = header.split(']').next().unwrap_or_default();
                entries = header.ends_with("dependencies");
                fields = false;
                if let Some((table, name)) = header.rsplit_once('.')
                    && table.ends_with("dependencies")
                {
                    names.push(unquoted(name));
                    fields = true;
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if entries {
                // `tokio.workspace = true` is an entry named tokio.
                let (name, field) = key.split_once('.').unwrap_or((key, ""));
                names.push(unquoted(name));
                if field == "package" {
                    names.push(unquoted(value));
                } else if let Some(package) = renamed_in(value) {
                    names.push(package);
                }
            } else if fields && key == "package" {
                names.push(unquoted(value));
            }
        }
        names
    }

    /// The crate an inline table renames its entry from: `{ package = "tokio", .. }`.
    fn renamed_in(inline: &str) -> Option<String> {
        let (_, after) = inline.split_once("package")?;
        let quoted = after.trim_start().strip_prefix('=')?.trim_start();
        Some(quoted.strip_prefix('"')?.split('"').next()?.to_owned())
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

    // Every way Cargo lets a dependency be written, since the first one this test does not
    // read is the one a `tokio` will arrive in: a dotted key (`tokio.workspace = true`, the
    // likeliest, once two shells share `[workspace.dependencies]`), a quoted key, a crate
    // renamed with `package`, and a table header with a comment after it.
    #[test]
    fn dependency_names_reads_every_spelling_of_an_entry() {
        let manifest = "[dependencies]\n\
                        a = \"1\"\n\
                        # not = \"a dependency\"\n\
                        b.workspace = true\n\
                        \"c\" = \"1\"\n\
                        d = { package = \"e\", version = \"1\" }\n\
                        f.package = \"g\"\n\
                        [target.'cfg(unix)'.dependencies] # a comment\n\
                        h = \"1\"\n\
                        [dev-dependencies.i]\n\
                        package = \"j\"\n\
                        version = \"1\"\n\
                        [features]\n\
                        k = [\"dep:a\"]\n";
        assert_eq!(
            dependency_names(manifest),
            ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
        );
    }

    #[test]
    fn dependency_names_reads_both_spellings_of_a_table() {
        let manifest = "[package]\nname = \"x\"\n\n[dependencies]\na = \"1\"\n\n\
                        [target.'cfg(unix)'.dependencies]\nb = \"1\"\n\n\
                        [dev-dependencies.c]\nversion = \"1\"\n";
        assert_eq!(dependency_names(manifest), ["a", "b", "c"]);
    }
}
