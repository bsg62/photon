//! What photon asks of the system it runs on, beyond the window: a folder chosen in the
//! system's own picker, and - where photon has no window to say it in - a message box.
//! The rest of the platform's calls (the clipboard, the file manager) join this as they
//! are built (spec `2026-10-16-photon-native-platform-design.md`).
//!
//! **One trait, `Platform`, so that a test stands in for the system** (`App::with_platform`,
//! as `App::with_videos` stands in for its video): a portal, a picker and a file manager
//! are things a CI runner has none of, and nothing here is run by the gate against the
//! real system.
//!
//! The picker is the system's own on Windows and macOS, through rfd, the crate Tauri's
//! dialog plugin is built on. On Linux it is the XDG desktop portal over D-Bus, through
//! ashpd, the client rfd itself uses there - pure Rust, no GTK - and where the portal does
//! not answer, `zenity`, as rfd falls back to it; where neither is there, photon says that
//! no picker could be opened, which rfd would have answered as a cancel. rfd is not used
//! on Linux: built there it must have GTK or the portal and refuses both at once, and in a
//! build of the whole workspace Tauri's dialog plugin asks for GTK.

use std::path::PathBuf;

/// What is said where no picker could be opened: the field is the way that is left.
pub const NO_PICKER: &str =
    "photon could not open a folder picker on this desktop: type the folder's path instead.";

/// What came of asking the system for a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picked {
    Folder(PathBuf),
    /// The user closed the picker without choosing.
    Cancelled,
    /// No picker could be opened: no portal, and no `zenity`.
    Unavailable,
}

/// What photon asks of the system. Every call blocks: it is made off the window's thread.
pub trait Platform: Send + Sync + 'static {
    /// A folder chosen in the system's picker, under `title`.
    fn pick_folder(&self, title: &str) -> Picked;
}

/// The system photon runs on.
pub struct System;

impl Platform for System {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn pick_folder(&self, title: &str) -> Picked {
        match rfd::FileDialog::new().set_title(title).pick_folder() {
            Some(folder) => Picked::Folder(folder),
            None => Picked::Cancelled,
        }
    }

    #[cfg(target_os = "linux")]
    fn pick_folder(&self, title: &str) -> Picked {
        match portal_folder(title) {
            Ok(picked) => picked,
            Err(err) => {
                tracing::warn!(%err, "the desktop portal could not open a folder picker");
                zenity_folder(title)
            }
        }
    }
}

/// How long the portal is given to open its picker.
#[cfg(target_os = "linux")]
const PORTAL_PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// The portal's file chooser, asked for a folder.
#[cfg(target_os = "linux")]
fn portal_folder(title: &str) -> Result<Picked, ashpd::Error> {
    use ashpd::desktop::{ResponseError, file_chooser::SelectedFiles};
    let request = SelectedFiles::open_file()
        .title(title)
        .directory(true)
        .multiple(false)
        .modal(true);
    // The call that opens the picker has a time of its own; the user's choosing has none.
    // A portal that accepts the call and never opens anything - one dying as it starts -
    // would otherwise hold the picker open for the session.
    let opened = pollster::block_on(futures_lite::future::or(
        async { Some(request.send().await) },
        async {
            async_io::Timer::after(PORTAL_PATIENCE).await;
            None
        },
    ));
    let Some(opened) = opened else {
        return Err(ashpd::Error::Response(ResponseError::Other));
    };
    let answer = opened.and_then(|request| request.response());
    match answer {
        // A place that is no file of this computer's - a share the desktop opened as
        // `smb://` - goes into the field as it is, which says it is no whole path: chosen
        // and nothing said, it would read as a picker that did nothing.
        Ok(files) => Ok(files.uris().first().map_or(Picked::Cancelled, |uri| {
            Picked::Folder(
                uri.to_file_path()
                    .unwrap_or_else(|()| PathBuf::from(uri.as_str())),
            )
        })),
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(Picked::Cancelled),
        Err(err) => Err(err),
    }
}

/// `zenity`'s folder picker, where the portal did not answer: its path on its standard
/// output, or the status 1 of a cancel.
#[cfg(target_os = "linux")]
fn zenity_folder(title: &str) -> Picked {
    let run = std::process::Command::new("zenity")
        .args(["--file-selection", "--directory", "--title", title])
        .output();
    match run {
        // The path as bytes, as a path on Linux is: not every name is UTF-8.
        Ok(out) if out.status.success() => {
            use std::os::unix::ffi::OsStringExt;
            let mut path = out.stdout;
            while path
                .last()
                .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
            {
                path.pop();
            }
            if path.is_empty() {
                Picked::Cancelled
            } else {
                Picked::Folder(PathBuf::from(std::ffi::OsString::from_vec(path)))
            }
        }
        Ok(out) if out.status.code() == Some(1) => Picked::Cancelled,
        Ok(out) => {
            tracing::warn!(status = ?out.status, "zenity's folder picker failed");
            Picked::Unavailable
        }
        Err(err) => {
            tracing::warn!(%err, "no zenity to pick a folder with");
            Picked::Unavailable
        }
    }
}

/// What is said when the window could not be started: the Tauri photon's words for a
/// library that could not be opened - the one way the application's own making fails -
/// and the error itself for anything else (no window, no adapter).
pub fn startup_failed(err: &eframe::Error) -> String {
    match err {
        eframe::Error::AppCreation(err) => {
            format!("photon could not open its library:\n\n{err}\n\nNothing was changed.")
        }
        err => format!("photon could not start:\n\n{err}"),
    }
}

/// Says `text` in the system's own message box, for when photon has no window to say it
/// in - the library that could not be opened, a window that could not be made. On Linux
/// through `zenity`, where that is installed; where it is not, nothing is shown, which is
/// why the caller also writes it to the standard error and the log.
pub fn show_error(text: &str) {
    // On Windows the system's own box, from `user32`: rfd's is a task dialog wherever
    // Tauri's dialog plugin is in the build (its `common-controls-v6`, which a build of the
    // whole workspace unifies in), and a task dialog is in Common Controls 6 alone, which
    // a program without a manifest asking for it is not given - it would not start.
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let wide = |words: &str| words.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (text, title) = (wide(text), wide("photon"));
        // SAFETY: both are UTF-16 and NUL-terminated, and outlive the call; no owner window.
        unsafe { MessageBoxW(0, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    }
    #[cfg(target_os = "macos")]
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("photon")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    #[cfg(target_os = "linux")]
    {
        let shown = std::process::Command::new("zenity")
            .args([
                "--error",
                "--title",
                "photon",
                "--no-markup",
                "--text",
                text,
            ])
            .status();
        if let Err(err) = shown {
            tracing::warn!(%err, "no zenity to say that photon could not start");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A library that could not be opened is said in the Tauri photon's words, with what
    // was wrong, and that nothing was changed; anything else as what it is.
    #[test]
    fn a_library_that_could_not_be_opened_is_said_so() {
        let err = eframe::Error::AppCreation("the database is locked".into());
        assert_eq!(
            startup_failed(&err),
            "photon could not open its library:\n\nthe database is locked\n\nNothing was changed."
        );
    }
}
