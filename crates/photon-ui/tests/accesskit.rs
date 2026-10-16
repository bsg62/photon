//! AccessKit, which a screen reader presses through: switched on (eframe's `accesskit`
//! feature), and kept from what lies under a dialog. egui's `Modal` keeps the pointer from
//! what is under it, and not a screen reader's press, which is an action on a widget by
//! its id; what the window answers under a dialog is dropped by the application, and that
//! is what keeps it out.

use crate::common::{library, opened};
use egui_kittest::kittest::Queryable;
use photon_core::grid::GridView;

// Settings open over the window: a screen reader's press on a sidebar row behind it -
// AccessKit's action (`click_accesskit`; `click` is the pointer's, which would be a press
// on the scrim and close Settings) - switches no view, and leaves Settings open.
#[test]
fn a_screen_readers_press_under_a_dialog_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(dir.path(), 12, 0, 0);
    let (mut harness, mut driver) = opened(&library, 12);
    driver.settle(&mut harness);
    harness.get_by_label("Settings").click();
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert!(harness.state().dialog().is_some());
    harness.get_by_label("Recent").click_accesskit();
    driver.act(&mut harness);
    driver.settle(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    assert_eq!(harness.state().settled().view, GridView::All);
    assert!(harness.state().dialog().is_some(), "Settings stays");
}
