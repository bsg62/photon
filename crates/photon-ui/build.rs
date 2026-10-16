//! Media Foundation's DLLs delay-loaded by photon on Windows (spec
//! `2026-10-16-photon-native-platform-design.md`, decision 8): the `windows` crate links
//! them when the program loads, and a Windows "N" edition without the Media Feature Pack
//! then could not start photon at all. Delay-loaded, they are looked for at the first call,
//! which `video::mf` makes only after loading them itself (`installed`). The packaging
//! workflow reads the built program's imports (`dumpbin /imports`) to hold it. The tests'
//! programs link them as before: they run where Media Foundation is.
//!
//! And the program's icon, embedded in it on Windows, where Explorer, a shortcut and a
//! button pinned to the taskbar read it from the file - the window's own, set as it opens
//! (`window.rs`), is the running window's alone. The Tauri shell's build embedded it; without
//! it an upgrade showed Windows' blank program in all three. Compiled by the Windows SDK's
//! `rc`, through embed-resource, which Tauri's build used for it too.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if windows {
        let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("icons")
            .join("icon.ico");
        let rc = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("photon.rc");
        // Named by its whole path: `rc` reads a relative one from wherever it is run.
        let quoted = icon.display().to_string().replace('\\', "\\\\");
        std::fs::write(&rc, format!("1 ICON \"{quoted}\"\n")).unwrap();
        embed_resource::compile(&rc, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows && msvc {
        for dll in ["mfplat.dll", "mfreadwrite.dll"] {
            println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
}
