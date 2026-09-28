fn main() {
    let mut windows = tauri_build::WindowsAttributes::new();
    // tauri-build embeds its app manifest as a resource linked into the binary only, so the
    // test binary runs without the Common Controls v6 dependency it declares. Built with
    // `tauri`'s `test` feature (the mock runtime `ipc.rs`'s tests dispatch through), that
    // binary imports comctl32 entry points only v6 has, and Windows refuses to start it:
    // STATUS_ENTRYPOINT_NOT_FOUND, before a single test runs. Given to the linker instead,
    // the same manifest is embedded in every binary this crate links, tests included.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        let manifest = std::env::current_dir()
            .unwrap()
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("tauri-build failed");
}
