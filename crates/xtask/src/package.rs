//! `xtask package <kind>`: photon's installers, built without Tauri's bundler
//! (spec `2026-10-16-photon-native-platform-design.md`, decision 8). The files an installer
//! carries - the desktop entry, the Debian control file, `Info.plist`, the licence as RTF,
//! the AppImage's `AppRun` - are written by pure functions, tested here as text; the tools
//! that make the packages of them (`dpkg-deb`, `linuxdeploy` and `readelf`, `codesign` and
//! `hdiutil`, WiX's `candle` and `light`) are run by the workflow that calls this,
//! `release.yml`, and by nothing in the gate.
//!
//!   cargo run -p xtask -- package deb      --binary <photon> --out <dir>
//!   cargo run -p xtask -- package appimage --binary <photon> --out <dir>
//!   cargo run -p xtask -- package dmg      --binary <photon> --arch <aarch64|x86_64> --out <dir>
//!   cargo run -p xtask -- package msi      --binary <photon.exe> --out <dir>
//!
//! The program is installed as `photon` (`photon.exe`), where the Tauri photon's installers
//! put theirs, under the identifier the Tauri photon has: one replaces the other.

use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

/// What the system knows photon by: the Tauri photon's identifier, the native window's
/// app id (`photon_ui::dirs::IDENTIFIER`).
pub const IDENTIFIER: &str = "io.github.bsg62.photon";

/// What the `.deb` recommends beside the libraries it depends on: GStreamer's plugins, for
/// video (the binary links GStreamer's libraries, never its plugins); the desktop portal,
/// for the folder picker; Mesa's Vulkan drivers, which carry the software adapter where
/// there is no GPU.
const DEB_RECOMMENDS: [&str; 4] = [
    "gstreamer1.0-plugins-good",
    "gstreamer1.0-libav",
    "xdg-desktop-portal",
    "mesa-vulkan-drivers",
];

/// The desktop entry: the menus' photon.
pub fn desktop_entry(description: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=photon\n\
         Comment={description}\n\
         Exec=photon\n\
         Icon={IDENTIFIER}\n\
         Terminal=false\n\
         Categories=Graphics;Photography;\n\
         StartupWMClass={IDENTIFIER}\n"
    )
}

/// The Debian control file. `depends` is what `dpkg-shlibdeps` reads off the binary.
pub fn deb_control(version: &str, installed_kib: u64, depends: &str, description: &str) -> String {
    format!(
        "Package: photon\n\
         Version: {version}\n\
         Architecture: amd64\n\
         Maintainer: David Henning\n\
         Installed-Size: {installed_kib}\n\
         Depends: {depends}\n\
         Recommends: {}\n\
         Section: graphics\n\
         Priority: optional\n\
         Homepage: https://photon.webcodr.io\n\
         Description: {description}\n \
         photon is a fast, local photo manager and a spiritual successor to Picasa 3.\n \
         It watches folders in place and never moves or changes your files.\n",
        DEB_RECOMMENDS.join(", ")
    )
}

/// The app bundle's `Info.plist`.
pub fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  <key>CFBundleDisplayName</key><string>photon</string>
  <key>CFBundleExecutable</key><string>photon</string>
  <key>CFBundleIconFile</key><string>photon.icns</string>
  <key>CFBundleIdentifier</key><string>{IDENTIFIER}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>photon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>Copyright (c) 2026 David Henning</string>
</dict>
</plist>
"#
    )
}

/// The licence as RTF, for the MSI's licence page: each line a paragraph, the three
/// characters RTF gives a meaning escaped, and anything past ASCII as its code point.
pub fn license_rtf(text: &str) -> String {
    let mut rtf = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Tahoma;}}\\fs16\n");
    for line in text.lines() {
        for c in line.chars() {
            match c {
                '\\' | '{' | '}' => {
                    rtf.push('\\');
                    rtf.push(c);
                }
                c if c.is_ascii() => rtf.push(c),
                c => {
                    let mut units = [0u16; 2];
                    for unit in c.encode_utf16(&mut units) {
                        rtf.push_str(&format!("\\u{}?", *unit as i16));
                    }
                }
            }
        }
        rtf.push_str("\\par\n");
    }
    rtf.push('}');
    rtf
}

/// Where the AppImage keeps the libraries linuxdeploy carried - GStreamer's and the GLib
/// under them - off the binary's runpath (`$ORIGIN/../lib`), so they are found only when
/// `AppRun` names them.
pub const FALLBACK: &str = "usr/fallback";

/// The GStreamer libraries the binary links, which `AppRun` looks for on the host. Every
/// GStreamer library the binary names must be one of them (`appimage` refuses otherwise):
/// the carried copies are off the runpath, so one it comes to link that is not asked after
/// would, on a host without it, leave photon unable to start. What they link in turn
/// (`libgstallocators` under a newer `libgstvideo`) comes with the host's own.
pub const HOST_GSTREAMER: [&str; 4] = [
    "libgstreamer-1.0.so.0",
    "libgstbase-1.0.so.0",
    "libgstvideo-1.0.so.0",
    "libgstapp-1.0.so.0",
];

/// The AppImage's `AppRun`: **the host's GStreamer wherever the host has one**, the carried
/// one only where it has none. The carried libraries are the Ubuntu's the AppImage is built
/// on, and a host's plugins built for a newer GStreamer do not load into them (a symbol of
/// the newer core missing): on Arch's 1.28 every video said GStreamer was not installed.
/// GStreamer keeps its ABI within 1.x, so the binary built against the old one runs on the
/// host's. The host is asked through the loader's cache, and through the usual folders
/// only where there is no cache to ask - a 32-bit library in `/usr/lib` is not the host's
/// 64-bit one. Carried, the libraries look for plugins where the Ubuntu keeps them, so the
/// plugin path is pointed at where distributions put theirs.
pub fn app_run() -> String {
    format!(
        r#"#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
cache="$( (ldconfig -p || /sbin/ldconfig -p) 2>/dev/null)"
on_host() {{
    if [ -n "$cache" ]; then
        case "$cache" in *"$1 (libc6,x86-64)"*) return 0 ;; esac
        return 1
    fi
    for dir in /usr/lib/x86_64-linux-gnu /usr/lib64 /usr/lib; do
        [ -e "$dir/$1" ] && return 0
    done
    return 1
}}
for lib in {libs}; do
    if ! on_host "$lib"; then
        export LD_LIBRARY_PATH="$HERE/{FALLBACK}${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}"
        export GST_PLUGIN_SYSTEM_PATH="${{GST_PLUGIN_SYSTEM_PATH:-/usr/lib/x86_64-linux-gnu/gstreamer-1.0:/usr/lib64/gstreamer-1.0:/usr/lib/gstreamer-1.0}}"
        break
    fi
done
exec "$HERE/usr/bin/photon" "$@"
"#,
        libs = HOST_GSTREAMER.join(" ")
    )
}

/// The workspace's version and description, from `Cargo.toml`.
struct Meta {
    version: String,
    description: String,
}

fn meta(root: &Path) -> Result<Meta, String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|err| format!("cannot read Cargo.toml: {err}"))?;
    let value: toml::Value =
        toml::from_str(&text).map_err(|err| format!("Cargo.toml is not TOML: {err}"))?;
    let package = (value.get("workspace").and_then(|w| w.get("package")))
        .ok_or("Cargo.toml has no [workspace.package]")?;
    let field = |name: &str| {
        (package.get(name).and_then(toml::Value::as_str))
            .map(str::to_owned)
            .ok_or(format!("[workspace.package] has no {name}"))
    };
    Ok(Meta {
        version: field("version")?,
        description: field("description")?,
    })
}

/// The value after `--name`.
fn arg(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at + 1).cloned()
}

fn run_tool(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|err| format!("{:?} could not start: {err}", command.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{:?} failed: {status}", command.get_program()))
    }
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|err| format!("{} to {}: {err}", from.display(), to.display()))
}

/// A directory of its own under the output directory, made empty.
fn fresh(out: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = out.join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    Ok(dir)
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let done = (|| -> Result<PathBuf, String> {
        let kind = args
            .get(1)
            .ok_or("which package? deb, appimage, dmg or msi")?;
        let binary = PathBuf::from(arg(args, "--binary").ok_or("--binary <path> is needed")?);
        let out = PathBuf::from(arg(args, "--out").ok_or("--out <dir> is needed")?);
        std::fs::create_dir_all(&out).map_err(|err| format!("{}: {err}", out.display()))?;
        // Whole paths: dpkg-shlibdeps, linuxdeploy and WiX's light are each started in a
        // folder of their own, and a path given as `dist/...` was read from there.
        let absolute = |path: &Path| {
            std::path::absolute(path).map_err(|err| format!("{}: {err}", path.display()))
        };
        let (binary, out) = (absolute(&binary)?, absolute(&out)?);
        let meta = meta(root)?;
        let icons = root.join("crates/photon-ui/icons");
        match kind.as_str() {
            "deb" => deb(&meta, &binary, &icons, &out),
            "appimage" => appimage(&meta, &binary, &icons, &out),
            "dmg" => {
                let arch = arg(args, "--arch").ok_or("--arch <aarch64|x86_64> is needed")?;
                dmg(&meta, &binary, &icons, &arch, &out)
            }
            "msi" => msi(root, &meta, &binary, &icons, &out),
            other => Err(format!("no package called {other}")),
        }
    })();
    match done {
        Ok(made) => {
            println!("{}", made.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("package: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The icons a Linux desktop finds the program's by, by size.
fn hicolor(tree: &Path, icons: &Path) -> Result<(), String> {
    for (size, file) in [
        ("32x32", "32x32.png"),
        ("128x128", "128x128.png"),
        ("256x256", "photon-256.png"),
    ] {
        let to = tree.join(format!(
            "usr/share/icons/hicolor/{size}/apps/{IDENTIFIER}.png"
        ));
        copy(&icons.join(file), &to)?;
    }
    Ok(())
}

fn deb(meta: &Meta, binary: &Path, icons: &Path, out: &Path) -> Result<PathBuf, String> {
    let tree = fresh(out, "deb-root")?;
    copy(binary, &tree.join("usr/bin/photon"))?;
    write(
        &tree.join(format!("usr/share/applications/{IDENTIFIER}.desktop")),
        &desktop_entry(&meta.description),
    )?;
    hicolor(&tree, icons)?;
    // What the binary links, as Debian names the packages that hold it. dpkg-shlibdeps
    // wants a debian/control beside it, which it reads nothing from here but refuses
    // without a binary stanza's Architecture.
    let shlibs = fresh(out, "shlibs")?;
    write(
        &shlibs.join("debian/control"),
        "Source: photon\n\nPackage: photon\nArchitecture: amd64\n",
    )?;
    let found = Command::new("dpkg-shlibdeps")
        .arg("-O")
        .arg(format!("-e{}", tree.join("usr/bin/photon").display()))
        .current_dir(&shlibs)
        .output()
        .map_err(|err| format!("dpkg-shlibdeps could not start: {err}"))?;
    if !found.status.success() {
        return Err(format!(
            "dpkg-shlibdeps failed: {}",
            String::from_utf8_lossy(&found.stderr)
        ));
    }
    let depends = String::from_utf8_lossy(&found.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("shlibs:Depends="))
        .ok_or("dpkg-shlibdeps named no dependency")?
        .to_owned();
    let size = dir_size(&tree)? / 1024;
    write(
        &tree.join("DEBIAN/control"),
        &deb_control(&meta.version, size, &depends, &meta.description),
    )?;
    let deb = out.join(format!("photon_{}_amd64.deb", meta.version));
    run_tool(
        Command::new("dpkg-deb")
            .arg("--build")
            .arg("--root-owner-group")
            .arg(&tree)
            .arg(&deb),
    )?;
    Ok(deb)
}

fn dir_size(dir: &Path) -> Result<u64, String> {
    let mut size = 0;
    for entry in std::fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| err.to_string())?;
        let kind = entry.file_type().map_err(|err| err.to_string())?;
        size += if kind.is_dir() {
            dir_size(&entry.path())?
        } else {
            entry.metadata().map_err(|err| err.to_string())?.len()
        };
    }
    Ok(size)
}

fn appimage(meta: &Meta, binary: &Path, icons: &Path, out: &Path) -> Result<PathBuf, String> {
    let app_dir = fresh(out, "photon.AppDir")?;
    copy(binary, &app_dir.join("usr/bin/photon"))?;
    let desktop = out.join(format!("{IDENTIFIER}.desktop"));
    write(&desktop, &desktop_entry(&meta.description))?;
    let runner = out.join("AppRun");
    write(&runner, &app_run())?;
    let icon = out.join(format!("{IDENTIFIER}.png"));
    copy(&icons.join("photon-256.png"), &icon)?;
    let linuxdeploy = std::env::var_os("LINUXDEPLOY").unwrap_or_else(|| "linuxdeploy".into());
    let deploy = |excluded: &[String], output: bool| {
        let mut command = Command::new(&linuxdeploy);
        command
            .arg("--appdir")
            .arg(&app_dir)
            .arg("--executable")
            .arg(app_dir.join("usr/bin/photon"))
            .arg("--desktop-file")
            .arg(&desktop)
            .arg("--icon-file")
            .arg(&icon)
            .arg("--custom-apprun")
            .arg(&runner)
            .env("LINUXDEPLOY_OUTPUT_VERSION", &meta.version)
            .env("ARCH", "x86_64")
            .current_dir(out);
        for name in excluded {
            command.arg("--exclude-library").arg(name);
        }
        if output {
            command.arg("--output").arg("appimage");
        }
        run_tool(&mut command)
    };
    // linuxdeploy carries the libraries the binary links - GStreamer's among them, never
    // its plugins. They are moved off the runpath for `AppRun` to name only where the host
    // has no GStreamer, and the second pass, which makes the AppImage (and names it from
    // these), is told not to carry them again.
    for name in linked(binary)? {
        if name.starts_with("libgst") && !HOST_GSTREAMER.contains(&name.as_str()) {
            return Err(format!(
                "photon links {name}, which AppRun does not look for on the host"
            ));
        }
    }
    deploy(&[], false)?;
    let lib = app_dir.join("usr/lib");
    let fallback = app_dir.join(FALLBACK);
    std::fs::create_dir_all(&fallback).map_err(|err| format!("{}: {err}", fallback.display()))?;
    let mut carried = Vec::new();
    for entry in std::fs::read_dir(&lib).map_err(|err| format!("{}: {err}", lib.display()))? {
        let path = entry.map_err(|err| err.to_string())?.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::rename(&path, fallback.join(&name))
            .map_err(|err| format!("{}: {err}", path.display()))?;
        carried.push(name);
    }
    deploy(&carried, true)?;
    let made = out.join(format!("photon-{}-x86_64.AppImage", meta.version));
    let named = out.join(format!("photon_{}_amd64.AppImage", meta.version));
    std::fs::rename(&made, &named).map_err(|err| format!("{}: {err}", made.display()))?;
    Ok(named)
}

/// The libraries an ELF file names (`DT_NEEDED`), as `readelf -d` lists them.
fn linked(binary: &Path) -> Result<Vec<String>, String> {
    let said = Command::new("readelf")
        .arg("-d")
        .arg(binary)
        .output()
        .map_err(|err| format!("readelf could not start: {err}"))?;
    if !said.status.success() {
        return Err(format!("readelf failed: {}", said.status));
    }
    Ok(needed(&String::from_utf8_lossy(&said.stdout)))
}

fn needed(dynamic: &str) -> Vec<String> {
    dynamic
        .lines()
        .filter(|line| line.contains("(NEEDED)"))
        .filter_map(|line| Some(line.split_once('[')?.1.split_once(']')?.0.to_owned()))
        .collect()
}

/// The size of a `.dmg`'s volume for `bytes` of files, in megabytes: a quarter more, and
/// 32 MB for the file system's own. The image is compressed (`UDZO`), so the room left over
/// costs the download nothing.
fn dmg_megabytes(bytes: u64) -> u64 {
    (bytes + bytes / 4).div_ceil(1 << 20) + 32
}

/// The bytes of the files under `dir`, a link counted as nothing (the `Applications` link
/// would otherwise be the whole disk's).
fn bytes_under(dir: &Path) -> Result<u64, String> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))? {
        let path = entry.map_err(|err| err.to_string())?.path();
        let meta =
            std::fs::symlink_metadata(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        if meta.is_dir() {
            total += bytes_under(&path)?;
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
}

fn dmg(
    meta: &Meta,
    binary: &Path,
    icons: &Path,
    arch: &str,
    out: &Path,
) -> Result<PathBuf, String> {
    let name = match arch {
        "aarch64" => "aarch64",
        "x86_64" => "x64",
        other => return Err(format!("no Mac of the architecture {other}")),
    };
    let stage = fresh(out, &format!("dmg-{name}"))?;
    let app = stage.join("photon.app/Contents");
    copy(binary, &app.join("MacOS/photon"))?;
    write(&app.join("Info.plist"), &info_plist(&meta.version))?;
    copy(&icons.join("icon.icns"), &app.join("Resources/photon.icns"))?;
    // Signed ad hoc: an arm64 program that is not signed at all is not run.
    run_tool(
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(stage.join("photon.app")),
    )?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("/Applications", stage.join("Applications"))
        .map_err(|err| format!("the Applications link: {err}"))?;
    let dmg = out.join(format!("photon_{}_{name}.dmg", meta.version));
    // The volume's size is said, not left to `-srcfolder`'s estimate, which fell short of
    // the x86_64 program by a little and failed every try with "No space left on device"
    // (#228's run, while the aarch64 one and main's passed).
    let size = format!("{}m", dmg_megabytes(bytes_under(&stage)?));
    // `hdiutil create` fails now and then on a runner with "Resource busy", and succeeds
    // when asked again (Tauri's own dmg script asks again too).
    let mut tries = 0;
    loop {
        let made = run_tool(
            Command::new("hdiutil")
                .args([
                    "create",
                    "-volname",
                    "photon",
                    "-ov",
                    "-format",
                    "UDZO",
                    "-size",
                    &size,
                    "-srcfolder",
                ])
                .arg(&stage)
                .arg(&dmg),
        );
        tries += 1;
        match made {
            Ok(()) => break,
            Err(err) if tries >= 3 => return Err(err),
            Err(err) => {
                eprintln!("{err}; trying again");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
    Ok(dmg)
}

fn msi(
    root: &Path,
    meta: &Meta,
    binary: &Path,
    icons: &Path,
    out: &Path,
) -> Result<PathBuf, String> {
    let work = fresh(out, "msi-work")?;
    let sources = root.join("crates/photon-ui/packaging/windows");
    for file in ["main.wxs", "install-dir-ui.wxs", "en-US.wxl"] {
        copy(&sources.join(file), &work.join(file))?;
    }
    let licence = std::fs::read_to_string(root.join("LICENSE"))
        .map_err(|err| format!("cannot read LICENSE: {err}"))?;
    write(&work.join("license.rtf"), &license_rtf(&licence))?;
    copy(&icons.join("icon.ico"), &work.join("icon.ico"))?;
    copy(binary, &work.join("photon.exe"))?;
    // WiX 3: where its installer sets WIX to, when it is there; else on the PATH, where
    // the workflow puts the toolset's own binaries.
    let tool = |name: &str| {
        (std::env::var_os("WIX").map(|wix| PathBuf::from(wix).join("bin").join(name)))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from(name))
    };
    run_tool(
        Command::new(tool("candle.exe"))
            .args(["-nologo", "-arch", "x64", "-ext", "WixUIExtension"])
            .arg(format!("-dVersion={}", meta.version))
            .args([
                "-dBinary=photon.exe",
                "-dIcon=icon.ico",
                "-dLicenseRtf=license.rtf",
            ])
            .args(["main.wxs", "install-dir-ui.wxs"])
            .current_dir(&work),
    )?;
    let msi = out.join(format!("photon_{}_x64_en-US.msi", meta.version));
    run_tool(
        Command::new(tool("light.exe"))
            .args([
                "-nologo",
                "-ext",
                "WixUIExtension",
                "-cultures:en-US",
                "-loc",
                "en-US.wxl",
            ])
            .args(["main.wixobj", "install-dir-ui.wixobj", "-out"])
            .arg(&msi)
            .current_dir(&work),
    )?;
    Ok(msi)
}

#[cfg(test)]
mod tests {
    // A quarter more than the files, and 32 MB beside: never less than they need.
    #[test]
    fn a_dmgs_volume_has_room_for_its_files_and_more() {
        assert_eq!(dmg_megabytes(0), 32);
        assert_eq!(dmg_megabytes(80 << 20), 132);
        assert!(dmg_megabytes(80 << 20) << 20 > (80 << 20) * 5 / 4);
    }

    // What is under a folder, its subfolders' too, and nothing a link points at.
    #[cfg(unix)]
    #[test]
    fn the_bytes_under_a_folder_count_no_link() {
        let dir = std::env::temp_dir().join(format!("photon-xtask-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/one"), [0; 100]).unwrap();
        std::fs::write(dir.join("a/b/two"), [0; 50]).unwrap();
        std::os::unix::fs::symlink("/usr", dir.join("link")).unwrap();
        let counted = bytes_under(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(counted.unwrap(), 150);
    }

    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    // The desktop entry names photon, runs the program the packages install, and is the
    // window's own: a desktop matches a window to its entry by the window's app id.
    #[test]
    fn the_desktop_entry_names_photon_and_its_window() {
        let entry = desktop_entry("A photo manager.");
        assert!(entry.starts_with("[Desktop Entry]\n"));
        for line in [
            "Name=photon",
            "Exec=photon",
            "Type=Application",
            "Comment=A photo manager.",
            "Icon=io.github.bsg62.photon",
            "StartupWMClass=io.github.bsg62.photon",
            "Categories=Graphics;Photography;",
        ] {
            assert!(entry.lines().any(|l| l == line), "{line}");
        }
        let window = std::fs::read_to_string(root().join("crates/photon-ui/src/dirs.rs")).unwrap();
        assert!(
            window.contains(&format!("\"{IDENTIFIER}\"")),
            "the window's app id"
        );
    }

    // The control file carries the version and what the binary links, and recommends what
    // video, the folder picker and a machine with no GPU want.
    #[test]
    fn the_control_file_depends_on_what_the_binary_links() {
        let control = deb_control(
            "1.2.3",
            9000,
            "libc6 (>= 2.35), libgstreamer1.0-0",
            "A photo manager.",
        );
        for line in [
            "Package: photon",
            "Version: 1.2.3",
            "Architecture: amd64",
            "Installed-Size: 9000",
            "Depends: libc6 (>= 2.35), libgstreamer1.0-0",
            "Section: graphics",
            "Description: A photo manager.",
        ] {
            assert!(control.lines().any(|l| l == line), "{line}");
        }
        let recommends = control
            .lines()
            .find(|l| l.starts_with("Recommends: "))
            .unwrap();
        for package in [
            "gstreamer1.0-plugins-good",
            "gstreamer1.0-libav",
            "xdg-desktop-portal",
        ] {
            assert!(recommends.contains(package), "{package}");
        }
        // A description's further lines start with a space, or dpkg-deb refuses the file.
        let after = control
            .split("Description: A photo manager.\n")
            .nth(1)
            .unwrap();
        assert!(after.lines().all(|l| l.starts_with(' ')), "{after:?}");
    }

    #[test]
    fn the_app_bundle_is_photon_at_its_version() {
        let plist = info_plist("1.2.3");
        for part in [
            "<key>CFBundleExecutable</key><string>photon</string>",
            "<key>CFBundleIdentifier</key><string>io.github.bsg62.photon</string>",
            "<key>CFBundleShortVersionString</key><string>1.2.3</string>",
            "<key>CFBundleIconFile</key><string>photon.icns</string>",
        ] {
            assert!(plist.contains(part), "{part}");
        }
    }

    // The licence's braces and backslashes are not RTF's, and a letter past ASCII is kept.
    #[test]
    fn the_licence_is_rtf_with_its_own_characters() {
        let rtf = license_rtf("a {b} \\c\nCopyright © 2026");
        assert!(rtf.starts_with("{\\rtf1"));
        assert!(rtf.contains("a \\{b\\} \\\\c\\par\n"));
        assert!(rtf.contains("Copyright \\u169? 2026\\par\n"));
        assert!(rtf.ends_with('}'));
    }

    #[test]
    fn the_libraries_a_binary_names_are_read_off_readelf() {
        let dynamic = " 0x0000000000000001 (NEEDED)             Shared library: [libgstvideo-1.0.so.0]\n \
                       0x000000000000001d (RUNPATH)            Library runpath: [$ORIGIN/../lib]\n \
                       0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n";
        assert_eq!(needed(dynamic), ["libgstvideo-1.0.so.0", "libc.so.6"]);
    }

    /// `AppRun` in an AppDir of its own over a `photon` that says what it was given, on a
    /// host whose loader's cache is `cache`, run with `LD_LIBRARY_PATH` set to `ld_path`.
    #[cfg(target_os = "linux")]
    fn app_run_on(test: &str, cache: &str, ld_path: Option<&str>) -> (PathBuf, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("photon-apprun-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let script = |path: PathBuf, text: &str| {
            write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        script(dir.join("AppDir/AppRun"), &app_run());
        script(
            dir.join("AppDir/usr/bin/photon"),
            "#!/bin/sh\necho \"lib=${LD_LIBRARY_PATH-unset}\"\n\
             echo \"gst=${GST_PLUGIN_SYSTEM_PATH-unset}\"\necho \"args=$*\"\n",
        );
        script(
            dir.join("bin/ldconfig"),
            &format!("#!/bin/sh\ncat <<'END'\n{cache}END\n"),
        );
        let mut command = Command::new(dir.join("AppDir/AppRun"));
        command.args(["a b", "c"]).env_clear().env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("bin").display()),
        );
        if let Some(ld_path) = ld_path {
            command.env("LD_LIBRARY_PATH", ld_path);
        }
        let said = command.output().unwrap();
        assert!(
            said.status.success(),
            "{}",
            String::from_utf8_lossy(&said.stderr)
        );
        let app_dir = std::fs::canonicalize(dir.join("AppDir")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        (app_dir, String::from_utf8(said.stdout).unwrap())
    }

    /// A loader's cache listing `libs` as 64-bit, and one 32-bit library beside them.
    #[cfg(target_os = "linux")]
    fn cache_of(libs: &[&str]) -> String {
        let mut cache = String::from("5 libs found in cache `/etc/ld.so.cache'\n");
        cache.push_str("\tlibgstapp-1.0.so.0 (libc6) => /usr/lib32/libgstapp-1.0.so.0\n");
        for lib in libs {
            cache.push_str(&format!("\t{lib} (libc6,x86-64) => /usr/lib/{lib}\n"));
        }
        cache
    }

    // A host with GStreamer runs photon on its own: the carried 1.20 refused Arch's 1.28
    // plugins, and every video said GStreamer was missing.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_appimage_uses_the_hosts_gstreamer_where_the_host_has_one() {
        let (_, said) = app_run_on("host", &cache_of(&HOST_GSTREAMER), None);
        assert_eq!(said, "lib=unset\ngst=unset\nargs=a b c\n");
    }

    // A host lacking any one of them - here with only a 32-bit libgstapp - runs photon on
    // the carried libraries, the user's own path kept behind them, plugins looked for where
    // distributions put them.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_appimage_uses_its_own_gstreamer_where_the_host_lacks_one() {
        let (app_dir, said) = app_run_on("lacks", &cache_of(&HOST_GSTREAMER[..3]), Some("/mine"));
        let fallback = app_dir.join(FALLBACK);
        assert_eq!(
            said,
            format!(
                "lib={}:/mine\ngst=/usr/lib/x86_64-linux-gnu/gstreamer-1.0:/usr/lib64/gstreamer-1.0:\
                 /usr/lib/gstreamer-1.0\nargs=a b c\n",
                fallback.display()
            )
        );
    }

    // The MSI is photon's Tauri MSI's successor: its upgrade code (a UUID v5 of
    // "photon.exe.app.x64" in the DNS namespace, Tauri's rule, which `tauri inspect
    // wix-upgrade-code` answers), the opt-in desktop shortcut, and nothing of Tauri's
    // template left unfilled.
    #[test]
    fn the_msi_replaces_the_tauri_one_and_keeps_its_opt_in_shortcut() {
        let wxs =
            std::fs::read_to_string(root().join("crates/photon-ui/packaging/windows/main.wxs"))
                .unwrap();
        assert!(wxs.contains("UpgradeCode=\"0ccaa18b-f3ff-50ce-ad76-6fb4d33f39b7\""));
        assert!(wxs.contains("<Condition>DESKTOPSHORTCUT</Condition>"));
        assert!(wxs.contains("<Property Id=\"DESKTOPSHORTCUT\" Secure=\"yes\" />"));
        assert!(wxs.contains("Name=\"photon.exe\""));
        assert!(
            !wxs.contains("{{"),
            "a placeholder of Tauri's template is left"
        );
        let strings =
            std::fs::read_to_string(root().join("crates/photon-ui/packaging/windows/en-US.wxl"))
                .unwrap();
        // Every string main.wxs asks its localisation for is there.
        for id in wxs
            .split("!(loc.")
            .skip(1)
            .map(|rest| &rest[..rest.find(')').unwrap()])
        {
            assert!(strings.contains(&format!("<String Id=\"{id}\">")), "{id}");
        }
    }

    // The icons every package takes are in the place the packages take them from.
    #[test]
    fn the_icons_are_there() {
        let icons = root().join("crates/photon-ui/icons");
        for file in [
            "32x32.png",
            "128x128.png",
            "photon-256.png",
            "icon.icns",
            "icon.ico",
        ] {
            assert!(icons.join(file).is_file(), "{file}");
        }
    }
}
