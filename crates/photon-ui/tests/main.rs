//! The tests of the whole application, one module for each file beside this one, in one
//! binary (`Cargo.toml` says why). `cargo test -p photon-ui --test all dialogs::` runs one
//! file's.

mod common;

mod app;
mod band;
mod compare;
mod compare_edges;
mod dialogs;
mod drop;
mod edits;
mod edits_edges;
mod export;
mod faces;
mod faces_edges;
mod lists;
mod menu;
mod posters;
mod probe;
mod screenshots;
mod search;
mod selection;
mod shell;
mod side;
mod slideshow;
mod slideshow_edges;
mod sort;
mod source;
mod status;
mod viewer;

// With `autotests = false` a file in `tests/` that is not a module here is not built at all,
// and its tests pass by never running.
#[test]
fn every_file_in_tests_is_a_module_of_the_binary() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    // By line: a Windows checkout reads this file with CRLF endings.
    let main = std::fs::read_to_string(tests.join("main.rs")).unwrap();
    let declared: Vec<&str> = main
        .lines()
        .filter_map(|line| line.trim().strip_prefix("mod ")?.strip_suffix(';'))
        .collect();
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(&tests).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let module = if path.is_dir() {
            path.join("mod.rs").is_file()
        } else {
            path.extension().is_some_and(|ext| ext == "rs") && name != "main"
        };
        if module && !declared.contains(&name.as_str()) {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "not modules of tests/main.rs: {missing:?}"
    );
}
