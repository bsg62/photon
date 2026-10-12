//! The viewer, drawn: a black ground in place of the shell, the photo fitted on it, and
//! the controls over it. What the user did is answered as `ViewerAction`s, for the
//! application to act on.
//!
//! **The ground is black in both themes** - a photo is judged against black, not against
//! a theme - and everything on it is in the dark palette, on the glass the Svelte viewer
//! has (`--glass`, a dark colour nine tenths opaque: a web view blurs what is behind it,
//! which nothing here does, and the opacity is what was chosen to read without the blur).
//!
//! **No control here is a stop of the Tab key.** Every one has a key - Escape, the
//! arrows, the period - and a control that had the keyboard would take the arrows from
//! the viewer: egui walks the keyboard on from a focused widget with them. (What the
//! application draws over the viewer can be one, a message's button: it calls Tab off
//! while the viewer is open.)

use crate::{
    icons::Icon,
    text,
    theme::{
        apply::{color, menu_shadow},
        fonts,
        tokens::{DARK, GLASS, GLASS_LINE, R, S, T},
    },
    viewer::wheel_step,
    viewer_info_view::{InfoAction, InfoData, InfoView},
    viewer_layout::{BAR, CLOSE, EDGE, Frame, NAV, contained, info_place, photo_area},
    viewer_levels::level_for,
    viewer_zoom::{
        MAX_ZOOM, MIN_ZOOM, Pan, ZOOM_KEY_STEP, Zoom, actual_size_zoom, wheel_zoom_factor,
    },
};
use eframe::egui::{
    self, Color32, CursorIcon, Event, Id, Key, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, WidgetInfo, WidgetType, pos2, vec2,
};

/// A tool in the bar, how many there are - the star, the info panel - and the room
/// between two.
const TOOL: f32 = 30.0;
const TOOLS: usize = 2;
const TOOL_GAP: f32 = 2.0;
/// The room the zoom's slider takes at the bar's right, and the same kept on its left so
/// that the bar stays in the middle: the bar is no wider than what is left between.
const BESIDE_BAR: f32 = 214.0;
/// How far the message keeps from the window's sides: clear of the two buttons there.
const MESSAGE_SIDE: f32 = 64.0;
/// The photo's own caption, over the bar.
const CAPTION_BOTTOM: f32 = 64.0;
/// The zoom's slider: how long its track is, and the room its percentage has.
const SLIDER: f32 = 120.0;
const SLIDER_LEVEL: f32 = 38.0;
/// What the slider moves by, as the web view's range input did.
const SLIDER_STEP: f32 = 0.05;
/// How many pixels of a turn of the wheel one line of it is, to the web view whose
/// arithmetic the zoom's is (`wheel_zoom_factor`): a notch is a hundred.
const PIXELS_A_LINE: f32 = 100.0;

/// What the user did in a frame of the viewer.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewerAction {
    Close,
    /// One photo on (1) or back (-1).
    Step(i64),
    First,
    Last,
    Star,
    Hide,
    /// The info panel, opened or closed.
    Info,
    /// Something pressed in the info panel.
    Panel(InfoAction),
}

/// One size of the full picture.
#[derive(Clone, Copy)]
pub struct Level<'a> {
    pub texture: &'a TextureHandle,
    /// Its width in pixels.
    pub width: u32,
}

/// What is said in the photo's place, or over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    pub text: &'a str,
    /// Over the picture, which stays: a video's poster. Otherwise in the picture's
    /// place, since there is none to show.
    pub over: bool,
}

/// What the viewer draws this frame.
pub struct ViewerData<'a> {
    /// The full picture's levels, largest first; none until it is made.
    pub levels: &'a [Level<'a>],
    /// The full picture scaled, with a proper filter, to the size it had when it was
    /// last fitted into the window: drawn while it is fitted at that size still.
    pub fitted: Option<&'a TextureHandle>,
    /// The photo's preview, drawn until then.
    pub preview: Option<&'a TextureHandle>,
    pub message: Option<Message<'a>>,
    /// The line in the bar: the file's name, its time, its size, its place.
    pub caption: &'a str,
    /// The photo's own caption, where it has one.
    pub photo_caption: Option<&'a str>,
    /// The photo's width in pixels as it is seen, as the library says it; nought while
    /// it has not. What a double click measures "its own pixels" by.
    pub width: u32,
    /// A video: its poster is neither zoomed nor panned, and has no slider.
    pub video: bool,
    /// Whether the photo is starred; none while that is not known, or for a photo that
    /// cannot be - the button is then there and takes no press.
    pub starred: Option<bool>,
    pub can_prev: bool,
    pub can_next: bool,
    /// Whether the info panel is open: the photo is then fitted beside it, and its
    /// button in the bar is shown pressed.
    pub info_open: bool,
    /// What the panel says, once the library has said it of this photo.
    pub info: Option<InfoData<'a>>,
}

/// A press of the first button on the photo, from the button going down until it comes
/// up - or is taken away.
#[derive(Clone, Copy, Debug)]
struct Press {
    /// Where the button went down.
    at: Pos2,
    /// The pan as it was then, moved along with every zoom made since: the drag is
    /// measured from here. None once the photo has been fitted under the press, which
    /// is then no drag and does not become one again: zoomed under a button that was
    /// down at fit and has moved since, the photo would be thrown by all the pointer
    /// had moved while there was nothing to move.
    pan: Option<Pan>,
    /// Whether the pointer has gone further from `at` than a click may: it is a drag,
    /// and no click when the button comes up.
    moved: bool,
}

#[derive(Default)]
pub struct ViewerView {
    /// What the wheel has turned towards the next photo and not yet reached.
    wheel: f32,
    /// The press on the photo that is held.
    press: Option<Press>,
    /// The last click on the photo, when and where: the first of a double click.
    clicked: Option<(f64, Pos2)>,
    /// How wide the bar was in the last frame, for a frame with no caption to size it by.
    bar: Option<f32>,
    /// The info panel: how far it is scrolled, and what is typed into its field.
    info: InfoView,
    /// The window was left, or a touch cancelled, since the first button last went
    /// down: the slider follows the pointer no further, as a pan does not. A button let
    /// go of over another window may never be heard of.
    let_go: bool,
}

/// Where the controls stand: what a press on the photo is not.
struct Controls {
    prev: Rect,
    next: Rect,
    close: Rect,
    bar: Rect,
    /// None for a video, which has none.
    slider: Option<Rect>,
    /// The photo's own caption, where it has one: no control, but a press on it is not
    /// the photo's either - a double click on a line of text is not "closer".
    caption: Option<Rect>,
    /// The info panel, while it is open.
    panel: Option<Rect>,
}

impl Controls {
    fn hold(&self, at: Pos2) -> bool {
        [self.prev, self.next, self.close, self.bar]
            .into_iter()
            .chain(self.slider)
            .chain(self.caption)
            .chain(self.panel)
            .any(|rect| rect.contains(at))
    }
}

impl ViewerView {
    /// Draws the viewer over the whole window and answers what was done in it. The zoom
    /// and the pan are `zoom`'s, which the keys, the wheel, a pinch, a double click, a
    /// drag and the slider change here.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &ViewerData<'_>,
        zoom: &mut Zoom,
    ) -> Vec<ViewerAction> {
        let window = ui.ctx().content_rect();
        let painter = ui.painter().clone();
        // The one colour in photon that no token holds: the ground a photo is judged
        // against is not a theme's to choose.
        painter.rect_filled(window, 0.0, Color32::BLACK);

        // The photo's area: the window, less the info panel's room while it is open.
        // The fit, the zoom's bounds, the point a zoom holds still and the pan are all
        // measured in it, and the picture is cut off at its edge.
        let whole = Frame {
            left: window.left(),
            top: window.top(),
            width: window.width(),
            height: window.height(),
        };
        let area = photo_area(whole, data.info_open);
        let panel = data.info_open.then(|| {
            let place = info_place(whole);
            Rect::from_min_size(pos2(place.left, place.top), vec2(place.width, place.height))
        });
        let within = Rect::from_min_size(pos2(area.left, area.top), vec2(area.width, area.height));
        let shows = !data.message.is_some_and(|message| !message.over);
        // Nothing is zoomed where there is no picture, and a video's poster is not.
        let zooms = shows && !data.video;
        // The area may be another size than it was: a pan that was within bounds in the
        // larger one is pulled back in.
        zoom.held_to((area.width, area.height));

        // Where the controls stand, before anything is read: a press on one is its own.
        // (The photo's own caption is not drawn beside the open panel, which has it
        // whole.)
        let caption = (data.photo_caption)
            .filter(|_| !data.info_open)
            .map(|line| caption_place(ui, window, line));
        let side = |left: f32| {
            Rect::from_min_size(pos2(left, window.center().y - NAV / 2.0), vec2(NAV, NAV))
        };
        let controls = Controls {
            prev: side(window.left() + EDGE),
            next: side(within.right() - EDGE - NAV),
            close: Rect::from_min_size(
                pos2(window.right() - EDGE - CLOSE, window.top() + EDGE),
                vec2(CLOSE, CLOSE),
            ),
            bar: bar_place(ui, window, data.caption, self.bar),
            slider: (!data.video).then(|| slider_place(window)),
            caption: caption.as_ref().map(|(place, _)| *place),
            panel,
        };
        self.bar = Some(controls.bar.width());
        // The photo's own surface, under the controls: with something there to be hit,
        // egui hands no press that lands a few points beside a button to that button,
        // and what a press was is decided in one place, by where it was.
        ui.interact(window, Id::new("viewer-photo"), Sense::CLICK);
        let mut actions = self.input(ui, data, zoom, area, &controls, zooms);

        if shows && let Some((texture, at)) = picture(ui, data, area, zoom) {
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            (painter.with_clip_rect(within)).image(texture.id(), at, whole, Color32::WHITE);
        }
        if let Some(message) = data.message {
            said(ui, within, message);
        }

        // The one before and the one after, at the window's sides.
        if round(
            ui,
            controls.prev,
            "viewer-prev",
            Icon::ChevronLeft,
            "Previous photo",
            data.can_prev,
        ) {
            actions.push(ViewerAction::Step(-1));
        }
        if round(
            ui,
            controls.next,
            "viewer-next",
            Icon::ChevronRight,
            "Next photo",
            data.can_next,
        ) {
            actions.push(ViewerAction::Step(1));
        }
        if round(
            ui,
            controls.close,
            "viewer-close",
            Icon::X,
            "Close viewer",
            true,
        ) {
            actions.push(ViewerAction::Close);
        }

        if let (Some(line), Some((place, galley))) = (data.photo_caption, caption) {
            photo_caption(ui, place, galley, line);
        }
        actions.extend(bar(ui, controls.bar, data));
        if let Some(place) = controls.slider {
            slider(
                ui,
                place,
                zoom,
                (area.width, area.height),
                zooms,
                !self.let_go,
            );
        }
        if let Some(place) = panel {
            let pressed = self.info.show(ui, place, data.info.as_ref());
            actions.extend(pressed.into_iter().map(ViewerAction::Panel));
        }
        actions
    }

    /// The frame's keys, its wheel, a pinch and the mouse's buttons, in the order they
    /// came.
    ///
    /// A key held down repeats, and a repeat steps or zooms again - but not the ones
    /// that close or write: Escape held from whatever it has just closed must not close
    /// the viewer with it, and each press of `.` or `H` is a write. A chord with Ctrl,
    /// Cmd or Alt is not the viewer's.
    ///
    /// The zoom changed here is the photo's that the frame began on. A step is acted on
    /// after the frame, and puts the next photo back to the fit: a zoom's key pressed
    /// *before* an arrow in one frame is the photo's that is left, as it should be, and
    /// one pressed *after* it is lost with it - a key in a frame long enough to hold
    /// both, which costs a second press.
    fn input(
        &mut self,
        ui: &egui::Ui,
        data: &ViewerData<'_>,
        zoom: &mut Zoom,
        area: Frame,
        controls: &Controls,
        zooms: bool,
    ) -> Vec<ViewerAction> {
        let mut actions = Vec::new();
        let size = (area.width, area.height);
        let middle = pos2(area.left + area.width / 2.0, area.top + area.height / 2.0);
        // Measured from the middle of the photo's area, as the pan is.
        let from_middle = |at: Pos2| Pan {
            x: at.x - middle.x,
            y: at.y - middle.y,
        };
        let (events, pointer, now) =
            ui.input(|input| (input.events.clone(), input.pointer.latest_pos(), input.time));
        // How far a press may move and be a click still, and how long after a click
        // the second of a double click may come: egui's own.
        let (click_dist, double_click) = ui.ctx().options(|options| {
            (
                options.input_options.max_click_dist,
                options.input_options.max_double_click_delay,
            )
        });
        // Where nothing zooms - a video stepped to, a photo that has gone - a press and
        // a click that began on what was there before are over: the second click of a
        // double click that comes up on a video's poster would zoom it, with no key, no
        // wheel and no slider there to put it back.
        if !zooms {
            self.press = None;
            self.clicked = None;
        }
        // Fitted, there is no drag: not for a press that went down on a fitted photo, and
        // not for one held from before a step, which put the next photo back to the fit
        // and must not bring the last photo's pan to it.
        if zoom.is_fit()
            && let Some(press) = &mut self.press
        {
            press.pan = None;
        }
        // Whether a press at `at` is the photo's: on no control, and under nothing that
        // lies over the viewer - a message, with its button. Asked of the layer egui
        // finds on top there, the one judge (`grid/view.rs`): whether a widget "contains
        // the pointer" is true of one under an area all along the inside of its edge.
        // (`layer_id_at` locks egui's memory: not inside an `input` closure.)
        let on_photo =
            |at: Pos2| !controls.hold(at) && ui.ctx().layer_id_at(at) == Some(ui.layer_id());
        // A zoom moves the pan, and a drag in progress is measured from where it began:
        // moved along with the photo, or the pointer's next move would put the pan back
        // where the drag had it and undo the zoom's.
        let zoomed = |press: &mut Option<Press>, moved: Pan| {
            if let Some(from) = press.as_mut().and_then(|press| press.pan.as_mut()) {
                from.x += moved.x;
                from.y += moved.y;
            }
        };
        let at_pointer = pointer.map_or(Pan::default(), from_middle);
        // While a keyword is being typed the keys are its field's: `h` is a letter, the
        // arrows move the caret, and Escape leaves the field and closes nothing.
        let typing = InfoView::typing(ui.ctx());
        let over_panel =
            pointer.is_some_and(|at| controls.panel.is_some_and(|panel| panel.contains(at)));
        for event in &events {
            match event {
                Event::Key { pressed: true, .. } if typing => {}
                Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    repeat,
                    modifiers,
                } => {
                    if modifiers.command || modifiers.ctrl || modifiers.alt {
                        continue;
                    }
                    // The key that types a period, wherever a keyboard has it
                    // (`grid/view.rs`, where the grid's star is read).
                    let types_period = !modifiers.shift || *physical_key != Some(Key::Period);
                    // The zoom's keys, about the middle as the slider's is. `=` is `+`
                    // without Shift where the two are one key.
                    let by = match key {
                        Key::Plus | Key::Equals => Some(ZOOM_KEY_STEP),
                        Key::Minus => Some(1.0 / ZOOM_KEY_STEP),
                        Key::Num0 => Some(0.0),
                        _ => None,
                    };
                    if let Some(by) = by {
                        if zooms {
                            let moved = zoom.by(by, Pan::default(), size);
                            zoomed(&mut self.press, moved);
                        }
                        continue;
                    }
                    actions.extend(match key {
                        Key::ArrowLeft => Some(ViewerAction::Step(-1)),
                        Key::ArrowRight => Some(ViewerAction::Step(1)),
                        Key::Home => Some(ViewerAction::First),
                        Key::End => Some(ViewerAction::Last),
                        Key::Escape | Key::Backspace if !repeat => Some(ViewerAction::Close),
                        Key::Period if !repeat && types_period => Some(ViewerAction::Star),
                        Key::H if !repeat => Some(ViewerAction::Hide),
                        Key::I if !repeat => Some(ViewerAction::Info),
                        _ => None,
                    });
                }
                // Over the info panel the wheel is the panel's, which is as tall as
                // what it has to say: by the system's own distance a notch, and to no
                // other photo.
                Event::MouseWheel { unit, delta, .. } if over_panel => {
                    let notch = ui
                        .ctx()
                        .options(|options| options.input_options.line_scroll_speed);
                    self.info.turned(match unit {
                        egui::MouseWheelUnit::Line => delta.y * notch,
                        egui::MouseWheelUnit::Point => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * area.height,
                    });
                }
                // With Ctrl or Cmd the wheel zooms, at the pointer - which is how a
                // pinch arrives too, where the system makes a wheel of it. Read from
                // the wheel's own event: egui's `zoom_delta` is the same turn scaled by
                // its scroll speed, which is the grid's and another on every system.
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if modifiers.command || modifiers.ctrl => {
                    // Away from the user is in, and below nought to a web view.
                    let turned = match unit {
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                            -delta.y * PIXELS_A_LINE
                        }
                        egui::MouseWheelUnit::Point => -delta.y,
                    };
                    // What had been turned towards the next photo is not kept for
                    // after the zoom.
                    self.wheel = 0.0;
                    if zooms {
                        let moved = zoom.by(wheel_zoom_factor(turned), at_pointer, size);
                        zoomed(&mut self.press, moved);
                    }
                }
                // The plain wheel goes from photo to photo: a notch a photo, whatever
                // the system's lines a notch are, since nothing here scrolls.
                Event::MouseWheel { unit, delta, .. } => {
                    let notches = match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / POINTS_A_NOTCH,
                        egui::MouseWheelUnit::Page => delta.y,
                    };
                    // Turned towards the user is on, as a page is scrolled down.
                    let (left, step) = wheel_step(self.wheel, -notches);
                    self.wheel = left;
                    if step != 0 {
                        actions.push(ViewerAction::Step(step));
                    }
                }
                // A pinch the system reports as one (a Mac's trackpad): by its own
                // factor, at the pointer.
                Event::Zoom(factor) if zooms => {
                    let moved = zoom.by(*factor, at_pointer, size);
                    zoomed(&mut self.press, moved);
                }
                Event::PointerButton {
                    button: PointerButton::Extra1,
                    pressed: true,
                    ..
                } => actions.push(ViewerAction::Close),
                // The first button on the photo: a click when it comes up where it
                // went down, a pan while it is held and the photo is zoomed. Read from
                // the button's own events, as the grid's band is: egui's click gives up
                // after a moment and its drag at the release of any button.
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } => {
                    // A button down again is a gesture begun again: the slider follows.
                    self.let_go = false;
                    // (Where nothing zooms, and on a fitted photo, the press is over
                    // or no drag by the next frame: above.)
                    self.press = on_photo(*pos).then(|| Press {
                        at: *pos,
                        pan: Some(zoom.pan()),
                        moved: false,
                    });
                }
                Event::PointerMoved(to) => {
                    if let Some(press) = &mut self.press {
                        press.moved |= press.at.distance(*to) > click_dist;
                        if let Some(from) = press.pan {
                            let by = *to - press.at;
                            zoom.pan_to(from.x + by.x, from.y + by.y, size);
                        }
                    }
                }
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    let Some(press) = self.press.take() else {
                        continue;
                    };
                    if press.moved {
                        continue;
                    }
                    // A click. The second of two in the same place, in the time a
                    // double click has: to the photo's own pixels where it was clicked,
                    // and from anything closer than the fit back to the whole photo.
                    let second = (self.clicked.take()).is_some_and(|(when, at)| {
                        now - when <= double_click && at.distance(*pos) <= click_dist
                    });
                    if !second {
                        self.clicked = Some((now, *pos));
                    } else if zoom.is_fit() {
                        // Its own pixels, as the library says them; until it has, the
                        // largest level there is to draw; with neither, "closer".
                        let own = match data.width {
                            0 => data.levels.first().map_or(0, |level| level.width),
                            width => width,
                        };
                        let next = actual_size_zoom(
                            own as f32,
                            fitted_width(data, area),
                            ui.ctx().pixels_per_point(),
                        );
                        zoom.to(next, from_middle(*pos), size);
                    } else {
                        zoom.to(MIN_ZOOM, Pan::default(), size);
                    }
                }
                // The gesture taken away with no release: the window left - a button
                // let go of over another window may never be heard of - or a touch the
                // system cancels. The pan stays where the drag had brought it.
                Event::WindowFocused(false)
                | Event::Touch {
                    phase: egui::TouchPhase::Cancel,
                    ..
                } => {
                    self.press = None;
                    self.let_go = true;
                }
                _ => {}
            }
        }
        // A hand over a photo that can be moved: closed while it holds the photo,
        // wherever the pointer has gone with it - over a button too, with the photo
        // still following - and open over the photo otherwise. Not where nothing is
        // drawn that could move: a photo that was zoomed and has gone.
        let holding = (self.press.as_ref()).is_some_and(|press| press.pan.is_some());
        if zooms && !zoom.is_fit() && (holding || pointer.is_some_and(on_photo)) {
            ui.ctx().set_cursor_icon(if holding {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        actions
    }
}

/// How many pixels the fitted copy may differ from what is drawn and still be the copy
/// for this size.
const SAME_SIZE: f32 = 1.5;

/// How many points of a trackpad's stream are one notch of a wheel: what the Svelte
/// viewer takes for one photo.
const POINTS_A_NOTCH: f32 = 100.0;

/// How wide the photo is drawn at fit, in points: by its full picture's shape, else its
/// preview's; nought while there is neither.
fn fitted_width(data: &ViewerData<'_>, area: Frame) -> f32 {
    (data.levels.first().map(|level| level.texture))
        .or(data.preview)
        .map_or(0.0, |texture| {
            let [width, height] = texture.size();
            contained(width as f32, height as f32, area).width
        })
}

/// The texture to draw and where: fitted into `area`, then `zoom` times that size about
/// the middle of the area and moved by the pan. At fit, the copy made for exactly this
/// size, where there is one; else the level that is at least as large as what is drawn;
/// else the preview. None while there is none of them.
fn picture<'a>(
    ui: &egui::Ui,
    data: &ViewerData<'a>,
    area: Frame,
    zoom: &Zoom,
) -> Option<(&'a TextureHandle, Rect)> {
    let texture = (data.levels.first().map(|level| level.texture)).or(data.preview)?;
    let [width, height] = texture.size();
    let fitted = contained(width as f32, height as f32, area);
    let fit = Rect::from_min_size(
        pos2(fitted.left, fitted.top),
        vec2(fitted.width, fitted.height),
    );
    // A point `q` of the fitted photo, from the middle of the area, is at `pan + zoom *
    // q` (`viewer_zoom.rs`).
    let middle = pos2(area.left + area.width / 2.0, area.top + area.height / 2.0);
    let pan = vec2(zoom.pan().x, zoom.pan().y);
    let at = Rect::from_min_size(
        middle + pan + (fit.min - middle) * zoom.zoom(),
        fit.size() * zoom.zoom(),
    );
    if data.levels.is_empty() {
        return Some((texture, at));
    }
    let scale = ui.ctx().pixels_per_point();
    let drawn = at.size() * scale;
    // The copy scaled for this size is drawn pixel for pixel, on whole pixels: a
    // linear filter then has nothing to do, and what a proper one made of the photo is
    // what is seen. A pixel's difference is the roundings - of the box to whole pixels,
    // of the copy's own size; more is a window that has changed size since, and the
    // levels until the copy for the new one is made. And only at fit: a zoom can be
    // the copy's size too - a hair over the fit, or twice in a window half as wide as
    // the one the copy on hand was made for - and the copy is then no picture of what
    // the zoom shows.
    if zoom.is_fit()
        && let Some(copy) = data.fitted
    {
        let [width, height] = copy.size();
        let own = vec2(width as f32, height as f32);
        if (own.x - drawn.x).abs() < SAME_SIZE && (own.y - drawn.y).abs() < SAME_SIZE {
            let corner = ((at.center().to_vec2() * scale - own / 2.0).round() / scale).to_pos2();
            return Some((copy, Rect::from_min_size(corner, own / scale)));
        }
    }
    let widths: Vec<u32> = data.levels.iter().map(|level| level.width).collect();
    let level = data.levels[level_for(&widths, drawn.x)];
    // At the level's own pixels - what a double click zooms to, and what a photo is
    // checked for sharpness at - it stands on whole pixels too: half a pixel off them
    // is the linear filter's blur over the whole photo.
    let own = (level.width as f32 - drawn.x).abs() < 0.5;
    let at = if own {
        Rect::from_min_size(
            ((at.min.to_vec2() * scale).round() / scale).to_pos2(),
            at.size(),
        )
    } else {
        at
    };
    Some((level.texture, at))
}

/// The glass a control stands on.
fn glass(ui: &egui::Ui, rect: Rect, rounding: f32) {
    let painter = ui.painter();
    painter.add(menu_shadow().as_shape(rect, rounding));
    painter.rect_filled(rect, rounding, color(GLASS));
    painter.rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0, color(GLASS_LINE)),
        StrokeKind::Outside,
    );
}

/// A round button on the glass: the one before, the one after, close. One that leads
/// nowhere is dimmed and takes no press, and is in its place all the same.
fn round(ui: &egui::Ui, rect: Rect, id: &str, icon: Icon, label: &str, works: bool) -> bool {
    let sense = if works { Sense::CLICK } else { Sense::hover() };
    let response = ui.interact(rect, Id::new(id), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, works, label));
    glass(ui, rect, rect.height() / 2.0);
    let tint = match (works, response.hovered()) {
        (false, _) => color(DARK.text_dim).gamma_multiply(0.35),
        (true, true) => color(DARK.text),
        (true, false) => color(DARK.text_dim),
    };
    icon.paint(ui, rect, 18.0, false, tint);
    response.clicked()
}

/// Where the bar at the foot of the window stands: as wide as its tools and its caption,
/// which is the part that gives way when the window is narrow. With no caption - the
/// library has not said yet what the photo stepped to is - as wide as it `was`: shrunk
/// to its tools for the frame or two a step's answer takes, the bar and the buttons in
/// it jumped twice at every step.
fn bar_place(ui: &egui::Ui, window: Rect, caption: &str, was: Option<f32>) -> Rect {
    let wanted = ui
        .painter()
        .layout_no_wrap(
            caption.to_owned(),
            fonts::regular(T[1]),
            color(DARK.text_dim),
        )
        .size()
        .x;
    let pad = (BAR - TOOL) / 2.0;
    let tools = TOOLS as f32 * TOOL + (TOOLS - 1) as f32 * TOOL_GAP;
    let most = (window.width() - 2.0 * BESIDE_BAR).max(tools + 2.0 * pad);
    let own = pad + tools + S[1] + wanted + S[2];
    let width = (was.filter(|_| caption.is_empty()).unwrap_or(own)).min(most);
    Rect::from_min_size(
        pos2(
            window.center().x - width / 2.0,
            window.bottom() - EDGE - BAR,
        ),
        vec2(width, BAR),
    )
}

/// Where the zoom's slider stands: in the corner beside the bar.
fn slider_place(window: Rect) -> Rect {
    let width = S[2] + SLIDER + S[1] + SLIDER_LEVEL + S[2];
    Rect::from_min_size(
        pos2(window.right() - EDGE - width, window.bottom() - EDGE - BAR),
        vec2(width, BAR),
    )
}

/// The zoom's slider and what it stands at, in hundredths: a press anywhere along it
/// zooms to there, and a drag follows the pointer. It takes no press where there is
/// nothing to zoom, and is in its place all the same. No stop of the Tab key, as no
/// control here is: `+`, `-` and `0` are its keys. It `follows` the first button, held,
/// for as long as the gesture is the window's.
fn slider(
    ui: &egui::Ui,
    place: Rect,
    zoom: &mut Zoom,
    area: (f32, f32),
    works: bool,
    follows: bool,
) {
    glass(ui, place, R[3]);
    let track = Rect::from_min_size(
        pos2(place.left() + S[2], place.center().y - 2.0),
        vec2(SLIDER, 4.0),
    );
    let reach = Rect::from_min_max(
        pos2(track.left() - S[1], place.top()),
        pos2(track.right() + S[1], place.bottom()),
    );
    let sense = if works {
        Sense::CLICK | Sense::DRAG
    } else {
        Sense::hover()
    };
    let response = ui.interact(reach, Id::new("viewer-zoom"), sense);
    // (egui's "a button is down on it" is of whichever button went down.)
    if follows
        && response.is_pointer_button_down_on()
        && ui.input(|input| input.pointer.primary_down())
        && let Some(at) = response.interact_pointer_pos()
    {
        // (Past either end of the track is past either end of the range, where the
        // zoom stops by itself.)
        let along = (at.x - track.left()) / track.width();
        let value = MIN_ZOOM + along * (MAX_ZOOM - MIN_ZOOM);
        zoom.set((value / SLIDER_STEP).round() * SLIDER_STEP, area);
    }
    let value = zoom.zoom();
    response.widget_info(|| WidgetInfo::slider(works, f64::from(value), "Zoom"));

    let along = (value - MIN_ZOOM) / (MAX_ZOOM - MIN_ZOOM);
    let thumb = pos2(track.left() + along * track.width(), track.center().y);
    let dim = if works { 1.0 } else { 0.4 };
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, color(GLASS_LINE).gamma_multiply(dim));
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(thumb.x, track.bottom())),
        2.0,
        color(DARK.accent).gamma_multiply(dim),
    );
    painter.circle_filled(thumb, 6.0, color(DARK.text).gamma_multiply(dim));
    let level = format!("{}%", (value * 100.0).round());
    let room = Rect::from_min_max(
        pos2(track.right() + S[1], place.top()),
        pos2(place.right() - S[2], place.bottom()),
    );
    let galley = painter.layout_no_wrap(
        level,
        fonts::regular(T[1]),
        color(DARK.text_dim).gamma_multiply(dim),
    );
    painter.galley(
        pos2(
            room.right() - galley.size().x,
            room.center().y - galley.size().y / 2.0,
        ),
        galley,
        color(DARK.text_dim),
    );
}

/// The bar at the foot of the window: its tools - the star, the info panel - and the
/// caption. Answers what was pressed in it.
fn bar(ui: &egui::Ui, rect: Rect, data: &ViewerData<'_>) -> Vec<ViewerAction> {
    let font = fonts::regular(T[1]);
    let pad = (BAR - TOOL) / 2.0;
    glass(ui, rect, R[3]);
    let mut pressed = Vec::new();

    let star = Rect::from_min_size(rect.min + vec2(pad, pad), vec2(TOOL, TOOL));
    let works = data.starred.is_some();
    let starred = data.starred == Some(true);
    let sense = if works { Sense::CLICK } else { Sense::hover() };
    let response = ui.interact(star, Id::new("viewer-star"), sense);
    let label = if starred { "Unstar" } else { "Star" };
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, works, starred, label));
    if works && response.hovered() {
        ui.painter().rect_filled(star, R[3], color(DARK.hover));
    }
    let tint = match (works, starred, response.hovered()) {
        (false, _, _) => color(DARK.text_dim).gamma_multiply(0.4),
        (true, true, _) => color(DARK.star),
        (true, false, true) => color(DARK.text),
        (true, false, false) => color(DARK.text_dim),
    };
    Icon::Star.paint(ui, star, 16.0, starred, tint);
    if response.clicked() {
        pressed.push(ViewerAction::Star);
    }

    // The info panel's button: lit while the panel is open.
    let info = star.translate(vec2(TOOL + TOOL_GAP, 0.0));
    let response = ui.interact(info, Id::new("viewer-info"), Sense::CLICK);
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::Button,
            true,
            data.info_open,
            "Photo information",
        )
    });
    if data.info_open || response.hovered() {
        ui.painter().rect_filled(info, R[3], color(DARK.hover));
    }
    let tint = if data.info_open || response.hovered() {
        color(DARK.text)
    } else {
        color(DARK.text_dim)
    };
    Icon::Info.paint(ui, info, 16.0, false, tint);
    if response.clicked() {
        pressed.push(ViewerAction::Info);
    }

    let left = info.right() + S[1];
    let room = rect.right() - S[2] - left;
    if room > 0.0 {
        let painter = ui.painter().with_clip_rect(rect);
        let caption = Rect::from_min_max(pos2(left, rect.top()), pos2(left + room, rect.bottom()));
        ui.interact(caption, Id::new("viewer-caption"), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, data.caption));
        text::paint_line(
            ui,
            &painter,
            (left, rect.center().y),
            room,
            data.caption,
            font,
            color(DARK.text_dim),
        );
    }
    pressed
}

/// Where the photo's own caption stands, and its words laid out: over the bar and
/// centred like it, two lines at most, so that a long one does not climb over the photo.
/// The info panel has it whole.
fn caption_place(ui: &egui::Ui, window: Rect, line: &str) -> (Rect, std::sync::Arc<egui::Galley>) {
    let most = (window.width() - 2.0 * BESIDE_BAR - 2.0 * S[2]).max(0.0);
    let mut job = egui::text::LayoutJob::simple(
        line.to_owned(),
        fonts::regular(T[1]),
        color(DARK.text),
        most,
    );
    job.wrap.max_rows = 2;
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let size = galley.size() + vec2(2.0 * S[2], 2.0 * S[0]);
    let rect = Rect::from_min_size(
        pos2(
            window.center().x - size.x / 2.0,
            window.bottom() - CAPTION_BOTTOM - size.y,
        ),
        size,
    );
    (rect, galley)
}

/// The photo's own caption, drawn where `caption_place` put it.
fn photo_caption(ui: &egui::Ui, rect: Rect, galley: std::sync::Arc<egui::Galley>, line: &str) {
    glass(ui, rect, R[3]);
    ui.interact(rect, Id::new("viewer-photo-caption"), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, line));
    // A centred galley is placed by its middle.
    ui.painter().galley(
        pos2(rect.center().x, rect.top() + S[0]),
        galley,
        color(DARK.text),
    );
}

/// What is said where no picture can be shown, or over one that is only a poster.
fn said(ui: &egui::Ui, window: Rect, message: Message<'_>) {
    let most = (window.width() - 2.0 * MESSAGE_SIDE).clamp(0.0, 520.0);
    let tint = if message.over {
        color(DARK.text)
    } else {
        color(DARK.text_dim)
    };
    let mut job =
        egui::text::LayoutJob::simple(message.text.to_owned(), fonts::regular(T[2]), tint, most);
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let size = galley.size();
    let rect = Rect::from_center_size(window.center(), size);
    if message.over {
        glass(ui, rect.expand2(vec2(S[2], S[1])), R[3]);
    }
    ui.interact(rect, Id::new("viewer-message"), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, message.text));
    ui.painter()
        .galley(pos2(rect.center().x, rect.top()), galley, tint);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{ColorImage, Modifiers, Pos2, RawInput, TextureOptions, Vec2};

    const WINDOW: Vec2 = vec2(1000.0, 700.0);

    fn each_shape(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
            other => visit(other),
        }
    }

    /// The viewer over a picture of `sizes` (the levels' sizes, largest first) with a
    /// preview of `preview`.
    struct Fixture {
        ctx: egui::Context,
        view: ViewerView,
        time: f64,
        size: Vec2,
        scale: f32,
        levels: Vec<(TextureHandle, u32)>,
        fitted: Option<TextureHandle>,
        preview: Option<TextureHandle>,
        message: Option<(&'static str, bool)>,
        caption: String,
        photo_caption: Option<&'static str>,
        starred: Option<bool>,
        can_prev: bool,
        can_next: bool,
        info_open: bool,
        /// What the library has said of the photo, for the open panel.
        item: Option<photon_engine::commands::ViewerItem>,
        /// The photo's width in pixels as the library says it; nought until it has.
        width: u32,
        video: bool,
        zoom: Zoom,
        /// The textures painted in the last frame, each with where.
        images: Vec<(egui::TextureId, Rect)>,
        /// What each of them was cut off at.
        clips: Vec<Rect>,
        texts: Vec<(String, Rect)>,
        fills: Vec<(Rect, Color32)>,
        repaint: std::time::Duration,
        cursor: CursorIcon,
    }

    fn texture(ctx: &egui::Context, name: &str, [width, height]: [usize; 2]) -> TextureHandle {
        ctx.load_texture(
            name,
            ColorImage::filled([width, height], Color32::GRAY),
            TextureOptions::LINEAR,
        )
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                ctx: egui::Context::default(),
                view: ViewerView::default(),
                time: 0.0,
                size: WINDOW,
                scale: 1.0,
                levels: Vec::new(),
                fitted: None,
                preview: None,
                message: None,
                caption: "a.jpg · Jul 14, 2026, 12:30 PM · 4000 × 3000 · 2.4 MB (1 / 3)".to_owned(),
                photo_caption: None,
                starred: Some(false),
                can_prev: true,
                can_next: true,
                info_open: false,
                item: None,
                width: 0,
                video: false,
                zoom: Zoom::default(),
                images: Vec::new(),
                clips: Vec::new(),
                texts: Vec::new(),
                fills: Vec::new(),
                repaint: std::time::Duration::MAX,
                cursor: CursorIcon::Default,
            }
        }

        fn with_preview(mut self, size: [usize; 2]) -> Self {
            self.preview = Some(texture(&self.ctx, "preview", size));
            self
        }

        /// The full picture's levels, by their sizes in pixels, largest first. (Small
        /// textures stand in for them: a level is known by the width it is said to have.)
        fn with_levels(mut self, sizes: &[[u32; 2]]) -> Self {
            self.levels = (sizes.iter().enumerate())
                .map(|(n, [width, height])| {
                    let small = [(*width / 100) as usize, (*height / 100) as usize];
                    (texture(&self.ctx, &format!("level-{n}"), small), *width)
                })
                .collect();
            self
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<ViewerAction> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                viewports: std::iter::once((
                    egui::ViewportId::ROOT,
                    egui::ViewportInfo {
                        native_pixels_per_point: Some(self.scale),
                        ..Default::default()
                    },
                ))
                .collect(),
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let levels: Vec<Level<'_>> = (self.levels.iter())
                .map(|(texture, width)| Level {
                    texture,
                    width: *width,
                })
                .collect();
            let zone = jiff::tz::TimeZone::UTC;
            let data = ViewerData {
                levels: &levels,
                fitted: self.fitted.as_ref(),
                preview: self.preview.as_ref(),
                message: (self.message).map(|(text, over)| Message { text, over }),
                caption: &self.caption,
                photo_caption: self.photo_caption,
                starred: self.starred,
                width: self.width,
                video: self.video,
                can_prev: self.can_prev,
                can_next: self.can_next,
                info_open: self.info_open,
                info: (self.item.as_ref()).map(|item| InfoData {
                    item,
                    albums: &[],
                    finds_faces: false,
                    looking: false,
                    zone: &zone,
                }),
            };
            let (view, zoom) = (&mut self.view, &mut self.zoom);
            let mut actions = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| actions = view.show(ui, &data, zoom));
            });
            full.textures_delta.clear();
            self.cursor = full.platform_output.cursor_icon;
            self.images.clear();
            self.clips.clear();
            self.texts.clear();
            self.fills.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => self
                        .texts
                        .push((text.galley.text().to_owned(), text.visual_bounding_rect())),
                    egui::Shape::Rect(rect) => match &rect.brush {
                        Some(brush) => {
                            self.images.push((brush.fill_texture_id, rect.rect));
                            self.clips.push(clipped.clip_rect);
                        }
                        None => self.fills.push((rect.rect, rect.fill)),
                    },
                    egui::Shape::Mesh(mesh) => {
                        self.images.push((mesh.texture_id, mesh.calc_bounds()));
                        self.clips.push(clipped.clip_rect);
                    }
                    _ => {}
                });
            }
            self.repaint = full
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay);
            actions
        }

        /// The photo as drawn: which texture, and where. Icons are images too, and are
        /// told from it by their size.
        fn photo(&self) -> Option<(egui::TextureId, Rect)> {
            (self.images.iter())
                .find(|(_, rect)| rect.width() > 100.0)
                .copied()
        }

        fn key(&mut self, key: Key) -> Vec<ViewerAction> {
            self.key_with(key, Modifiers::NONE)
        }

        fn key_with(&mut self, key: Key, modifiers: Modifiers) -> Vec<ViewerAction> {
            let event = |pressed| Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            };
            let actions = self.frame(vec![event(true)]);
            self.frame(vec![event(false)]);
            actions
        }

        fn click(&mut self, at: Pos2) -> Vec<ViewerAction> {
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![primary(at, true)]);
            self.frame(vec![primary(at, false)])
        }

        /// The pointer taken to `at`, a frame.
        fn point(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
        }

        /// The first button down at `at`, the pointer there already or not.
        fn press(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![primary(at, true)]);
        }

        fn release(&mut self, at: Pos2) {
            self.frame(vec![primary(at, false)]);
        }

        /// The zoom and the pan, as numbers.
        fn zoomed(&self) -> (f32, (f32, f32)) {
            (self.zoom.zoom(), (self.zoom.pan().x, self.zoom.pan().y))
        }

        fn wheel(
            &mut self,
            unit: egui::MouseWheelUnit,
            y: f32,
            modifiers: Modifiers,
        ) -> Vec<ViewerAction> {
            self.frame(vec![Event::MouseWheel {
                unit,
                delta: vec2(0.0, y),
                phase: egui::TouchPhase::Move,
                modifiers,
            }])
        }

        fn drew(&self, text: &str) -> Option<Rect> {
            (self.texts.iter())
                .find(|(drawn, _)| drawn == text)
                .map(|(_, place)| *place)
        }
    }

    fn primary(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    /// A photo 2,400 pixels wide in its full picture and its half, with its preview.
    fn photo() -> Fixture {
        let mut f = Fixture::new()
            .with_preview([1600, 800])
            .with_levels(&[[2400, 1200], [1200, 600]]);
        f.frame(Vec::new());
        f
    }

    #[test]
    fn the_ground_is_black_whatever_the_theme() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let mut f = Fixture::new();
            f.ctx.set_theme(theme);
            f.frame(Vec::new());
            let whole = Rect::from_min_size(Pos2::ZERO, WINDOW);
            assert!(
                (f.fills.iter()).any(|(rect, fill)| *rect == whole && *fill == Color32::BLACK),
                "{theme:?}"
            );
        }
    }

    #[test]
    fn the_preview_is_fitted_to_the_window_and_centred() {
        // Landscape: its width reaches the window's first.
        let mut f = Fixture::new().with_preview([1600, 800]);
        f.frame(Vec::new());
        let (id, at) = f.photo().expect("the preview is drawn");
        assert_eq!(id, f.preview.as_ref().unwrap().id());
        assert_eq!(
            at,
            Rect::from_min_size(pos2(0.0, 100.0), vec2(1000.0, 500.0))
        );
        // Upright: its height does.
        let mut f = Fixture::new().with_preview([800, 1600]);
        f.frame(Vec::new());
        let (_, at) = f.photo().unwrap();
        assert_eq!(
            at,
            Rect::from_min_size(pos2(325.0, 0.0), vec2(350.0, 700.0))
        );
        // And in a window made another size, in the frame it is.
        f.size = vec2(600.0, 600.0);
        f.frame(Vec::new());
        assert_eq!(
            f.photo().unwrap().1,
            Rect::from_min_size(pos2(150.0, 0.0), vec2(300.0, 600.0))
        );
    }

    // A photo whose preview is not to be had - not cached, or not made in time - is
    // drawn from its full picture all the same, fitted by that picture's own shape.
    #[test]
    fn the_full_picture_is_drawn_where_there_is_no_preview() {
        let mut f = Fixture::new().with_levels(&[[1000, 2000]]);
        f.frame(Vec::new());
        let (id, at) = f.photo().expect("the full picture");
        assert_eq!(id, f.levels[0].0.id());
        assert!((at.width() - 350.0).abs() < 0.01 && (at.height() - 700.0).abs() < 0.01);
    }

    #[test]
    fn nothing_is_drawn_for_a_photo_with_no_picture_yet() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.photo(), None);
    }

    // The full picture is drawn in place of the preview, from the level that is at
    // least as large as what is drawn: a 6,000-pixel photo fitted into a thousand
    // points at one pixel a point is drawn from its 1,500-pixel level.
    #[test]
    fn the_full_picture_is_drawn_from_the_level_that_fits_what_is_drawn() {
        let mut f = Fixture::new().with_preview([1600, 800]).with_levels(&[
            [6000, 3000],
            [3000, 1500],
            [1500, 750],
        ]);
        f.frame(Vec::new());
        let (id, at) = f.photo().unwrap();
        assert_eq!(id, f.levels[2].0.id(), "the smallest that is wide enough");
        let fitted = Rect::from_min_size(pos2(0.0, 100.0), vec2(1000.0, 500.0));
        assert!((at.min - fitted.min).length() < 0.01 && (at.max - fitted.max).length() < 0.01);
        // On a screen of two pixels a point the same thousand points are two thousand
        // pixels, and the level above is the one.
        f.scale = 2.0;
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.levels[1].0.id());
        // In a window narrow enough for the smallest again.
        f.size = vec2(700.0, 700.0);
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.levels[2].0.id());
    }

    // Fitted, the photo is drawn from the copy that was scaled for exactly that size:
    // pixel for pixel, on whole pixels. In a window that has changed size since, from
    // the levels, until the copy for the new size is made.
    #[test]
    fn a_fitted_photo_is_drawn_from_the_copy_made_for_its_size() {
        let mut f = Fixture::new().with_preview([1600, 800]).with_levels(&[
            [6000, 3000],
            [3000, 1500],
            [1500, 750],
        ]);
        f.fitted = Some(texture(&f.ctx, "fitted", [1000, 500]));
        f.frame(Vec::new());
        let (id, at) = f.photo().unwrap();
        assert_eq!(id, f.fitted.as_ref().unwrap().id());
        assert_eq!(
            at,
            Rect::from_min_size(pos2(0.0, 100.0), vec2(1000.0, 500.0))
        );
        // A pixel off is the rounding of a size that is no whole number: the copy
        // still, at its own size, on whole pixels.
        f.fitted = Some(texture(&f.ctx, "fitted", [1000, 501]));
        f.frame(Vec::new());
        let (id, at) = f.photo().unwrap();
        assert_eq!(id, f.fitted.as_ref().unwrap().id());
        assert_eq!(at.size(), vec2(1000.0, 501.0));
        assert_eq!(at.min, pos2(0.0, 100.0), "on a whole pixel: {at:?}");
        // Two pixels off is another size.
        f.fitted = Some(texture(&f.ctx, "fitted", [1000, 502]));
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.levels[2].0.id());
        // The window made narrower: the copy is of another size, and is not stretched.
        f.fitted = Some(texture(&f.ctx, "fitted", [1000, 500]));
        f.size = vec2(900.0, 700.0);
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.levels[2].0.id());
        // On a screen of two pixels a point the copy is twice the points it is drawn in.
        f.size = WINDOW;
        f.scale = 2.0;
        f.fitted = Some(texture(&f.ctx, "fitted", [2000, 1000]));
        f.frame(Vec::new());
        f.frame(Vec::new());
        let (id, at) = f.photo().unwrap();
        assert_eq!(id, f.fitted.as_ref().unwrap().id());
        assert_eq!(
            at,
            Rect::from_min_size(pos2(0.0, 100.0), vec2(1000.0, 500.0))
        );
    }

    #[test]
    fn what_cannot_be_shown_is_said_in_the_photos_place() {
        let mut f = Fixture::new().with_preview([1600, 800]);
        f.message = Some(("This photo is no longer available.", false));
        f.frame(Vec::new());
        let said = f
            .drew("This photo is no longer available.")
            .expect("the message");
        assert!((said.center().x - 500.0).abs() < 1.0 && (said.center().y - 350.0).abs() < 8.0);
        assert_eq!(f.photo(), None, "and no picture under it");
        // A video's line stands over its poster.
        f.message = Some(("The native window does not play videos yet.", true));
        f.frame(Vec::new());
        assert!(
            f.drew("The native window does not play videos yet.")
                .is_some()
        );
        assert!(f.photo().is_some());
    }

    #[test]
    fn the_caption_is_in_the_bar_at_the_foot_of_the_window() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let caption = f.drew(&f.caption.clone()).expect("the caption");
        assert!(caption.bottom() < 700.0 - EDGE && caption.top() > 700.0 - EDGE - BAR);
        // In the middle, with the bar's tools before it.
        assert!((caption.center().x - 500.0).abs() < 50.0, "{caption:?}");
        // The photo's own caption stands over the bar, only where there is one.
        assert_eq!(f.drew("Lisbon, at dusk"), None);
        f.photo_caption = Some("Lisbon, at dusk");
        f.frame(Vec::new());
        let own = f.drew("Lisbon, at dusk").unwrap();
        assert!(own.bottom() < caption.top());
        assert!((own.center().x - 500.0).abs() < 2.0, "{own:?}");
    }

    // A photo's own caption can be a paragraph. Over the photo it is two lines at most:
    // the info panel has it whole.
    #[test]
    fn the_photos_own_caption_is_two_lines_at_most() {
        let mut f = Fixture::new();
        f.photo_caption = Some("Lisbon, at dusk");
        f.frame(Vec::new());
        let one = f.drew("Lisbon, at dusk").unwrap().height();
        let long = "Lisbon, at dusk, from the hill above the river, on the last evening \
                    of a week in which it rained on every day but this one, and the whole \
                    town was out; the light went in a quarter of an hour, and by the time \
                    the tram came back up the hill the lamps were on in every street below, \
                    and nobody on the terrace had gone home.";
        f.photo_caption = Some(long);
        f.frame(Vec::new());
        let drawn = f.drew(long).expect("the caption");
        assert!(drawn.height() > one * 1.5, "on more than one line");
        assert!(drawn.height() < one * 2.8, "and not on three: {drawn:?}");
        assert!(drawn.bottom() < 700.0 - CAPTION_BOTTOM + 1.0);
    }

    // The caption is the part of the bar that gives way: in a narrow window it is cut
    // short, and the bar keeps clear of the corners. (A galley's text is the whole text
    // whether or not it is drawn whole: what is seen is where it ends.)
    #[test]
    fn a_long_caption_is_cut_short_in_a_narrow_window() {
        let mut f = Fixture::new();
        f.caption = format!(
            "A file with a long name of its own, from a camera.jpg · {}",
            f.caption
        );
        f.size = vec2(1600.0, 600.0);
        f.frame(Vec::new());
        let whole = f.drew(&f.caption.clone()).unwrap().width();
        f.size = vec2(800.0, 600.0);
        f.frame(Vec::new());
        let glass = color(GLASS);
        let bar = (f.fills.iter())
            .filter(|(rect, fill)| *fill == glass && rect.height() == BAR)
            .map(|(rect, _)| *rect)
            .next()
            .expect("the bar");
        assert!(
            bar.left() >= BESIDE_BAR - 0.5 && bar.right() <= 800.0 - BESIDE_BAR + 0.5,
            "{bar:?}"
        );
        let cut = f.drew(&f.caption.clone()).unwrap();
        assert!(cut.width() < whole - 100.0, "{cut:?} of {whole}");
        assert!(
            cut.left() > bar.left() + TOOL && cut.right() <= bar.right(),
            "{cut:?} in {bar:?}"
        );
    }

    #[test]
    fn the_buttons_answer_what_they_are_for() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let side = |left: f32| pos2(left + NAV / 2.0, 350.0);
        assert_eq!(f.click(side(EDGE)), [ViewerAction::Step(-1)]);
        assert_eq!(f.click(side(1000.0 - EDGE - NAV)), [ViewerAction::Step(1)]);
        let close = pos2(1000.0 - EDGE - CLOSE / 2.0, EDGE + CLOSE / 2.0);
        assert_eq!(f.click(close), [ViewerAction::Close]);
        // A press on the photo, beside every control, is nothing yet.
        assert_eq!(f.click(pos2(500.0, 300.0)), []);
    }

    // At the ends of the view the buttons are in their place and take no press.
    #[test]
    fn a_button_that_leads_nowhere_takes_no_press() {
        let mut f = Fixture::new();
        f.can_prev = false;
        f.can_next = false;
        f.frame(Vec::new());
        assert_eq!(f.click(pos2(EDGE + NAV / 2.0, 350.0)), []);
        assert_eq!(f.click(pos2(1000.0 - EDGE - NAV / 2.0, 350.0)), []);
    }

    fn star_at(f: &Fixture) -> Pos2 {
        let glass = color(GLASS);
        let bar = (f.fills.iter())
            .find(|(rect, fill)| *fill == glass && rect.height() == BAR)
            .map(|(rect, _)| *rect)
            .expect("the bar");
        pos2(bar.left() + BAR / 2.0, bar.center().y)
    }

    #[test]
    fn the_star_is_pressed_in_the_bar_and_not_while_nothing_is_known_of_it() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let star = star_at(&f);
        assert_eq!(f.click(star), [ViewerAction::Star]);
        f.starred = None;
        f.frame(Vec::new());
        assert_eq!(f.click(star), []);
    }

    #[test]
    fn the_keys_step_close_star_and_hide() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::ArrowRight), [ViewerAction::Step(1)]);
        assert_eq!(f.key(Key::ArrowLeft), [ViewerAction::Step(-1)]);
        assert_eq!(f.key(Key::Home), [ViewerAction::First]);
        assert_eq!(f.key(Key::End), [ViewerAction::Last]);
        assert_eq!(f.key(Key::Escape), [ViewerAction::Close]);
        assert_eq!(f.key(Key::Backspace), [ViewerAction::Close]);
        assert_eq!(f.key(Key::Period), [ViewerAction::Star]);
        assert_eq!(f.key(Key::H), [ViewerAction::Hide]);
        assert_eq!(f.key(Key::Q), []);
        // A chord is not the viewer's.
        for held in [Modifiers::CTRL, Modifiers::COMMAND, Modifiers::ALT] {
            assert_eq!(f.key_with(Key::ArrowRight, held), [], "{held:?}");
            assert_eq!(f.key_with(Key::H, held), []);
        }
        // Shift with the period's own key types `>`, which is not the star's.
        assert_eq!(f.key_with(Key::Period, Modifiers::SHIFT), []);
    }

    // A held arrow steps again at each repeat; a held Escape, period or H does not act
    // again - the Escape that closed a dialog would close the viewer under it, and the
    // other two write. (egui decides for itself whether a press is a repeat - the key
    // has not come up - whatever the event says: the key is held across the frames.)
    #[test]
    fn a_key_held_steps_again_and_does_not_close_or_write_again() {
        let down = |key| Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(
            f.frame(vec![down(Key::ArrowRight)]),
            [ViewerAction::Step(1)]
        );
        assert_eq!(
            f.frame(vec![down(Key::ArrowRight)]),
            [ViewerAction::Step(1)]
        );
        for (key, first) in [
            (Key::Escape, ViewerAction::Close),
            (Key::Backspace, ViewerAction::Close),
            (Key::Period, ViewerAction::Star),
            (Key::H, ViewerAction::Hide),
        ] {
            assert_eq!(f.frame(vec![down(key)]), [first], "{key:?}");
            assert_eq!(f.frame(vec![down(key)]), [], "{key:?} held");
            assert_eq!(f.frame(vec![down(key)]), [], "{key:?} held on");
        }
    }

    // Several keys of one long frame are answered in the order they were pressed.
    #[test]
    fn the_keys_of_one_frame_are_answered_in_their_order() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let down = |key| Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(
            f.frame(vec![
                down(Key::ArrowRight),
                down(Key::H),
                down(Key::ArrowLeft)
            ]),
            [
                ViewerAction::Step(1),
                ViewerAction::Hide,
                ViewerAction::Step(-1)
            ]
        );
    }

    #[test]
    fn a_notch_of_the_wheel_is_a_photo_and_a_trackpad_adds_up_to_one() {
        use egui::MouseWheelUnit::{Line, Point};
        let mut f = Fixture::new();
        f.frame(Vec::new());
        // Towards the user is the next photo.
        assert_eq!(
            f.wheel(Line, -1.0, Modifiers::NONE),
            [ViewerAction::Step(1)]
        );
        assert_eq!(
            f.wheel(Line, 1.0, Modifiers::NONE),
            [ViewerAction::Step(-1)]
        );
        // A trackpad's stream: forty points are not a photo, a hundred are.
        assert_eq!(f.wheel(Point, -40.0, Modifiers::NONE), []);
        assert_eq!(f.wheel(Point, -40.0, Modifiers::NONE), []);
        assert_eq!(
            f.wheel(Point, -40.0, Modifiers::NONE),
            [ViewerAction::Step(1)]
        );
        // With Ctrl or Cmd the wheel is the zoom's, and steps nowhere.
        assert_eq!(f.wheel(Line, -1.0, Modifiers::CTRL), []);
        assert_eq!(f.wheel(Line, -1.0, Modifiers::COMMAND), []);
    }

    #[test]
    fn the_mouses_back_button_closes() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let back = |pressed| Event::PointerButton {
            pos: pos2(500.0, 300.0),
            button: PointerButton::Extra1,
            pressed,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(f.frame(vec![back(true)]), [ViewerAction::Close]);
        assert_eq!(f.frame(vec![back(false)]), []);
        let forward = Event::PointerButton {
            pos: pos2(500.0, 300.0),
            button: PointerButton::Extra2,
            pressed: true,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(f.frame(vec![forward]), [], "forward has nowhere to go");
    }

    // A photo shown is a still window: nothing in the viewer asks for a frame.
    #[test]
    fn a_photo_shown_asks_for_no_frame() {
        let mut f = Fixture::new().with_preview([1600, 800]).with_levels(&[
            [6000, 3000],
            [3000, 1500],
            [1500, 750],
        ]);
        f.photo_caption = Some("Lisbon, at dusk");
        for _ in 0..3 {
            f.frame(Vec::new());
        }
        for _ in 0..4 {
            f.frame(Vec::new());
            assert_eq!(f.repaint, std::time::Duration::MAX);
        }
    }

    // No control is a stop of the Tab key: one that had the keyboard would take the
    // arrows from the viewer.
    #[test]
    fn nothing_in_the_viewer_takes_the_keyboard() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        for _ in 0..4 {
            f.key(Key::Tab);
            assert_eq!(f.ctx.memory(|memory| memory.focused()), None);
        }
        assert_eq!(f.key(Key::ArrowRight), [ViewerAction::Step(1)]);
    }

    // Zoomed, the photo is its fitted rectangle that many times as large about the
    // middle of its area, and moved by the pan.
    #[test]
    fn a_zoomed_photo_is_drawn_larger_about_the_middle_and_moved_by_the_pan() {
        let mut f = Fixture::new().with_preview([1600, 800]);
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.frame(Vec::new());
        // Fitted it is 1000 by 500 at (0, 100): twice that about (500, 350).
        assert_eq!(
            f.photo().unwrap().1,
            Rect::from_min_size(pos2(-500.0, -150.0), vec2(2000.0, 1000.0))
        );
        f.zoom.pan_to(100.0, 50.0, (1000.0, 700.0));
        f.frame(Vec::new());
        assert_eq!(
            f.photo().unwrap().1,
            Rect::from_min_size(pos2(-400.0, -100.0), vec2(2000.0, 1000.0))
        );
    }

    // `+` and `=` are a quarter more, `-` back by the same, `0` the fit: about the
    // middle, so what is there stays there. A chord is not the viewer's, and a held key
    // goes on zooming.
    #[test]
    fn the_keys_zoom_about_the_middle_and_nought_fits() {
        let mut f = photo();
        // Wherever the pointer is: a key is not pressed at a place.
        f.point(pos2(700.0, 450.0));
        assert_eq!(f.key(Key::Plus), []);
        assert_eq!(f.zoomed(), (1.25, (0.0, 0.0)));
        f.key(Key::Equals);
        assert!(near(f.zoom.zoom(), 1.5625));
        // Shift with it, which is how `+` is typed on most keyboards.
        f.key_with(Key::Plus, Modifiers::SHIFT);
        assert!(near(f.zoom.zoom(), 1.953_125));
        f.key(Key::Minus);
        assert!(near(f.zoom.zoom(), 1.5625));
        // Panned, the middle stays the middle: the pan grows and shrinks with the zoom.
        f.zoom.pan_to(100.0, -40.0, (1000.0, 700.0));
        f.key(Key::Plus);
        let (_, pan) = f.zoomed();
        assert!(near(pan.0, 125.0) && near(pan.1, -50.0), "{pan:?}");
        f.key_with(Key::Plus, Modifiers::CTRL);
        f.key_with(Key::Minus, Modifiers::COMMAND);
        f.key_with(Key::Num0, Modifiers::ALT);
        assert!(near(f.zoom.zoom(), 1.953_125));
        f.key(Key::Num0);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
        // Held: every repeat is a quarter more, up to four times.
        let down = |repeat| Event::Key {
            key: Key::Plus,
            physical_key: Some(Key::Plus),
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![down(false)]);
        f.frame(vec![down(true)]);
        assert!(near(f.zoom.zoom(), 1.5625));
        for _ in 0..10 {
            f.frame(vec![down(true)]);
        }
        assert_eq!(f.zoom.zoom(), 4.0);
    }

    // With Ctrl or Cmd the wheel zooms, a fifth a notch, holding the photo under the
    // pointer still - and steps to no photo, then or after: what had been turned
    // towards the next one is not kept.
    #[test]
    fn the_wheel_with_ctrl_zooms_at_the_pointer_and_steps_nothing() {
        use egui::MouseWheelUnit::{Line, Point};
        let mut f = photo();
        // 200 right of the middle and 100 below it.
        f.point(pos2(700.0, 450.0));
        assert_eq!(f.wheel(Line, 1.0, Modifiers::CTRL), []);
        let by = 0.2_f32.exp();
        let (zoom, pan) = f.zoomed();
        assert!(near(zoom, by), "{zoom}");
        // The point of the photo under the pointer, (200, 100) from the middle at fit,
        // is drawn there still: pan + zoom * (200, 100) = (200, 100).
        assert!(
            near(pan.0 + zoom * 200.0, 200.0) && near(pan.1 + zoom * 100.0, 100.0),
            "{pan:?}"
        );
        // Towards the user, out again; Cmd is Ctrl's equal.
        f.wheel(Line, -1.0, Modifiers::COMMAND);
        assert!(near(f.zoom.zoom(), 1.0));
        // A wheel that reports pages is a notch a page.
        f.wheel(egui::MouseWheelUnit::Page, 1.0, Modifiers::CTRL);
        assert!(near(f.zoom.zoom(), by));
        f.wheel(egui::MouseWheelUnit::Page, -1.0, Modifiers::CTRL);
        assert!(f.zoom.is_fit());
        // A trackpad's points are taken as they come, and a flick is capped.
        f.wheel(Point, 10.0, Modifiers::CTRL);
        assert!(near(f.zoom.zoom(), 0.1_f32.exp()));
        f.wheel(Point, 900.0, Modifiers::CTRL);
        assert!(near(f.zoom.zoom(), 0.3_f32.exp()));
        f.key(Key::Num0);
        // Six tenths of a notch towards the next photo, a zoom, and six tenths more:
        // no step, where the two would have made one.
        assert_eq!(f.wheel(Point, -60.0, Modifiers::NONE), []);
        f.wheel(Line, 1.0, Modifiers::CTRL);
        assert_eq!(f.wheel(Point, -60.0, Modifiers::NONE), []);
    }

    // A pinch the system reports as one zooms by its own factor, at the pointer.
    #[test]
    fn a_pinch_zooms_by_its_own_factor_at_the_pointer() {
        let mut f = photo();
        f.point(pos2(700.0, 450.0));
        f.frame(vec![Event::Zoom(1.5)]);
        assert_eq!(f.zoomed(), (1.5, (-100.0, -50.0)));
        f.frame(vec![Event::Zoom(0.5)]);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
    }

    // A double click goes to the photo's own pixels, holding the place that was
    // clicked, and a second one back to the whole photo. There the picture is its full
    // level, on whole pixels.
    #[test]
    fn a_double_click_goes_to_the_photos_own_pixels_and_back() {
        let mut f = photo();
        let at = pos2(700.5, 450.25);
        f.click(at);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)), "one click is none");
        f.click(at);
        // 2,400 pixels drawn 1,000 points wide at fit, a pixel a point: 2.4 times.
        let (zoom, pan) = f.zoomed();
        assert_eq!(zoom, 2.4);
        assert!(
            near(pan.0, -200.5 * 1.4) && near(pan.1, -100.25 * 1.4),
            "{pan:?}"
        );
        f.frame(Vec::new());
        let (id, drawn) = f.photo().unwrap();
        assert_eq!(id, f.levels[0].0.id(), "its full level");
        assert_eq!(drawn.size(), vec2(2400.0, 1200.0));
        assert_eq!(
            (drawn.min.x.fract(), drawn.min.y.fract()),
            (0.0, 0.0),
            "{drawn:?}"
        );
        // Time passes, and a double click again: the fit.
        f.time += 1.0;
        f.click(at);
        assert_eq!(f.zoom.zoom(), 2.4);
        f.click(at);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
    }

    // Two clicks are a double click only close together, in time and in place. A third
    // right after a double click is the first of the next, not another.
    #[test]
    fn two_clicks_apart_are_no_double_click() {
        let mut f = photo();
        f.click(pos2(700.0, 450.0));
        f.time += 0.5;
        f.click(pos2(700.0, 450.0));
        assert!(f.zoom.is_fit(), "too late");
        f.time += 1.0;
        f.click(pos2(700.0, 450.0));
        f.click(pos2(300.0, 200.0));
        assert!(f.zoom.is_fit(), "elsewhere");
        f.time += 1.0;
        f.click(pos2(300.0, 200.0));
        f.click(pos2(300.0, 200.0));
        assert!(!f.zoom.is_fit());
        f.click(pos2(300.0, 200.0));
        assert!(!f.zoom.is_fit(), "the third click is the first of the next");
        f.click(pos2(300.0, 200.0));
        assert!(f.zoom.is_fit());
    }

    // "Its own pixels" are what the library says the photo has, whether or not its full
    // picture is made yet: a double click a moment after a step goes where one a second
    // later does. Until the library has said, the largest level there is to draw.
    #[test]
    fn a_double_click_goes_by_the_size_the_library_gives() {
        // Its preview alone on screen, 1,600 pixels of a photo of 2,400.
        let mut f = Fixture::new().with_preview([1600, 800]);
        f.width = 2400;
        f.frame(Vec::new());
        f.click(pos2(500.0, 350.0));
        f.click(pos2(500.0, 350.0));
        assert_eq!(f.zoom.zoom(), 2.4);
        // The library's number, not the level's: a photo whose full level the device
        // cannot hold is drawn from its half, and is 3,000 pixels wide all the same.
        let mut f = Fixture::new().with_levels(&[[1500, 700]]);
        f.width = 3000;
        f.frame(Vec::new());
        f.click(pos2(500.0, 350.0));
        f.click(pos2(500.0, 350.0));
        assert!(near(f.zoom.zoom(), 3.0), "{}", f.zoom.zoom());
    }

    // With nothing known of its size - the library has not answered, and there is only
    // the preview - and for a small photo, which has no pixels to spare over its fitted
    // size, a double click zooms all the same, to twice.
    #[test]
    fn a_double_click_zooms_a_photo_whose_pixels_are_not_known_or_spent() {
        let preview = Fixture::new().with_preview([1600, 800]);
        let small = Fixture::new().with_levels(&[[900, 400]]);
        for (mut f, what) in [(preview, "its preview alone"), (small, "a small photo")] {
            f.frame(Vec::new());
            f.click(pos2(500.0, 350.0));
            f.click(pos2(500.0, 350.0));
            assert_eq!(f.zoom.zoom(), 2.0, "{what}");
        }
        // On a screen of two pixels a point, half the zoom is the photo's own pixels:
        // 3,000 of them over 1,000 points.
        let mut f = Fixture::new().with_levels(&[[3000, 1500], [1500, 700]]);
        f.scale = 2.0;
        f.frame(Vec::new());
        f.click(pos2(500.0, 350.0));
        f.click(pos2(500.0, 350.0));
        assert!(near(f.zoom.zoom(), 1.5), "{}", f.zoom.zoom());
    }

    // A zoomed photo is moved by a drag with the first button, as far as it may go:
    // until its area's edge is at the window's.
    #[test]
    fn a_drag_pans_a_zoomed_photo_as_far_as_it_may_go() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.press(pos2(500.0, 350.0));
        // From the first point it moves: the photo follows the hand at once, not once
        // it has gone further than a click may.
        f.point(pos2(502.0, 351.0));
        assert_eq!(f.zoomed(), (2.0, (2.0, 1.0)));
        f.point(pos2(560.0, 380.0));
        assert_eq!(f.zoomed(), (2.0, (60.0, 30.0)));
        f.point(pos2(2000.0, -2000.0));
        assert_eq!(f.zoomed().1, (500.0, -350.0));
        f.release(pos2(2000.0, -2000.0));
        // Let go of, it follows nothing.
        f.point(pos2(100.0, 100.0));
        assert_eq!(f.zoomed().1, (500.0, -350.0));
        // And from where it is, the next drag: measured from its own beginning.
        f.press(pos2(100.0, 100.0));
        f.point(pos2(90.0, 120.0));
        assert_eq!(f.zoomed().1, (490.0, -330.0));
        f.release(pos2(90.0, 120.0));
        // The other buttons move nothing.
        let other = |pressed| Event::PointerButton {
            pos: pos2(90.0, 120.0),
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        f.frame(vec![other(true)]);
        f.point(pos2(300.0, 300.0));
        f.frame(vec![other(false)]);
        assert_eq!(f.zoomed().1, (490.0, -330.0));
    }

    // Fitted there is nothing to move; and a drag is no click, so the click after it is
    // the first of a double click and not the second.
    #[test]
    fn a_drag_moves_nothing_at_fit_and_is_no_click() {
        let mut f = photo();
        f.press(pos2(500.0, 350.0));
        f.point(pos2(600.0, 400.0));
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
        f.release(pos2(600.0, 400.0));
        f.click(pos2(600.0, 400.0));
        assert!(f.zoom.is_fit(), "a drag and a click are no double click");
        // Nor is a drag that came back to where it began before the button came up.
        f.time += 1.0;
        f.press(pos2(600.0, 400.0));
        f.point(pos2(700.0, 400.0));
        f.point(pos2(600.0, 400.0));
        f.release(pos2(600.0, 400.0));
        f.click(pos2(600.0, 400.0));
        assert!(f.zoom.is_fit(), "there and back is a drag");
        // A press that came up within a click's distance of where it went down is one.
        f.time += 1.0;
        f.press(pos2(600.0, 400.0));
        f.point(pos2(602.0, 401.0));
        f.release(pos2(602.0, 401.0));
        f.click(pos2(602.0, 401.0));
        assert!(!f.zoom.is_fit());
    }

    // A zoom made while a drag is held moves the drag along with the photo: measured
    // from where it began with the pan it had then, the pointer's next move would put
    // the pan back and undo the zoom's.
    #[test]
    fn a_zoom_during_a_drag_moves_the_drag_along() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.press(pos2(500.0, 350.0));
        f.point(pos2(580.0, 350.0));
        assert_eq!(f.zoomed().1, (80.0, 0.0));
        f.key(Key::Plus);
        assert_eq!(f.zoomed(), (2.5, (100.0, 0.0)));
        f.point(pos2(581.0, 350.0));
        assert_eq!(f.zoomed().1, (101.0, 0.0));
        // By the wheel and by a pinch as well.
        f.frame(vec![Event::Zoom(0.8)]);
        let (zoom, pan) = f.zoomed();
        f.point(pos2(582.0, 350.0));
        assert_eq!(f.zoomed(), (zoom, (pan.0 + 1.0, pan.1)));
        f.wheel(egui::MouseWheelUnit::Line, 1.0, Modifiers::CTRL);
        let (zoom, pan) = f.zoomed();
        f.point(pos2(583.0, 350.0));
        assert_eq!(f.zoomed(), (zoom, (pan.0 + 1.0, pan.1)));
    }

    // The fit under a press that is held - a step put the next photo back to it - and a
    // zoom after it: the last photo's pan is not brought to this one, and the press that
    // moved the last photo moves this one nowhere.
    #[test]
    fn a_press_held_across_a_photo_put_back_to_the_fit_is_no_drag_any_more() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.zoom.pan_to(100.0, 0.0, (1000.0, 700.0));
        f.press(pos2(500.0, 350.0));
        f.point(pos2(550.0, 350.0));
        assert_eq!(f.zoomed().1, (150.0, 0.0));
        f.zoom = Zoom::default();
        f.frame(Vec::new());
        f.key(Key::Plus);
        f.point(pos2(551.0, 350.0));
        assert_eq!(f.zoomed(), (1.25, (0.0, 0.0)));
        // The next press is a drag again.
        f.release(pos2(551.0, 350.0));
        f.press(pos2(551.0, 350.0));
        f.point(pos2(561.0, 350.0));
        assert_eq!(f.zoomed(), (1.25, (10.0, 0.0)));
    }

    // A press that went down on a fitted photo is no drag, and does not become one when
    // the photo is zoomed under it - by a key, or by the wheel under the same hand:
    // measured from where the button went down, the photo would be thrown by all the
    // pointer had moved while there was nothing to move.
    #[test]
    fn a_press_that_began_at_fit_moves_nothing_once_the_photo_is_zoomed() {
        let mut f = photo();
        f.press(pos2(300.0, 350.0));
        f.point(pos2(500.0, 350.0));
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
        for _ in 0..3 {
            f.key(Key::Plus);
        }
        let zoomed = f.zoomed();
        assert_eq!(zoomed.1, (0.0, 0.0));
        f.point(pos2(501.0, 350.0));
        assert_eq!(f.zoomed(), zoomed);

        let mut f = photo();
        f.press(pos2(300.0, 350.0));
        f.point(pos2(600.0, 450.0));
        f.wheel(egui::MouseWheelUnit::Line, 1.0, Modifiers::CTRL);
        f.wheel(egui::MouseWheelUnit::Line, 1.0, Modifiers::CTRL);
        let zoomed = f.zoomed();
        f.point(pos2(601.0, 450.0));
        assert_eq!(f.zoomed(), zoomed);
        // It was no click either: it moved.
        f.release(pos2(601.0, 450.0));
        f.click(pos2(601.0, 450.0));
        assert_eq!(f.zoomed().0, zoomed.0);
    }

    // A pan has the endings a rubber band has: the button comes up; the window is left,
    // where a button let go of may never be heard of; a touch is cancelled. After each
    // the pointer moves the photo no further.
    #[test]
    fn a_pan_ends_when_the_button_comes_up_the_window_is_left_or_a_touch_is_cancelled() {
        let cancelled = Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(0),
            phase: egui::TouchPhase::Cancel,
            pos: pos2(560.0, 350.0),
            force: None,
        };
        for ending in [
            primary(pos2(560.0, 350.0), false),
            Event::WindowFocused(false),
            cancelled,
        ] {
            let mut f = photo();
            f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
            f.press(pos2(500.0, 350.0));
            f.point(pos2(560.0, 350.0));
            assert_eq!(f.zoomed().1, (60.0, 0.0));
            f.frame(vec![ending.clone()]);
            f.point(pos2(800.0, 500.0));
            assert_eq!(f.zoomed().1, (60.0, 0.0), "{ending:?}");
            // The window come back to is no press either.
            f.frame(vec![Event::WindowFocused(true)]);
            f.point(pos2(810.0, 500.0));
            assert_eq!(f.zoomed().1, (60.0, 0.0), "{ending:?}");
        }
    }

    // A press on a control is the control's: no pan begins on it, and two quick presses
    // of one are not a double click on the photo.
    #[test]
    fn a_press_on_a_control_is_no_pan_and_no_half_of_a_double_click() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.frame(Vec::new());
        let star = f
            .images
            .iter()
            .map(|(_, at)| *at)
            .find(|at| at.center().y > 650.0 && at.center().x < 500.0)
            .expect("the star is drawn")
            .center();
        let places = [
            pos2(32.0, 350.0),  // the one before
            pos2(968.0, 350.0), // the one after
            pos2(972.0, 28.0),  // close
            star,
        ];
        for at in places {
            let mut f = photo();
            f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
            f.press(at);
            f.point(at + vec2(-90.0, -60.0));
            f.release(at + vec2(-90.0, -60.0));
            assert_eq!(f.zoomed(), (2.0, (0.0, 0.0)), "{at:?}");
            f.time += 1.0;
            f.click(at);
            f.click(at);
            assert_eq!(f.zoom.zoom(), 2.0, "{at:?}");
        }
    }

    // A press a few points beside a button is the photo's, not the button's: egui
    // hands a near miss to the nearest widget only where nothing is hit, and the photo
    // is there to be hit.
    #[test]
    fn a_press_just_beside_a_button_is_the_photos() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.frame(Vec::new());
        // Three points left of the button that closes, which begins at 956.
        let beside = pos2(953.0, 28.0);
        assert_eq!(f.click(beside), []);
        f.press(beside);
        f.point(beside + vec2(-40.0, 20.0));
        assert_eq!(f.zoomed().1, (-40.0, 20.0));
        f.release(beside + vec2(-40.0, 20.0));
        // On it, it closes.
        assert_eq!(f.click(pos2(957.0, 28.0)), [ViewerAction::Close]);
    }

    // The slider: a press anywhere along it zooms to there, a drag follows the pointer
    // to either end, and it says what it stands at. The pan stays where it may.
    #[test]
    fn the_slider_zooms_to_where_it_is_pressed_and_follows_a_drag() {
        let mut f = photo();
        assert!(f.drew("100%").is_some());
        // Its track: 120 points, ending 12 + 12 + 38 + 8 short of the window's edge.
        let (left, y) = (
            1000.0 - 12.0 - 12.0 - 38.0 - 8.0 - 120.0,
            700.0 - 12.0 - 19.0,
        );
        f.press(pos2(left + 60.0, y));
        assert_eq!(f.zoom.zoom(), 2.5);
        f.frame(Vec::new());
        assert!(f.drew("250%").is_some());
        // In steps of a twentieth.
        f.point(pos2(left + 61.0, y));
        assert!(near(f.zoom.zoom(), 2.55), "{}", f.zoom.zoom());
        f.point(pos2(left + 400.0, y - 300.0));
        assert_eq!(f.zoom.zoom(), 4.0);
        f.point(pos2(left - 400.0, y));
        assert_eq!(f.zoom.zoom(), 1.0);
        f.release(pos2(left - 400.0, y));
        // Let go of, it follows nothing; and none of it was a pan or a click on the
        // photo.
        f.point(pos2(left + 60.0, y));
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
        // Turned down, a pan that was within bounds is pulled in, not scaled.
        f.zoom.to(4.0, Pan::default(), (1000.0, 700.0));
        f.zoom.pan_to(600.0, 0.0, (1000.0, 700.0));
        f.press(pos2(left + 40.0, y));
        assert_eq!(f.zoomed(), (2.0, (500.0, 0.0)));
    }

    // A video's poster is not zoomed and has no slider; and where something is said in
    // the picture's place there is nothing to zoom, with the slider in its place and
    // taking no press.
    #[test]
    fn nothing_is_zoomed_for_a_video_or_where_there_is_no_picture() {
        let (left, y) = (
            1000.0 - 12.0 - 12.0 - 38.0 - 8.0 - 120.0,
            700.0 - 12.0 - 19.0,
        );
        for video in [true, false] {
            let mut f = photo();
            f.video = video;
            f.message = Some(if video {
                ("The native window does not play videos yet.", true)
            } else {
                ("This photo can't be shown.", false)
            });
            f.frame(Vec::new());
            assert_eq!(f.drew("100%").is_some(), !video);
            // Each way in, by itself: a double click would put back what another had
            // done.
            let fit = (1.0, (0.0, 0.0));
            f.key(Key::Plus);
            assert_eq!(f.zoomed(), fit, "a key, video: {video}");
            f.point(pos2(700.0, 450.0));
            f.wheel(egui::MouseWheelUnit::Line, 1.0, Modifiers::CTRL);
            assert_eq!(f.zoomed(), fit, "the wheel, video: {video}");
            f.frame(vec![Event::Zoom(1.5)]);
            assert_eq!(f.zoomed(), fit, "a pinch, video: {video}");
            f.click(pos2(700.0, 450.0));
            f.click(pos2(700.0, 450.0));
            assert_eq!(f.zoomed(), fit, "a double click, video: {video}");
            f.time += 1.0;
            f.press(pos2(left + 60.0, y));
            f.release(pos2(left + 60.0, y));
            assert_eq!(f.zoomed(), fit, "the slider, video: {video}");
        }
        // What is said over a picture that stays - nothing but a video's line today -
        // would not stop a photo's zoom.
        let mut f = photo();
        f.message = Some(("Over it.", true));
        f.key(Key::Plus);
        assert_eq!(f.zoom.zoom(), 1.25);
    }

    // The copy scaled for the window is the photo at fit and at no other size: zoomed,
    // it is the level that is at least as large as what is drawn.
    #[test]
    fn zoomed_the_photo_is_drawn_from_its_levels_and_not_from_the_fitted_copy() {
        let mut f = Fixture::new().with_levels(&[[6000, 3000], [3000, 1500], [1500, 750]]);
        f.fitted = Some(texture(&f.ctx, "fitted", [1000, 500]));
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.fitted.as_ref().unwrap().id());
        // A quarter more: 1,250 pixels wide, from the 1,500 level.
        f.key(Key::Plus);
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.levels[2].0.id());
        // Twice: 2,000 pixels, from the 3,000 level. Four times: 4,000, from the full.
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.levels[1].0.id());
        f.zoom.to(4.0, Pan::default(), (1000.0, 700.0));
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.levels[0].0.id());
        // Back at the fit, the copy again.
        f.key(Key::Num0);
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().0, f.fitted.as_ref().unwrap().id());
    }

    // Off the photo's own pixels nothing is put on whole pixels: a pan follows the hand
    // by fractions of a point.
    #[test]
    fn only_the_photos_own_pixels_are_put_on_whole_pixels() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.zoom.pan_to(10.25, 0.0, (1000.0, 700.0));
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().1.min, pos2(-489.75, -150.0));
        f.zoom.to(2.4, Pan::default(), (1000.0, 700.0));
        f.zoom.pan_to(10.25, 0.4, (1000.0, 700.0));
        f.frame(Vec::new());
        assert_eq!(f.photo().unwrap().1.min, pos2(-690.0, -250.0));
    }

    // A window made smaller pulls a pan back in: the photo is not left hanging off the
    // edge of its area.
    #[test]
    fn a_smaller_window_pulls_the_pan_back_in() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.zoom.pan_to(500.0, 350.0, (1000.0, 700.0));
        f.size = vec2(600.0, 500.0);
        f.frame(Vec::new());
        assert_eq!(f.zoomed().1, (300.0, 250.0));
    }

    // Over a photo that can be moved the pointer is a hand, closed while it holds the
    // photo; not at fit, and not over a control.
    #[test]
    fn the_pointer_is_a_hand_over_a_photo_that_can_be_moved() {
        let mut f = photo();
        f.point(pos2(500.0, 350.0));
        assert_eq!(f.cursor, CursorIcon::Default);
        f.key(Key::Plus);
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Grab);
        f.press(pos2(500.0, 350.0));
        assert_eq!(f.cursor, CursorIcon::Grabbing);
        f.release(pos2(500.0, 350.0));
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Grab);
        f.point(pos2(972.0, 28.0));
        f.frame(Vec::new());
        assert_ne!(f.cursor, CursorIcon::Grab);
    }

    // A zoomed photo is a still window too.
    #[test]
    fn a_zoomed_photo_asks_for_no_frame() {
        let mut f = photo();
        f.point(pos2(500.0, 350.0));
        f.key(Key::Plus);
        for _ in 0..3 {
            f.frame(Vec::new());
        }
        for _ in 0..4 {
            f.frame(Vec::new());
            assert_eq!(f.repaint, std::time::Duration::MAX);
        }
    }

    // As many notches of the wheel out as in end at the fit, to everything that asks:
    // the slider says a hundred, the pointer is no hand, and the next double click zooms
    // in. In single precision the factors' product is a hair over one, which was
    // "zoomed": that double click then went "back to the whole photo", and nothing
    // happened.
    #[test]
    fn as_many_notches_of_the_wheel_out_as_in_end_at_the_fit() {
        for notches in 1..=6 {
            let mut f = photo();
            f.point(pos2(700.0, 450.0));
            for _ in 0..notches {
                f.wheel(egui::MouseWheelUnit::Line, 1.0, Modifiers::CTRL);
            }
            for _ in 0..notches {
                f.wheel(egui::MouseWheelUnit::Line, -1.0, Modifiers::CTRL);
            }
            assert!(f.zoom.is_fit(), "{notches}: {}", f.zoom.zoom());
            f.frame(Vec::new());
            assert!(f.drew("100%").is_some());
            assert_eq!(f.cursor, CursorIcon::Default);
            f.click(pos2(700.0, 450.0));
            f.click(pos2(700.0, 450.0));
            assert_eq!(f.zoom.zoom(), 2.4, "after {notches} notches out and back");
        }
    }

    // A pinch whose factor is no number, nought, without end or below nought leaves
    // numbers within the range.
    #[test]
    fn a_pinch_that_is_no_number_leaves_numbers() {
        for factor in [f32::NAN, 0.0, f32::INFINITY, -1.0, f32::MIN_POSITIVE] {
            let mut f = photo();
            f.point(pos2(700.0, 450.0));
            f.frame(vec![Event::Zoom(1.7)]);
            f.frame(vec![Event::Zoom(factor)]);
            let (zoom, pan) = f.zoomed();
            assert!(
                zoom.is_finite() && pan.0.is_finite() && pan.1.is_finite(),
                "{factor}: {zoom} {pan:?}"
            );
            assert!((1.0..=4.0).contains(&zoom));
        }
    }

    // The photo's own caption is no control, and a press on it is not the photo's
    // either: a double click on a line of text does not zoom.
    #[test]
    fn a_double_click_on_the_photos_own_caption_zooms_nothing() {
        let mut f = photo();
        f.photo_caption = Some("Lisbon, at dusk");
        f.frame(Vec::new());
        let caption = f.drew("Lisbon, at dusk").expect("the caption").center();
        f.click(caption);
        f.click(caption);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
        // Beside it, it does.
        f.time += 1.0;
        f.click(caption + vec2(0.0, -80.0));
        f.click(caption + vec2(0.0, -80.0));
        assert_eq!(f.zoom.zoom(), 2.4);
    }

    // "Nothing zooms a video" is asked when the button comes up as well as when it goes
    // down: the second click of a double click, held across a step onto a video, would
    // zoom the poster - which has no slider, no key and no wheel to put it back with.
    #[test]
    fn a_click_that_comes_up_on_a_video_zooms_nothing() {
        let mut f = photo();
        let at = pos2(700.0, 450.0);
        f.click(at);
        f.press(at);
        // The step, as the application makes it: the next is a video, fitted.
        f.video = true;
        f.message = Some(("The native window does not play videos yet.", true));
        f.zoom = Zoom::default();
        f.release(at);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
        // Nor is the click before the step the first of a double click on what came
        // after it.
        f.video = false;
        f.message = None;
        f.click(at);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
    }

    // No hand where nothing is drawn that could be moved: a photo that was zoomed and
    // has gone keeps its zoom, and its place says so in words.
    #[test]
    fn the_pointer_is_no_hand_where_nothing_can_be_moved() {
        let mut f = photo();
        f.point(pos2(500.0, 300.0));
        f.key(Key::Plus);
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Grab);
        f.message = Some(("This photo is no longer available.", false));
        f.frame(Vec::new());
        f.frame(Vec::new());
        assert_eq!(f.photo(), None, "nothing is drawn");
        assert_ne!(f.cursor, CursorIcon::Grab);
    }

    // While the photo is held the hand stays closed wherever the pointer goes with it:
    // over a button the drag passes, the photo still follows.
    #[test]
    fn the_hand_stays_closed_over_a_control_while_the_photo_is_held() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.press(pos2(900.0, 100.0));
        assert_eq!(f.cursor, CursorIcon::Grabbing);
        f.point(pos2(972.0, 28.0));
        assert_eq!(f.zoomed().1, (72.0, -72.0), "the photo follows");
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Grabbing);
        // A press at fit holds nothing, and is no closed hand once the photo is zoomed.
        let mut f = photo();
        f.press(pos2(500.0, 350.0));
        f.key(Key::Plus);
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Grab);
    }

    // The slider is a control like the others - two quick presses of it are no double
    // click on the photo, and a drag along it moves no photo - and it is the first
    // button's alone.
    #[test]
    fn the_slider_is_a_control_and_the_first_buttons() {
        let (left, y) = (
            1000.0 - 12.0 - 12.0 - 38.0 - 8.0 - 120.0,
            700.0 - 12.0 - 19.0,
        );
        let mut f = photo();
        let at = pos2(left + 40.0, y);
        f.click(at);
        f.click(at);
        assert_eq!(f.zoomed(), (2.0, (0.0, 0.0)), "two presses of the slider");
        f.time += 1.0;
        f.press(at);
        f.point(at + vec2(20.0, 0.0));
        assert_eq!(f.zoomed(), (2.5, (0.0, 0.0)), "a drag along the slider");
        f.release(at + vec2(20.0, 0.0));
        for button in [PointerButton::Secondary, PointerButton::Middle] {
            let mut f = photo();
            let at = pos2(left + 60.0, y);
            let other = |pressed| Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            };
            f.frame(vec![Event::PointerMoved(at)]);
            f.frame(vec![other(true)]);
            f.frame(vec![other(false)]);
            assert_eq!(f.zoom.zoom(), 1.0, "{button:?}");
        }
    }

    // The slider's drag has the pan's endings: the window left, it follows the pointer
    // no further - a button let go of over another window may never be heard of - and
    // the next press is followed again.
    #[test]
    fn the_sliders_drag_ends_when_the_window_is_left() {
        let (left, y) = (
            1000.0 - 12.0 - 12.0 - 38.0 - 8.0 - 120.0,
            700.0 - 12.0 - 19.0,
        );
        let mut f = photo();
        f.press(pos2(left + 60.0, y));
        assert_eq!(f.zoom.zoom(), 2.5);
        f.frame(vec![Event::WindowFocused(false)]);
        f.frame(vec![Event::WindowFocused(true)]);
        f.point(pos2(left + 100.0, y - 300.0));
        assert_eq!(
            f.zoom.zoom(),
            2.5,
            "it follows a pointer that holds nothing"
        );
        f.release(pos2(left + 100.0, y - 300.0));
        f.press(pos2(left + 80.0, y));
        assert_eq!(f.zoom.zoom(), 3.0);
    }

    // The fitted copy is the photo at fit and at no zoom, also where a zoom happens to
    // be the copy's size: a hair over the fit, and a copy made for a window twice as
    // wide while this one's photo is zoomed to twice.
    #[test]
    fn zoomed_the_fitted_copy_is_not_drawn_whatever_its_size() {
        let mut f = Fixture::new().with_levels(&[[6000, 3000], [3000, 1500], [1500, 750]]);
        f.fitted = Some(texture(&f.ctx, "fitted", [1000, 500]));
        f.frame(Vec::new());
        f.zoom.to(1.001, Pan::default(), (1000.0, 700.0));
        f.frame(Vec::new());
        assert!(!f.zoom.is_fit());
        assert_ne!(f.photo().unwrap().0, f.fitted.as_ref().unwrap().id());

        let mut f = Fixture::new().with_levels(&[[6000, 3000], [3000, 1500], [1500, 750]]);
        f.fitted = Some(texture(&f.ctx, "fitted", [2000, 1000]));
        f.frame(Vec::new());
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        f.frame(Vec::new());
        assert_ne!(f.photo().unwrap().0, f.fitted.as_ref().unwrap().id());
    }

    // A press that began on a control and came up on the photo pans nothing, and is no
    // click on the photo.
    #[test]
    fn a_press_that_began_on_a_control_and_came_up_on_the_photo_is_nothing() {
        let mut f = photo();
        f.zoom.to(2.0, Pan::default(), (1000.0, 700.0));
        let (close, at) = (pos2(972.0, 28.0), pos2(500.0, 350.0));
        f.click(at);
        f.press(close);
        f.point(at);
        f.release(at);
        assert_eq!(f.zoomed(), (2.0, (0.0, 0.0)));
    }

    /// The room the open panel takes at the right of a window a thousand points wide:
    /// its 280, the 56 it stands from the edge and the gap of 12 beside it.
    const BESIDE_PANEL: f32 = 1000.0 - 348.0;

    // With the info panel open the photo is fitted beside it, not under it: its area is
    // what the panel leaves, and the button for the next photo stands at that area's
    // edge. Closed again, it is the window.
    #[test]
    fn the_photo_is_fitted_beside_the_open_panel() {
        let mut f = Fixture::new().with_preview([1600, 800]);
        f.info_open = true;
        f.frame(Vec::new());
        // 652 points wide and half as high, in the middle of the window's height.
        assert_eq!(
            f.photo().unwrap().1,
            Rect::from_min_size(
                pos2(0.0, 350.0 - BESIDE_PANEL / 4.0),
                vec2(BESIDE_PANEL, BESIDE_PANEL / 2.0)
            )
        );
        // The next photo's button, twelve points inside the area's edge.
        assert_eq!(
            f.click(pos2(BESIDE_PANEL - 12.0 - 20.0, 350.0)),
            [ViewerAction::Step(1)]
        );
        assert_eq!(f.click(pos2(968.0, 350.0)), [], "where it stands without");
        f.info_open = false;
        f.frame(Vec::new());
        assert_eq!(
            f.photo().unwrap().1,
            Rect::from_min_size(pos2(0.0, 100.0), vec2(1000.0, 500.0))
        );
        assert_eq!(f.click(pos2(968.0, 350.0)), [ViewerAction::Step(1)]);
    }

    // A zoomed photo stops at the panel: the picture is cut off at its area's edge, and
    // moved only as far as that area's edge - not the window's.
    #[test]
    fn a_zoomed_photo_stays_beside_the_panel() {
        let mut f = photo();
        f.info_open = true;
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        let (_, drawn) = f.photo().unwrap();
        assert!(drawn.right() > BESIDE_PANEL, "it is larger than its area");
        let at = (f.images.iter())
            .position(|(_, rect)| *rect == drawn)
            .unwrap();
        assert_eq!(
            f.clips[at],
            Rect::from_min_size(pos2(0.0, 0.0), vec2(BESIDE_PANEL, 700.0))
        );
        // Twice its area overflows it by half the area on each side.
        f.press(pos2(300.0, 350.0));
        f.point(pos2(3000.0, 350.0));
        assert_eq!(f.zoomed().1, (BESIDE_PANEL / 2.0, 0.0));
    }

    // The point a zoom holds still, and the place a double click zooms to, are measured
    // from the middle of the photo's area: from the window's, a double click beside the
    // open panel zoomed to a place half the panel's room away.
    #[test]
    fn a_zoom_beside_the_panel_is_measured_from_the_middle_of_the_photos_area() {
        let mut f = photo();
        f.info_open = true;
        f.frame(Vec::new());
        // A hundred points right of the area's middle, which is at 326.
        f.point(pos2(BESIDE_PANEL / 2.0 + 100.0, 350.0));
        f.wheel(egui::MouseWheelUnit::Line, 1.0, Modifiers::CTRL);
        let (zoom, pan) = f.zoomed();
        assert!(near(pan.0 + zoom * 100.0, 100.0), "{pan:?}");
        f.key(Key::Num0);
        // At the middle of the area exactly, a double click moves nothing aside; and
        // the photo's own pixels are measured against its width beside the panel.
        f.click(pos2(BESIDE_PANEL / 2.0, 350.0));
        f.click(pos2(BESIDE_PANEL / 2.0, 350.0));
        let (zoom, pan) = f.zoomed();
        assert!(near(zoom, 2400.0 / BESIDE_PANEL), "{zoom}");
        assert_eq!(pan, (0.0, 0.0));
    }

    // What is said in the picture's place is said in the middle of the photo's area, and
    // the photo's own caption is not drawn beside the open panel, which has it whole.
    #[test]
    fn beside_the_panel_the_message_is_in_the_photos_area_and_the_caption_is_the_panels() {
        let mut f = Fixture::new();
        f.info_open = true;
        f.message = Some(("This photo is no longer available.", false));
        f.photo_caption = Some("Lisbon, at dusk");
        f.frame(Vec::new());
        let said = f.drew("This photo is no longer available.").unwrap();
        assert!(
            (said.center().x - BESIDE_PANEL / 2.0).abs() < 1.0,
            "{said:?}"
        );
        assert_eq!(f.drew("Lisbon, at dusk"), None);
        f.info_open = false;
        f.frame(Vec::new());
        assert!(f.drew("Lisbon, at dusk").is_some());
    }

    // `I` opens and closes the info panel, and so does its button in the bar, beside the
    // star. Not at the key's repeat, and not as a chord.
    #[test]
    fn the_info_panel_is_asked_for_by_its_key_and_its_button() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::I), [ViewerAction::Info]);
        assert_eq!(f.key_with(Key::I, Modifiers::CTRL), []);
        let held = |repeat| Event::Key {
            key: Key::I,
            physical_key: Some(Key::I),
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(f.frame(vec![held(false)]), [ViewerAction::Info]);
        assert_eq!(f.frame(vec![held(true)]), []);
        // Its button is the second tool of the bar.
        f.frame(Vec::new());
        let tools: Vec<Rect> = (f.images.iter())
            .map(|(_, at)| *at)
            .filter(|at| at.center().y > 650.0 && at.center().x < 500.0)
            .collect();
        assert_eq!(tools.len(), 2, "the star and the panel's button");
        let button = tools
            .iter()
            .max_by(|a, b| a.center().x.total_cmp(&b.center().x))
            .unwrap();
        assert_eq!(f.click(button.center()), [ViewerAction::Info]);
    }

    // A step's answer takes a frame or two, and there is no caption to size the bar by
    // meanwhile: it keeps the width it had, so that its buttons stand still under the
    // pointer - and takes the new caption's when that has come.
    #[test]
    fn the_bar_keeps_its_width_while_the_caption_is_not_known() {
        let tools = |f: &Fixture| -> Vec<f32> {
            (f.images.iter())
                .map(|(_, at)| *at)
                .filter(|at| at.center().y > 650.0 && at.center().x < 500.0)
                .map(|at| at.center().x)
                .collect()
        };
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let with = tools(&f);
        assert_eq!(with.len(), 2);
        let caption = std::mem::take(&mut f.caption);
        f.frame(Vec::new());
        assert_eq!(tools(&f), with, "they stand where they stood");
        f.caption = "b.jpg".to_owned();
        f.frame(Vec::new());
        assert!(tools(&f)[0] > with[0], "a short caption is a narrow bar");
        f.caption = caption;
        f.frame(Vec::new());
        assert_eq!(tools(&f), with);
        // In a viewer just opened there is no width to keep: the tools alone.
        let mut f = Fixture::new();
        f.caption.clear();
        f.frame(Vec::new());
        assert!(tools(&f)[0] > with[0]);
    }

    /// The photo with the info panel open over what the library says of it.
    fn informed() -> Fixture {
        let mut f = photo();
        f.info_open = true;
        f.item = Some(crate::viewer_info_view::testing::full());
        f.frame(Vec::new());
        f
    }

    /// The panel's place in a window a thousand by seven hundred.
    fn panel() -> Rect {
        Rect::from_min_max(
            pos2(1000.0 - 56.0 - 280.0, 12.0),
            pos2(1000.0 - 56.0, 700.0 - 56.0),
        )
    }

    // The panel stands where the layout puts it, beside the photo's area, and what is
    // pressed in it is answered as the panel's.
    #[test]
    fn the_panel_stands_beside_the_photo_and_answers_what_is_pressed_in_it() {
        let mut f = informed();
        assert!(
            f.fills.iter().any(|(rect, _)| *rect == panel()),
            "{:?}",
            f.fills
        );
        let lens = f.drew("EF50mm f/1.8 STM").expect("the panel says the lens");
        assert!(panel().contains_rect(lens));
        assert_eq!(
            f.click(lens.center()),
            [ViewerAction::Panel(InfoAction::Search(
                "lens:\"EF50mm f/1.8 STM\"".to_owned()
            ))]
        );
        // Closed, there is no panel, and nothing of it to press.
        f.info_open = false;
        f.frame(Vec::new());
        assert_eq!(f.drew("EF50mm f/1.8 STM"), None);
        assert_eq!(f.click(lens.center()), []);
        // Open before the library has said anything of the photo: its ground alone.
        f.info_open = true;
        f.item = None;
        f.frame(Vec::new());
        assert!(f.fills.iter().any(|(rect, _)| *rect == panel()));
        assert_eq!(f.drew("EF50mm f/1.8 STM"), None);
    }

    // While a keyword is typed the keys are its field's: `h` is a letter and hides no
    // photo, an arrow moves the caret and steps nowhere, `+` zooms nothing, and Escape
    // leaves the field and does not close the viewer - the next one does.
    #[test]
    fn while_a_keyword_is_typed_the_viewers_keys_are_its_fields() {
        let mut f = informed();
        let field = f.drew("Add a keyword").expect("the field").center();
        f.click(field);
        assert!(InfoView::typing(&f.ctx));
        for key in [
            Key::H,
            Key::ArrowRight,
            Key::Home,
            Key::Period,
            Key::I,
            Key::Backspace,
        ] {
            assert_eq!(f.key(key), [], "{key:?}");
        }
        f.key(Key::Plus);
        assert!(f.zoom.is_fit());
        assert_eq!(f.key(Key::Escape), [], "it leaves the field");
        assert!(!InfoView::typing(&f.ctx));
        assert_eq!(f.key(Key::Escape), [ViewerAction::Close]);
        assert_eq!(f.key(Key::H), [ViewerAction::Hide]);
        // Enter in the field adds what was typed, as the panel's own answer.
        f.click(field);
        f.frame(vec![Event::Text("alps".to_owned())]);
        assert_eq!(
            f.key(Key::Enter),
            [ViewerAction::Panel(InfoAction::AddTag("alps".to_owned()))]
        );
    }

    // Over the panel the wheel is the panel's - it scrolls what the panel says, a
    // notch at a time - and steps to no photo, with or without Ctrl; over the photo it
    // steps as ever.
    #[test]
    fn the_wheel_over_the_panel_is_the_panels() {
        use egui::MouseWheelUnit::Line;
        let mut f = informed();
        f.size = vec2(1000.0, 400.0);
        f.frame(Vec::new());
        let top = f.drew("DATES").expect("in sight").top();
        f.point(pos2(800.0, 200.0));
        assert_eq!(f.wheel(Line, -1.0, Modifiers::NONE), []);
        f.frame(Vec::new());
        let moved = top - f.drew("DATES").expect("still in sight").top();
        assert!(moved > 10.0, "{moved}");
        assert_eq!(f.wheel(Line, 1.0, Modifiers::CTRL), []);
        assert!(f.zoom.is_fit(), "and zooms nothing");
        f.frame(Vec::new());
        assert_eq!(
            f.drew("DATES").unwrap().top(),
            top,
            "back by the same notch"
        );
        f.point(pos2(300.0, 200.0));
        assert_eq!(
            f.wheel(Line, -1.0, Modifiers::NONE),
            [ViewerAction::Step(1)]
        );
    }

    // A press on the panel is the panel's: no pan begins on it, and two quick presses
    // on it are no double click on the photo.
    #[test]
    fn a_press_on_the_panel_is_not_the_photos() {
        let mut f = informed();
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        let at = pos2(800.0, 600.0);
        f.press(at);
        f.point(at + vec2(-200.0, -100.0));
        f.release(at + vec2(-200.0, -100.0));
        assert_eq!(f.zoomed(), (2.0, (0.0, 0.0)));
        f.time += 1.0;
        f.click(at);
        f.click(at);
        assert_eq!(f.zoom.zoom(), 2.0);
        // In the black between the photo's area and the panel it is the photo's, as
        // anywhere that nothing else stands.
        let gap = pos2(BESIDE_PANEL + 6.0, 350.0);
        f.press(gap);
        f.point(gap + vec2(-30.0, 0.0));
        assert_eq!(f.zoomed().1, (-30.0, 0.0));
    }
}
