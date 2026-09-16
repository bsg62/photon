//! Works around WebKitGTK's DMABUF renderer on NVIDIA's proprietary driver.
//!
//! On that driver the renderer breaks the webview outright: under Wayland the process dies
//! within milliseconds of the window appearing, with only `Gdk-Message: Error 71 (Protocol
//! error) dispatching to Wayland display` on stderr. `WEBKIT_DISABLE_DMABUF_RENDERER=1`
//! makes WebKit fall back to its older compositing path, which works everywhere but is
//! slower, so it is applied only where the driver is loaded rather than for every Linux user.

use std::{env, path::Path};

const VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// Sets the workaround variable when NVIDIA's kernel module is loaded and the user has not
/// set it themselves, and reports whether it did.
///
/// Must run before any other thread exists: `set_var` races with any concurrent read of the
/// environment, and WebKit reads this one when the webview is created. `app::run` calls it
/// on its first line for that reason.
pub fn apply_dmabuf_workaround() -> bool {
    // `/sys/module/nvidia` exists while the proprietary kernel module is loaded, which also
    // covers hybrid laptops rendering on the integrated GPU; the fallback costs them some
    // speed, not correctness. nouveau is a different module and is left alone: the crash was
    // seen on the proprietary driver only.
    let apply = needs_workaround(
        env::var_os(VAR).is_some(),
        Path::new("/sys/module/nvidia").exists(),
    );
    if apply {
        // SAFETY: called from `app::run` before the tracing subscriber, Tauri or any other
        // code that could spawn a thread; the process is still single-threaded.
        unsafe { env::set_var(VAR, "1") };
    }
    apply
}

/// Any value the user set is kept, including `0`, which WebKit reads as "leave the renderer
/// on" — that is the opt-out for a driver release that fixes the bug.
fn needs_workaround(already_set: bool, nvidia_loaded: bool) -> bool {
    nvidia_loaded && !already_set
}

#[cfg(test)]
mod tests {
    use super::needs_workaround;

    #[test]
    fn the_workaround_applies_only_on_nvidia() {
        assert!(needs_workaround(false, true));
        assert!(!needs_workaround(false, false));
    }

    #[test]
    fn a_value_the_user_set_is_never_overridden() {
        assert!(!needs_workaround(true, true));
        assert!(!needs_workaround(true, false));
    }
}
