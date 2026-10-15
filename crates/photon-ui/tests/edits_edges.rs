//! Turns and the crop at their edges: the review's tests of them (pull request #193) and
//! the fix pass's. Each says what it shows; the helpers are `tests/edits.rs`'s, copied,
//! since a test file's are its own.

use crate::common::{Driver, Harness, over};
use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use photon_engine::commands::{self, ItemEdit};
use photon_ui::app::App;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

fn pictures(dir: &Path) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    for (n, (name, width, height)) in [("a.jpg", 800, 400), ("b.jpg", 400, 800), ("c.jpg", 64, 48)]
        .into_iter()
        .enumerate()
    {
        let picture = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb([40 * n as u8, 120, 200]),
        ));
        let mut bytes = Vec::new();
        picture
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        let path = folder.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(start + Duration::from_secs(60 * n as u64))
            .unwrap();
    }
    folder
}

type Files = Vec<(PathBuf, Vec<u8>)>;

fn files(folder: &Path) -> Files {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .map(|path| (path.clone(), std::fs::read(&path).unwrap()))
        .collect();
    files.sort();
    files
}

fn three<'a>(dir: &Path) -> (Harness<'a>, Driver, Vec<i64>, Files) {
    let folder = pictures(dir);
    let made = files(&folder);
    let (mut harness, mut driver, _) = over(dir, Some(&folder));
    driver.until(&mut harness, "the three photos shown", |app| {
        app.photos() == 3 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    let ids = harness.state().shown();
    (harness, driver, ids, made)
}

fn the_photos_are_as_they_were(made: &[(PathBuf, Vec<u8>)]) {
    let folder = made[0].0.parent().unwrap();
    assert!(
        files(folder) == made,
        "a file of the watched folder changed"
    );
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn click(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    let at = (harness.state().tile_rect(offset))
        .expect("the photo is on screen")
        .center();
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(harness);
}

fn key(driver: &mut Driver, harness: &mut Harness<'_>, key: Key) {
    harness.key_press_modifiers(Modifiers::NONE, key);
    driver.act(harness);
}

fn open(driver: &mut Driver, harness: &mut Harness<'_>, offset: usize) {
    click(driver, harness, offset);
    click(driver, harness, offset);
    assert!(harness.state().viewer().is_some(), "the viewer is open");
    driver.until(harness, "the photo's facts and its picture", |app| {
        app.viewer_item().is_some() && app.viewer_levels().is_some()
    });
}

fn viewing(harness: &Harness<'_>) -> Option<i64> {
    harness.state().viewer().map(|viewer| viewer.id())
}

fn edit(app: &App) -> Option<(u8, Option<[u16; 4]>)> {
    let ItemEdit { turns, crop } = app.viewer_item()?.edit.as_ref()?;
    Some((*turns, *crop))
}

fn rest(driver: &mut Driver, harness: &mut Harness<'_>, secs: f64) {
    driver.settle(harness);
    driver.frames_in(harness, secs);
    driver.settle(harness);
    driver.frames_in(harness, secs);
    driver.settle(harness);
}

fn cropped(driver: &mut Driver, harness: &mut Harness<'_>, id: i64, crop: [u16; 4]) {
    let engine = harness.state().engine().clone();
    let was = harness.state().version();
    commands::set_item_edit(&engine, id, 0, Some(crop)).unwrap();
    driver.until(harness, "the grid with the photo cropped", |app| {
        app.version() != was
    });
    driver.settle(harness);
}

const LEFT_HALF: [u16; 4] = [0, 0, 32768, 65535];

// One picture is made after an edit made in the viewer. The engine's rebuild puts a grid
// on screen whose key for the photo is the new one, `Viewer::follow` takes it, and the
// picture of the turned photo is asked for at once; a round trip later the library says
// what the photo is, and `took` finds it another picture. Asked again only when it is
// another under the key it *had* - turned and turned back - and not under the key the
// viewer has, which `follow` had already moved: so it made every turn, "Original" and
// "Apply" twice, the photo blank for two whole renders.
#[test]
fn a_turn_in_the_viewer_has_its_picture_made_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, made) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    key(&mut driver, &mut harness, Key::R);
    driver.until(&mut harness, "the turn known and its picture", |app| {
        edit(app) == Some((1, None)) && app.viewer_levels().is_some_and(|w| w[0] == 400)
    });
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(
        harness.state().pictures_asked(),
        asked + 1,
        "one picture is wanted after the turn"
    );
    assert_eq!(viewing(&harness), Some(ids[0]));
    the_photos_are_as_they_were(&made);
}

// The same, for the crop tool's "Apply".
#[test]
fn a_crop_applied_has_its_picture_made_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, made) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    harness.event(egui::Event::PointerMoved(egui::pos2(995.0, 350.0)));
    harness.event(button(
        egui::pos2(995.0, 350.0),
        PointerButton::Primary,
        true,
    ));
    harness.event(egui::Event::PointerMoved(egui::pos2(745.0, 350.0)));
    harness.event(button(
        egui::pos2(745.0, 350.0),
        PointerButton::Primary,
        false,
    ));
    driver.act(&mut harness);
    key(&mut driver, &mut harness, Key::Enter);
    driver.until(&mut harness, "the crop stored and its picture", |app| {
        app.cropping().is_none()
            && edit(app).is_some()
            && app.viewer_levels().is_some_and(|w| w[0] == 600)
    });
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(harness.state().pictures_asked(), asked + 1);
    the_photos_are_as_they_were(&made);
}

// A still window with the tool open and resting draws nothing: on a photo with a crop,
// once the picture without it is made, and with a drag held still.
#[test]
fn a_still_window_under_the_tool_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(driver.frames_in(&mut harness, 3.0), 0, "resting");
    // A drag held still on the right edge's handle.
    let at = egui::pos2(495.0, 350.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(egui::Event::PointerMoved(at + egui::vec2(-40.0, 0.0)));
    driver.act(&mut harness);
    assert!(harness.state().cropping().unwrap().dragging().is_some());
    rest(&mut driver, &mut harness, 1.0);
    assert_eq!(driver.frames_in(&mut harness, 3.0), 0, "a drag held still");
}

// The window resized under the open tool: the picture without the crop is made again for
// the new box once the size has stood, and the photo's own is not asked for meanwhile.
#[test]
fn the_tools_picture_follows_a_window_resized_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    harness.set_size(egui::vec2(900.0, 600.0));
    driver.act(&mut harness);
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(harness.state().pictures_asked(), asked + 1);
    assert!(harness.state().crop_picture().is_some());
    assert!(harness.state().cropping().is_some());
}

// A ratio chosen on the least rectangle a drag makes, on a panorama four times as wide as
// tall, held to 1:1, is a rectangle the library takes: a side shrunk by the ratio is grown
// back to `MIN_SIDE` (in `crop.ts` it is not, and "Apply" was refused).
#[test]
fn a_ratio_chosen_on_the_least_rectangle_is_one_the_library_takes() {
    use photon_ui::crop::Handle;
    use photon_ui::crop_tool::CropTool;
    let mut tool = CropTool::begin(None, 4000, 1000);
    tool.start_drag(Handle::Se, 0.0, 0.0);
    tool.drag_to(-9000.0, -9000.0, 1000.0, 250.0);
    tool.end_drag();
    let square = (photon_ui::crop::ASPECTS.iter())
        .position(|(label, _)| *label == "1:1")
        .unwrap();
    tool.set_aspect(square);
    let [left, top, right, bottom] = tool.wire().expect("a crop");
    let crop = photon_core::edit::Crop {
        left,
        top,
        right,
        bottom,
    };
    assert!(
        photon_core::edit::Edit::new(0, Some(crop)).is_some(),
        "a rectangle the tool allows: {:?} is {} units wide",
        tool.rect(),
        right - left
    );
}

// a turn made from elsewhere while the
// viewer is open has the picture made twice too - the rule is older than this pull
// request, which makes it every `R`, "Original" and "Apply".
#[test]
fn a_turn_from_elsewhere_has_its_picture_made_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    driver.until(&mut harness, "the turn known", |app| {
        edit(app).is_some_and(|(turns, _)| turns == 1)
    });
    driver.until(&mut harness, "the photo's own picture, turned", |app| {
        app.viewer_levels().is_some()
    });
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(harness.state().pictures_asked(), asked + 1);
}

// A double click on the bar's crop button: its first click opens the tool, and
// its second lands on the tool's bar, which stands where the viewer's did - here, in a
// window at its least width (800) on a photo that has a crop, on "Whole photo", which
// throws away the crop the tool opened on. The rule the window keeps elsewhere for a
// second click on what an overlay has just put in a place (`overlay_closed`,
// `DialogView::deaf`) is not kept here. (A double click on "Cancel" lands its second
// click on the viewer's bar the same way: at 800 points, on the info panel's button.)
#[test]
fn a_double_click_on_the_crop_button_does_not_press_the_tools_bar() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    harness.set_size(egui::vec2(800.0, 700.0));
    driver.act(&mut harness);
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    driver.act(&mut harness);
    let crop = harness.get_by_label("Crop").rect();
    let at = egui::pos2(crop.right() - 3.0, crop.center().y);
    for _ in 0..2 {
        harness.event(egui::Event::PointerMoved(at));
        harness.event(button(at, PointerButton::Primary, true));
        harness.event(button(at, PointerButton::Primary, false));
        driver.act(&mut harness);
    }
    let tool = *harness.state().cropping().expect("the tool is open");
    assert_eq!(tool.wire(), Some(LEFT_HALF), "the crop the photo has");
}

/// The library's write lock, taken from outside (`tests/edits.rs`'s `locked`).
fn locked(harness: &Harness<'_>) -> rusqlite::Connection {
    let path = harness.state().engine().lib.path().to_owned();
    let other = rusqlite::Connection::open(path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    other
}

fn stored(harness: &Harness<'_>, id: i64) -> photon_core::edit::Edit {
    (harness.state().engine().lib.item(id).unwrap())
        .expect("the photo")
        .edit
}

/// The texture the last frame drew largest, by the name the application gave it, and
/// where.
fn drawn_texture(harness: &Harness<'_>) -> Option<(String, egui::Rect)> {
    fn each(shape: &egui::Shape, found: &mut Vec<(egui::TextureId, egui::Rect)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each(shape, found)),
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                found.push((mesh.texture_id, mesh.calc_bounds()));
            }
            egui::Shape::Rect(rect) if rect.brush.is_some() => {
                let brush = rect.brush.as_ref().unwrap();
                found.push((brush.fill_texture_id, rect.rect));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &harness.output().shapes {
        each(&clipped.shape, &mut found);
    }
    let (id, at) = found
        .into_iter()
        .max_by(|a, b| a.1.area().total_cmp(&b.1.area()))?;
    let manager = harness.ctx.tex_manager();
    let name = manager.read().meta(id)?.name.clone();
    Some((name, at))
}

fn drag_on(driver: &mut Driver, harness: &mut Harness<'_>, from: Pos2, to: Pos2) {
    harness.event(egui::Event::PointerMoved(from));
    harness.event(button(from, PointerButton::Primary, true));
    harness.event(egui::Event::PointerMoved(to));
    harness.event(button(to, PointerButton::Primary, false));
    driver.act(harness);
}

fn click_at(driver: &mut Driver, harness: &mut Harness<'_>, at: Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(button(at, PointerButton::Primary, true));
    harness.event(button(at, PointerButton::Primary, false));
    driver.act(harness);
}

// A photo the library says cannot be shown - its thumbnail could not be made - whose full
// picture can be made all the same is not turned by `R`.
#[test]
fn a_photo_that_cannot_be_shown_is_not_turned() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, made) = three(dir.path());
    let engine = harness.state().engine().clone();
    (engine.lib)
        .set_thumb_state(
            ids[1],
            photon_core::media::ThumbState::Failed,
            Some("It is broken."),
        )
        .unwrap();
    click(&mut driver, &mut harness, 1);
    click(&mut driver, &mut harness, 1);
    driver.until(&mut harness, "its facts", |app| app.viewer_item().is_some());
    rest(&mut driver, &mut harness, 0.5);
    key(&mut driver, &mut harness, Key::R);
    key(&mut driver, &mut harness, Key::C);
    assert!(harness.state().cropping().is_none());
    rest(&mut driver, &mut harness, 0.5);
    assert!(stored(&harness, ids[1]).is_identity());
    the_photos_are_as_they_were(&made);
}

// The list of ratios lies over the picture; a press on it that moves - a hand that slides
// on its way to an option - does not move the rectangle under the list (the one judge, in
// the crop view's `free`).
#[test]
fn a_press_on_the_open_list_does_not_move_the_rectangle_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    // (Past the second press of a double click that opened it.)
    driver.frames_in(&mut harness, 0.5);
    // A rectangle that can move: its right edge a quarter in.
    drag_on(
        &mut driver,
        &mut harness,
        egui::pos2(995.0, 350.0),
        egui::pos2(745.0, 350.0),
    );
    harness.get_by_label("Crop ratio: Free").click();
    for _ in 0..30 {
        driver.act(&mut harness);
    }
    let free = harness.get_by_label("Free").rect().center();
    assert!(
        free.y > 100.0 && free.y < 600.0 && free.x < 745.0,
        "over the rectangle: {free:?}"
    );
    let before = harness.state().cropping().unwrap().rect();
    drag_on(
        &mut driver,
        &mut harness,
        free,
        free + egui::vec2(40.0, 0.0),
    );
    rest(&mut driver, &mut harness, 0.3);
    assert_eq!(harness.state().cropping().unwrap().rect(), before);
}

// A list left open by a tool that closed under it - the photo turned from elsewhere - is
// not open in the next tool.
#[test]
fn a_list_left_open_by_a_closed_tool_is_not_the_next_tools() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    // (Past the second press of a double click that opened it.)
    driver.frames_in(&mut harness, 0.5);
    harness.get_by_label("Crop ratio: Free").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label("1:1").is_some(), "the list is open");
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    driver.until(&mut harness, "the tool closed", |app| {
        app.cropping().is_none() && edit(app) == Some((1, None))
    });
    rest(&mut driver, &mut harness, 0.3);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.state().cropping().is_some());
    assert!(harness.query_by_label("1:1").is_none(), "the list is shut");
}

// While the tool's write is on its way no ratio is chosen: it would reshape the rectangle
// on screen while the one being stored is the one before, and the tool would close on a
// crop that is not what it showed.
#[test]
fn no_ratio_is_chosen_while_the_rectangle_is_being_stored() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, _, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    let lock = locked(&harness);
    key(&mut driver, &mut harness, Key::Enter);
    driver.act(&mut harness);
    let before = harness.state().cropping().unwrap().rect();
    harness.get_by_label("Crop ratio: Free").click();
    driver.act(&mut harness);
    for _ in 0..30 {
        driver.act(&mut harness);
    }
    if let Some(square) = harness.query_by_label("1:1") {
        let at = square.rect().center();
        click_at(&mut driver, &mut harness, at);
    }
    let after = harness.state().cropping().unwrap().rect();
    lock.execute_batch("COMMIT").unwrap();
    drop(lock);
    assert_eq!(after, before);
}

// A press on the tool's bar a few points beside "Apply" applies nothing: egui gives a near
// miss to a button where nothing is hit, which the photo's surface under the tool is
// there to prevent.
#[test]
fn a_press_beside_apply_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    drag_on(
        &mut driver,
        &mut harness,
        egui::pos2(995.0, 350.0),
        egui::pos2(745.0, 350.0),
    );
    let apply = harness.get_by_label("Apply").rect();
    click_at(
        &mut driver,
        &mut harness,
        egui::pos2(apply.right() + 3.0, apply.center().y),
    );
    rest(&mut driver, &mut harness, 0.5);
    assert!(harness.state().cropping().is_some());
    assert!(stored(&harness, ids[0]).is_identity());
}

// Once the picture without the crop is made, it is what is drawn under the tool - not
// the photo's own, which is of the part the crop keeps and would be stretched over the
// whole box.
#[test]
fn the_tools_picture_is_drawn_once_it_is_made() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    driver.act(&mut harness);
    let (name, _) = drawn_texture(&harness).expect("a picture");
    assert!(name.starts_with("uncropped-"), "{name}");
}

/// A folder with one large photo, larger than the window: a copy fitted to the window is
/// made of it.
fn large(dir: &Path) -> PathBuf {
    let folder = dir.join("Pictures");
    std::fs::create_dir_all(&folder).unwrap();
    let picture = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        2400,
        1200,
        image::Rgb([90, 120, 200]),
    ));
    picture.save(folder.join("big.jpg")).unwrap();
    folder
}

// A photo larger than the window, with no crop: under the tool its picture is the one
// fitted to the window, drawn pixel for pixel, as it is in the viewer.
#[test]
fn under_the_tool_the_copy_fitted_to_the_window_is_drawn() {
    let dir = tempfile::tempdir().unwrap();
    let folder = large(dir.path());
    let (mut harness, mut driver, _) = over(dir.path(), Some(&folder));
    driver.until(&mut harness, "the photo shown", |app| {
        app.photos() == 1 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    harness.input_mut().max_texture_side = Some(16_384);
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(&mut harness, "its fitted copy", |app| {
        app.viewer_item().is_some() && app.viewer_fitted().is_some()
    });
    driver.act(&mut harness);
    let (name, _) = drawn_texture(&harness).expect("a picture");
    assert!(
        name.ends_with("-fitted"),
        "the viewer draws its copy: {name}"
    );
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    rest(&mut driver, &mut harness, 0.5);
    let (name, _) = drawn_texture(&harness).expect("a picture");
    assert!(name.ends_with("-fitted"), "and so does the tool: {name}");
}

// The bar keeps its width while the caption is not known (`bar_place`'s `was`),
// so that its buttons do not jump at a step - but "Original" is drawn only while the
// library has said the photo has an edit, and at a step it has said nothing yet. From one
// edited photo to the next, the info panel's button jumps left by "Original"'s width for
// the round trip and back: the jump `was` was made to end, back for every step through
// a series of edited photos. The library's answer is held back here by renaming its table
// from a second connection, the seam `tests/faces_edges.rs` uses.
#[test]
fn the_bars_buttons_stand_still_at_a_step_between_edited_photos() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    cropped(&mut driver, &mut harness, ids[1], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    driver.act(&mut harness);
    let before = harness.get_by_label("Photo information").rect();
    let path = harness.state().engine().lib.path().to_owned();
    let other = rusqlite::Connection::open(path).unwrap();
    other
        .busy_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    other
        .execute_batch("ALTER TABLE items RENAME TO items_away")
        .unwrap();
    key(&mut driver, &mut harness, Key::ArrowRight);
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert_eq!(viewing(&harness), Some(ids[1]));
    assert!(harness.state().viewer_item().is_none(), "nothing said yet");
    let during = harness.get_by_label("Photo information").rect();
    other
        .execute_batch("ALTER TABLE items_away RENAME TO items")
        .unwrap();
    assert_eq!(during, before, "the panel's button stands where it stood");
}

// The crop tool open on a photo that has a crop, its picture without the crop made; the
// photo turned from elsewhere. The grid that says so is on screen a round trip before
// what the library says, and the tool, open still in the frames between, asked for a
// picture without the crop of the new state, which nobody draws: the library's answer
// closed the tool. It closes with that grid now.
#[test]
fn a_picture_change_under_the_tool_asks_for_one_picture_not_two() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, made) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    rest(&mut driver, &mut harness, 0.5);
    let asked = harness.state().pictures_asked();
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    driver.until(&mut harness, "the turn known, the tool closed", |app| {
        edit(app).is_some_and(|(turns, _)| turns == 1) && app.cropping().is_none()
    });
    driver.until(&mut harness, "the photo's own picture, turned", |app| {
        app.viewer_levels().is_some()
    });
    rest(&mut driver, &mut harness, 0.5);
    assert_eq!(
        harness.state().pictures_asked(),
        asked + 1,
        "one picture is wanted after the turn: the photo's own"
    );
    the_photos_are_as_they_were(&made);
}

// A video is not turned. Its bar has no tools for it, but `R` is a key, and before the
// picture worker has refused the video - it is still busy with the photo before, here
// held at its render - nothing else says it cannot be: the engine would be asked, and
// refuse, and the refusal said.
#[test]
fn a_video_is_not_turned_by_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(19_000 * 86_400);
    let video = folder.join("b.mp4");
    std::fs::write(&video, b"not a video").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&video)
        .unwrap()
        .set_modified(start + Duration::from_secs(30))
        .unwrap();
    let (mut harness, mut driver, _) = over(dir.path(), Some(&folder));
    driver.until(&mut harness, "the four shown", |app| {
        app.photos() == 4 && app.last_frame().is_some_and(|frame| frame.settled)
    });
    driver.settle(&mut harness);
    let ids = harness.state().shown();
    let rendering = photon_engine::engine::RENDERING.lock();
    click(&mut driver, &mut harness, 0);
    click(&mut driver, &mut harness, 0);
    driver.until(
        &mut harness,
        "its facts, and its picture asked for",
        |app| app.viewer_item().is_some() && app.viewer_asked(),
    );
    key(&mut driver, &mut harness, Key::ArrowRight);
    assert_eq!(viewing(&harness), Some(ids[1]));
    driver.until(&mut harness, "the video's facts", |app| {
        app.viewer_item()
            .is_some_and(|item| item.file_name == "b.mp4")
    });
    key(&mut driver, &mut harness, Key::R);
    drop(rendering);
    rest(&mut driver, &mut harness, 0.5);
    assert!(stored(&harness, ids[1]).is_identity());
    assert!(
        harness.state().toasts().is_empty(),
        "nothing asked, nothing refused"
    );
}

// The picture without the crop is refused after its tool has closed: nothing is said of
// it, there being no tool for it to be missing from.
#[test]
fn a_refusal_for_a_tool_that_has_closed_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, made) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    rest(&mut driver, &mut harness, 0.5);
    std::fs::remove_file(&made[0].0).unwrap();
    let rendering = photon_engine::engine::RENDERING.lock();
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    assert!(harness.state().cropping().is_some());
    key(&mut driver, &mut harness, Key::Escape);
    drop(rendering);
    for _ in 0..5 {
        driver.act(&mut harness);
    }
    driver.quiet();
    driver.act(&mut harness);
    assert!(harness.state().toasts().is_empty());
}

// The tool closes in the frame the grid says the photo is another picture, a round trip
// before the library does: until then it would draw its picture of the photo as it was
// under the photo as it is.
#[test]
fn the_tool_closes_with_the_grid_that_says_the_photo_is_another_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    cropped(&mut driver, &mut harness, ids[0], LEFT_HALF);
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.until(&mut harness, "the picture without its crop", |app| {
        app.crop_picture().is_some()
    });
    let was = harness.state().viewer().unwrap().key();
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    driver.until(&mut harness, "the grid with the photo turned", |app| {
        app.viewer().is_some_and(|viewer| viewer.key() != was)
    });
    assert!(harness.state().cropping().is_none());
}

/// `?` pressed, in one frame with what it types, and let go of.
fn question_mark(driver: &mut Driver, harness: &mut Harness<'_>) {
    harness.input_mut().events.extend([
        egui::Event::Key {
            key: Key::Questionmark,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        },
        egui::Event::Text("?".to_owned()),
    ]);
    driver.act(harness);
    // Let go of: held, the next `?` would be its repeat.
    harness.event(egui::Event::Key {
        key: Key::Questionmark,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    });
    driver.act(harness);
}

fn sheet(app: &App) -> bool {
    matches!(app.dialog(), Some(photon_ui::dialogs::Dialog::Shortcuts))
}

// An open ratio list has the keys, `?` among them: the list of shortcuts does not open
// over it. A list left open by a tool that closed under it has no keys any more, and the
// viewer's `?` is the list's again.
#[test]
fn no_list_of_shortcuts_opens_over_an_open_ratio_list() {
    let dir = tempfile::tempdir().unwrap();
    let (mut harness, mut driver, ids, _) = three(dir.path());
    open(&mut driver, &mut harness, 0);
    key(&mut driver, &mut harness, Key::C);
    driver.act(&mut harness);
    driver.frames_in(&mut harness, 0.5);
    harness.get_by_label("Crop ratio: Free").click();
    driver.act(&mut harness);
    driver.act(&mut harness);
    assert!(harness.query_by_label("1:1").is_some(), "the list is open");
    question_mark(&mut driver, &mut harness);
    assert!(
        !sheet(harness.state()),
        "the list of shortcuts opened over the ratio's"
    );
    assert!(harness.query_by_label("1:1").is_some(), "the list is open");
    let engine = harness.state().engine().clone();
    commands::rotate_item(&engine, ids[0], true).unwrap();
    driver.until(&mut harness, "the tool closed", |app| {
        app.cropping().is_none() && edit(app) == Some((1, None))
    });
    driver.act(&mut harness);
    question_mark(&mut driver, &mut harness);
    assert!(sheet(harness.state()), "`?` in the viewer opened nothing");
}
