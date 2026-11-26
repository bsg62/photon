//! What photon asks of the system it runs on, beyond the window: a folder chosen in the
//! system's own picker, a picture put on the clipboard, and - where photon has no window
//! to say it in - a message box.
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
//! zbus directly (pure Rust, no GTK) so the timeout bounds only the `OpenFile` call that
//! opens the picker and never the user's browsing time; where the portal does not answer,
//! `zenity`, as rfd falls back to it; where neither is there, photon says that no picker
//! could be opened, which rfd would have answered as a cancel. rfd is not used on Linux:
//! built there it must have GTK or the portal, and refuses both at once.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

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

    /// A picture, `width` by `height` in RGBA, put on the clipboard; or why not.
    fn copy_image(&self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String>;

    /// `path` shown in the system's file manager, selected in its folder; or why not.
    fn reveal(&self, path: &Path) -> Result<(), String>;

    /// `target` - a file, or a web address - opened by whatever the system opens it with.
    /// photon registers no file types, so a photo is never opened in photon itself.
    fn open(&self, target: &OsStr) -> Result<(), String>;
}

/// The system photon runs on.
#[derive(Default)]
pub struct System {
    clipboard: Clipboard,
}

impl Platform for System {
    fn copy_image(&self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        self.clipboard.copy_image(width, height, rgba)
    }

    fn reveal(&self, path: &Path) -> Result<(), String> {
        reveal(path)
    }

    fn open(&self, target: &OsStr) -> Result<(), String> {
        open::that_detached(target).map_err(|err| format!("Couldn't open it: {err}"))
    }

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

/// The clipboard, kept for the session by a thread of its own. On Linux what was copied
/// lives only as long as the clipboard that copied it, unless a clipboard manager takes it
/// over: one made for each copy and let go of with it would have lost every copy. A thread,
/// and not the window's own clipboard (`Context::copy_image`): arboard's X11 side makes a
/// PNG of the picture as it is put there, a frame's worth of work many times over.
#[derive(Default)]
struct Clipboard {
    jobs: std::sync::Mutex<Option<std::sync::mpsc::Sender<Job>>>,
}

/// A picture to put on the clipboard, and where to say how it went.
struct Job {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    done: std::sync::mpsc::Sender<Result<(), String>>,
}

impl Clipboard {
    fn copy_image(&self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        let (done, answer) = std::sync::mpsc::channel();
        let job = Job {
            width,
            height,
            rgba,
            done,
        };
        let sent = {
            let mut jobs = self
                .jobs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let jobs = jobs.get_or_insert_with(keeper);
            jobs.send(job)
        };
        if sent.is_err() {
            return Err(COPY_FAILED.to_owned());
        }
        answer
            .recv()
            .unwrap_or_else(|_| Err(COPY_FAILED.to_owned()))
    }
}

/// What is said when the clipboard's thread is gone.
const COPY_FAILED: &str = "Couldn't put the photo on the clipboard.";

/// The thread that keeps the clipboard: made at the first copy, and kept for the session.
fn keeper() -> std::sync::mpsc::Sender<Job> {
    let (jobs, queue) = std::sync::mpsc::channel::<Job>();
    let spawned = std::thread::Builder::new()
        .name("clipboard".to_owned())
        .spawn(move || {
            let mut clipboard = None;
            for job in queue {
                let result = (match &mut clipboard {
                    Some(clipboard) => Ok(clipboard),
                    None => arboard::Clipboard::new().map(|made| clipboard.insert(made)),
                })
                .and_then(|clipboard| {
                    clipboard.set_image(arboard::ImageData {
                        width: job.width,
                        height: job.height,
                        bytes: job.rgba.into(),
                    })
                })
                .map_err(|err| format!("Couldn't put the photo on the clipboard: {err}"));
                let _ = job.done.send(result);
            }
        });
    if let Err(err) = spawned {
        tracing::error!(%err, "the clipboard's thread could not start");
    }
    jobs
}

/// `path` shown in Explorer, selected: the shell's own call, as Tauri's opener makes it, on
/// a thread that has made COM ready. Where the shell cannot select it - Electron's note
/// says the call fails "mysteriously" with "file not found" on some systems, where Tauri's
/// opener opens the folder; here any failure does, the path having been found before -
/// its folder is opened instead.
#[cfg(target_os = "windows")]
fn reveal(path: &Path) -> Result<(), String> {
    use windows::{
        Win32::{
            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems},
        },
        core::HSTRING,
    };
    let folder = path.parent().unwrap_or(path);
    // SAFETY: called on this thread before any other COM call it makes; a second call on
    // a thread already ready answers so, which is no error here.
    let ready = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let (folder_name, path_name) = (HSTRING::from(folder), HSTRING::from(path));
    // SAFETY: both strings outlive the calls, and each list made is freed below.
    let (folder_item, path_item) = unsafe {
        (
            ILCreateFromPathW(&folder_name),
            ILCreateFromPathW(&path_name),
        )
    };
    let shown = if folder_item.is_null() || path_item.is_null() {
        Err(())
    } else {
        // SAFETY: both lists are the shell's own, alive until they are freed below.
        unsafe { SHOpenFolderAndSelectItems(folder_item, Some(&[path_item.cast_const()]), 0) }
            .map_err(|_| ())
    };
    // SAFETY: each is a list the shell made above, or null, which `ILFree` passes over.
    unsafe {
        ILFree(Some(folder_item.cast_const()));
        ILFree(Some(path_item.cast_const()));
    }
    let shown = shown.or_else(|()| {
        open::that_detached(folder).map_err(|err| format!("Couldn't show it in Explorer: {err}"))
    });
    if ready {
        // SAFETY: balances the `CoInitializeEx` above, on the same thread, after its last
        // COM call.
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
    shown
}

/// `path` shown in the Finder, selected: `open -R`, the Finder's own way to be asked.
#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> Result<(), String> {
    let shown = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status();
    match shown {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("Couldn't show it in the Finder ({status}).")),
        Err(err) => Err(format!("Couldn't show it in the Finder: {err}")),
    }
}

/// `path` shown in the file manager, selected: the file manager's own D-Bus interface,
/// which every desktop's file manager answers, as Tauri's opener asks it; where none
/// answers, its folder opened by whatever opens folders.
#[cfg(target_os = "linux")]
fn reveal(path: &Path) -> Result<(), String> {
    let shown = (|| {
        let uri = url::Url::from_file_path(path).map_err(|()| "no file path".to_owned())?;
        let connection = zbus::blocking::Connection::session().map_err(|err| err.to_string())?;
        connection
            .call_method(
                Some("org.freedesktop.FileManager1"),
                "/org/freedesktop/FileManager1",
                Some("org.freedesktop.FileManager1"),
                "ShowItems",
                &(vec![uri.as_str()], ""),
            )
            .map(|_| ())
            .map_err(|err| err.to_string())
    })();
    shown.or_else(|err| {
        tracing::warn!(%err, "no file manager answered: opening the folder");
        let folder = path.parent().unwrap_or(path);
        open::that_detached(folder).map_err(|err| format!("Couldn't show it: {err}"))
    })
}

/// How long the portal is given to accept `OpenFile` and open its picker.
#[cfg(target_os = "linux")]
const PORTAL_PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// The portal's file chooser, asked for a folder over D-Bus (`org.freedesktop.portal.FileChooser`).
///
/// Calls `OpenFile` directly via `zbus` rather than `ashpd::Request::send`: `ashpd` joins
/// `OpenFile` with waiting for the `Response` signal in one future, so timing out `send()`
/// timed out while the user was still browsing directories. Here `PORTAL_PATIENCE` bounds
/// only connecting and `OpenFile` returning its request handle; once the picker is open,
/// waiting for the user's `Response` signal has no timeout.
#[cfg(target_os = "linux")]
fn portal_folder(title: &str) -> Result<Picked, String> {
    use futures_lite::StreamExt;
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU64, Ordering},
    };
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
    let token = format!(
        "photon_{}_{}",
        std::process::id(),
        NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
    );

    pollster::block_on(async {
        let opened = futures_lite::future::or(
            async {
                let conn = zbus::Connection::session()
                    .await
                    .map_err(|err| format!("no session D-Bus: {err}"))?;
                let sender = conn
                    .unique_name()
                    .ok_or_else(|| "D-Bus connection has no unique name".to_owned())?
                    .trim_start_matches(':')
                    .replace('.', "_");
                let predicted = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
                let req_proxy = zbus::Proxy::new(
                    &conn,
                    "org.freedesktop.portal.Desktop",
                    predicted.as_str(),
                    "org.freedesktop.portal.Request",
                )
                .await
                .map_err(|err| format!("portal request proxy: {err}"))?;
                let mut responses = req_proxy
                    .receive_signal("Response")
                    .await
                    .map_err(|err| format!("portal Response subscription: {err}"))?;

                let chooser = zbus::Proxy::new(
                    &conn,
                    "org.freedesktop.portal.Desktop",
                    "/org/freedesktop/portal/desktop",
                    "org.freedesktop.portal.FileChooser",
                )
                .await
                .map_err(|err| format!("portal FileChooser proxy: {err}"))?;
                let mut options: HashMap<&str, Value<'_>> = HashMap::new();
                options.insert("handle_token", Value::from(token.as_str()));
                options.insert("directory", Value::from(true));
                options.insert("multiple", Value::from(false));
                options.insert("modal", Value::from(true));

                let actual: OwnedObjectPath = chooser
                    .call("OpenFile", &("", title, options))
                    .await
                    .map_err(|err| format!("portal OpenFile: {err}"))?;

                if actual.as_str() != predicted {
                    let actual_proxy = zbus::Proxy::new(
                        &conn,
                        "org.freedesktop.portal.Desktop",
                        actual.as_str(),
                        "org.freedesktop.portal.Request",
                    )
                    .await
                    .map_err(|err| format!("portal actual request proxy: {err}"))?;
                    responses = actual_proxy
                        .receive_signal("Response")
                        .await
                        .map_err(|err| format!("portal actual Response subscription: {err}"))?;
                }
                Ok::<_, String>(Some(responses))
            },
            async {
                async_io::Timer::after(PORTAL_PATIENCE).await;
                Ok::<_, String>(None)
            },
        )
        .await?;

        let Some(mut responses) = opened else {
            return Err("the desktop portal did not open in time".to_owned());
        };
        let signal = responses
            .next()
            .await
            .ok_or_else(|| "the desktop portal closed without answering".to_owned())?;
        let (code, results): (u32, HashMap<String, OwnedValue>) = signal
            .body()
            .deserialize()
            .map_err(|err| format!("malformed portal Response: {err}"))?;
        let uris: Vec<String> = results
            .get("uris")
            .and_then(|value| Vec::<String>::try_from(value.clone()).ok())
            .unwrap_or_default();
        portal_response(code, &uris)
    })
}

/// Decodes a `org.freedesktop.portal.Request::Response` code and its `uris` list.
#[cfg(target_os = "linux")]
fn portal_response(code: u32, uris: &[String]) -> Result<Picked, String> {
    match code {
        // A place that is no file of this computer's - a share the desktop opened as
        // `smb://` - goes into the field as it is, which says it is no whole path: chosen
        // and nothing said, it would read as a picker that did nothing.
        0 => Ok(uris.first().map_or(Picked::Cancelled, |raw| {
            let path = url::Url::parse(raw)
                .ok()
                .and_then(|uri| uri.to_file_path().ok())
                .unwrap_or_else(|| PathBuf::from(raw));
            Picked::Folder(path)
        })),
        1 => Ok(Picked::Cancelled),
        other => Err(format!("portal failed with response code {other}")),
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
        eframe::Error::Wgpu(err) => format!(
            "photon could not find a way to draw its window: no graphics adapter, not even a \
             software one, could be used.\n\n{err}"
        ),
        err => format!("photon could not start:\n\n{err}"),
    }
}

/// Says `text` in the system's own message box, for when photon has no window to say it
/// in - the library that could not be opened, a window that could not be made. On Linux
/// through `zenity`, where that is installed; where it is not, nothing is shown, which is
/// why the caller also writes it to the standard error and the log.
pub fn show_error(text: &str) {
    // On Windows the system's own box, from `user32`: rfd's is a task dialog when its
    // `common-controls-v6` feature is on (the Tauri shell's build turned it on), and a task
    // dialog is in Common Controls 6 alone, which a program without a manifest asking for
    // it is not given - it would not start.
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

    // A window no adapter can draw says so, in words that name what is missing.
    #[test]
    fn a_window_no_adapter_can_draw_is_said_so() {
        let err = eframe::Error::Wgpu(
            eframe::egui_wgpu::WgpuError::CustomNativeAdapterSelectionError("none".to_owned()),
        );
        assert!(startup_failed(&err).starts_with("photon could not find a way to draw its window"));
    }

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

    #[cfg(target_os = "linux")]
    #[test]
    fn portal_response_maps_file_uris_cancel_and_errors() {
        assert_eq!(
            portal_response(0, &["file:///tmp/my%20photos".to_owned()]),
            Ok(Picked::Folder(PathBuf::from("/tmp/my photos")))
        );
        assert_eq!(
            portal_response(0, &["smb://nas/photos".to_owned()]),
            Ok(Picked::Folder(PathBuf::from("smb://nas/photos")))
        );
        assert_eq!(portal_response(0, &[]), Ok(Picked::Cancelled));
        assert_eq!(portal_response(1, &[]), Ok(Picked::Cancelled));
        assert!(portal_response(2, &[]).is_err());
    }
}
