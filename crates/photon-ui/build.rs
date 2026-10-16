//! Media Foundation's DLLs delay-loaded by photon on Windows (spec
//! `2026-10-16-photon-native-platform-design.md`, decision 8): the `windows` crate links
//! them when the program loads, and a Windows "N" edition without the Media Feature Pack
//! then could not start photon at all. Delay-loaded, they are looked for at the first call,
//! which `video::mf` makes only after loading them itself (`installed`). The packaging
//! workflow reads the built program's imports (`dumpbin /imports`) to hold it. The tests'
//! programs link them as before: they run where Media Foundation is.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows && msvc {
        for dll in ["mfplat.dll", "mfreadwrite.dll"] {
            println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
}
