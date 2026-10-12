//! What the system says a notch of the mouse wheel scrolls. Windows alone has a setting
//! an application has to read for itself: a wheel reports notches there, and how many
//! lines a notch is worth is the user's to say (Settings, Mouse). `grid::scroll` makes
//! points of it.

/// Windows' lines a notch: `u32::MAX` for "one screen at a time", none where the setting
/// could not be read, or the system is not Windows.
#[cfg(target_os = "windows")]
pub fn lines() -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETWHEELSCROLLLINES, SystemParametersInfoW,
    };
    let mut lines: u32 = 0;
    // SAFETY: for `SPI_GETWHEELSCROLLLINES` the third argument is a pointer to one
    // `UINT`, which the call writes; `lines` is that, and outlives the call.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETWHEELSCROLLLINES,
            0,
            std::ptr::from_mut(&mut lines).cast(),
            0,
        )
    };
    (read != 0).then_some(lines)
}

#[cfg(not(target_os = "windows"))]
pub fn lines() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // The one call photon makes of the system for itself, so it is made here once on
    // every system the tests run on: it answers, and on Windows it answers with a
    // setting - which nothing here may assume the value of, the machine being whoever's.
    #[test]
    fn the_setting_is_read_where_there_is_one() {
        assert_eq!(lines().is_some(), cfg!(target_os = "windows"));
    }
}
