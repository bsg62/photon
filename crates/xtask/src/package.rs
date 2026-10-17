//! `xtask package <kind>`: photon's installers, built without Tauri's bundler
//! (spec `2026-10-16-photon-native-platform-design.md`, decision 8). The files an installer
//! carries - the desktop entry, the Debian control file, `Info.plist`, the licence as RTF,
//! the AppImage's `AppRun` - are written by pure functions, tested here as text; the tools
//! that make the packages of them (`dpkg-deb`, `linuxdeploy`, `codesign` and `hdiutil`, WiX's
//! `candle` and `light`) are run by the workflow that calls this, `release.yml`, and
//! by nothing in the gate.
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

/// The AppImage's `AppRun`: GStreamer's plugin path pointed at where distributions put
/// their plugins, since the GStreamer libraries the AppImage carries look where the Ubuntu
/// they were built on keeps them. A host's plugin built for a newer GStreamer than the one
/// carried can refuse to load, and the viewer then says the plugins are missing.
pub fn app_run() -> String {
    "#!/bin/sh\n\
     HERE=\"$(dirname \"$(readlink -f \"$0\")\")\"\n\
     export GST_PLUGIN_SYSTEM_PATH=\"${GST_PLUGIN_SYSTEM_PATH:-/usr/lib/x86_64-linux-gnu/gstreamer-1.0:/usr/lib64/gstreamer-1.0:/usr/lib/gstreamer-1.0}\"\n\
     exec \"$HERE/usr/bin/photon\" \"$@\"\n"
        .to_owned()
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
    // linuxdeploy carries the libraries the binary links - GStreamer's among them, never
    // its plugins - and names the AppImage from these.
    run_tool(
        Command::new(linuxdeploy)
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
            .arg("--output")
            .arg("appimage")
            .env("LINUXDEPLOY_OUTPUT_VERSION", &meta.version)
            .env("ARCH", "x86_64")
            .current_dir(out),
    )?;
    let made = out.join(format!("photon-{}-x86_64.AppImage", meta.version));
    let named = out.join(format!("photon_{}_amd64.AppImage", meta.version));
    std::fs::rename(&made, &named).map_err(|err| format!("{}: {err}", made.display()))?;
    Ok(named)
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
    fn the_appimage_runs_photon_with_the_hosts_gstreamer_plugins() {
        let run = app_run();
        assert!(run.starts_with("#!/bin/sh\n"));
        assert!(run.contains("GST_PLUGIN_SYSTEM_PATH"));
        assert!(run.contains("exec \"$HERE/usr/bin/photon\" \"$@\""));
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
