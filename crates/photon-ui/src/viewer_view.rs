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
    crop::ASPECTS,
    crop_tool::CropTool,
    crop_view::{self, CropData},
    icons::Icon,
    menu_view::asks_for_menu,
    playback::{Act, SEEK_STEP, Strip, clock},
    select_view::{self, SelectData, SelectView},
    text,
    theme::{
        apply::{color, menu_shadow},
        fonts,
        tokens::{DARK, GLASS, GLASS_LINE, PHOTO_LINE, R, S, SCRIM, SHADOW_INK, T},
    },
    viewer::wheel_step,
    viewer_faces::{Drawn, On, face_at, face_box, on},
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

/// A tool in the bar, and the room between two.
const TOOL: f32 = 30.0;
/// How far a notch of the wheel moves the info panel, in points.
const PANEL_NOTCH: f32 = 40.0;
const TOOL_GAP: f32 = 2.0;
/// The room between two groups of tools: a line a point wide, with air on both sides.
const GROUP_GAP: f32 = TOOL_GAP + S[0] + 1.0 + S[0] + TOOL_GAP;
/// How tall the line between two groups is.
const GROUP_LINE: f32 = 18.0;
/// The button that takes every turn and the crop off again.
pub const ORIGINAL: &str = "Original";
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
    /// Something done to the video on screen: by its strip, its keys, a click on it.
    Video(Act),
    /// One photo on (1) or back (-1).
    Step(i64),
    First,
    Last,
    Star,
    Hide,
    /// The info panel, opened or closed.
    Info,
    /// A quarter turn: to the right (true), or to the left.
    Turn(bool),
    /// Every turn and the crop taken off again.
    Original,
    /// The crop tool, opened.
    Crop,
    /// The slideshow, started - or stopped, while one runs.
    Slideshow,
    /// The slideshow, paused or played again.
    Pause,
    /// The crop tool's rectangle, to be written.
    CropApply,
    /// The crop tool, closed with nothing written.
    CropCancel,
    /// Something pressed in the info panel.
    Panel(InfoAction),
    /// The outline of an unnamed face was pressed: the person dialog, for this
    /// `detected_faces.id`.
    NameFace(i64),
    /// The other button went down on the photo: its menu, there, and what of the photo
    /// it went down on.
    Menu {
        at: Pos2,
        on: On,
    },
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
    /// Whether the photo can be turned and cropped: the library has said what it is,
    /// and it is a picture that can be shown - never a video, which has no such tools
    /// in its bar at all. Where it cannot, the tools are in their places and take no
    /// press, and their keys do nothing.
    pub editable: bool,
    /// Whether it has been turned or cropped: "Original" is offered.
    pub edited: bool,
    /// Whether the info panel is open: the photo is then fitted beside it, and its
    /// button in the bar is shown pressed.
    pub info_open: bool,
    /// What the panel says, once the library has said it of this photo.
    pub info: Option<InfoData<'a>>,
    /// The photo's faces, named and not, as the library says they are: outlined while
    /// the panel is open, and what the other button's press is asked of whether it is
    /// open or not.
    pub faces: &'a [Drawn<'a>],
    /// Something lies over the viewer that has the window for itself - a dialog, the
    /// viewer's own menu: the wheel and the buttons are not the viewer's. (The keys are
    /// taken out of the frame by whatever it is; these it does not take.)
    pub paused: bool,
    /// Whether a slideshow can be started: the library has said what the photo is.
    pub can_show: bool,
    /// The slideshow, while one runs.
    pub show: Option<Showing>,
    /// The photo the show has just left, drawn over the one arriving and faded out: what
    /// was drawn, where, and how much of it is still seen.
    pub outgoing: Option<(&'a TextureHandle, Rect, f32)>,
    /// A video being played: its strip, its keys, a click on its picture. None for a
    /// photo, and for a video that is not playing - not yet, or that cannot.
    pub strip: Option<Strip>,
}

/// A slideshow that is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Showing {
    pub playing: bool,
    /// The pointer has rested so long that the controls are out of the way: the bar,
    /// the slider, the buttons at the sides and the cross are not drawn, and the
    /// pointer itself is hidden.
    pub quiet: bool,
}

/// The crop tool, while it is open: the viewer draws the picture it is given under the
/// tool's rectangle, the tool's own bar in place of its own, and nothing else but the
/// button that closes.
pub struct Cropped<'a> {
    /// The rectangle, the ratio it is held to, the drag in progress: changed here.
    pub tool: &'a mut CropTool,
    /// The size in pixels of the picture the rectangle is drawn on: turned, and not
    /// cropped. Its box in the window is worked out from this, and not read off a
    /// texture: it is the same box whichever picture is on hand.
    pub size: (u32, u32),
    /// Where the picture on hand lies in that box, in fractions of it: all of it, or -
    /// for a photo that has a crop, until its picture without one is made - the part
    /// the crop keeps.
    pub part: Rect,
    /// The rectangle's write is on its way: the tool stands and takes nothing but
    /// "Cancel".
    pub busy: bool,
}

/// What the crop tool's dropdown is called, and its three buttons.
pub const CROP_RATIO: &str = "Crop ratio";
pub const WHOLE_PHOTO: &str = "Whole photo";
pub const CANCEL: &str = "Cancel";
pub const APPLY: &str = "Apply";

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
    /// The faces as this frame drew them, for the next frame's presses.
    faces: Vec<Aimed>,
    /// The window was left, or a touch cancelled, since the first button last went
    /// down: the slider follows the pointer no further, as a pan does not. A button let
    /// go of over another window may never be heard of.
    let_go: bool,
    /// The crop tool's dropdown.
    ratio: SelectView,
    /// When the crop tool was first drawn, while it is open; and whether a press made
    /// within a double click's time of that is still down - the second click of the
    /// double click on the bar's crop button, which lands on the tool's bar where the
    /// viewer's stood, and presses nothing there (as `DialogView::deaf`).
    crop_since: Option<f64>,
    crop_deaf: bool,
    /// Whether the photo has an edit, as last said: kept while nothing is known of the
    /// photo stepped to, so that "Original" does not leave the bar for the round trip and
    /// move the buttons after it - the jump `bar_place`'s `was` ends for the bar's width.
    edited: bool,
    /// The picture the last frame drew at fit, and where: what a slideshow fades out as
    /// it goes on.
    drawn: Option<(TextureHandle, Rect)>,
}

/// Where the controls stand: what a press on the photo is not.
struct Controls {
    prev: Rect,
    next: Rect,
    close: Rect,
    bar: Rect,
    /// None for a video, which has none.
    slider: Option<Rect>,
    /// A video's strip, while it plays.
    strip: Option<Rect>,
    /// The photo's own caption, where it has one: no control, but a press on it is not
    /// the photo's either - a double click on a line of text is not "closer".
    caption: Option<Rect>,
    /// The info panel, while it is open.
    panel: Option<Rect>,
    /// The faces as the last frame drew them: what a press in this one was aimed at.
    /// One that takes a press - an unnamed face, while the outlines show - is a
    /// control to the first button: a press on it names it, and is not the photo's.
    /// Every one of them is what the other button's press is asked of.
    faces: Vec<Aimed>,
}

/// A face where the last frame drew it - or would have, were the panel open.
#[derive(Clone, Copy, Debug)]
struct Aimed {
    at: Rect,
    /// What a menu opened on it is of.
    on: On,
    /// As much of it as takes a press, if any of it does.
    press: Option<Rect>,
}

impl Controls {
    /// Whether `at` is on something that lies over the photo: the other button's press
    /// there asks for no menu of the photo.
    fn over(&self, at: Pos2) -> bool {
        [self.prev, self.next, self.close, self.bar]
            .into_iter()
            .chain(self.slider)
            .chain(self.strip)
            .chain(self.caption)
            .chain(self.panel)
            .any(|rect| rect.contains(at))
    }

    /// Whether a press of the first button at `at` is a control's and not the photo's.
    fn hold(&self, at: Pos2) -> bool {
        self.over(at)
            || (self.faces.iter().filter_map(|face| face.press)).any(|part| part.contains(at))
    }
}

impl ViewerView {
    /// The picture the last frame drew at fit, and where: what a slideshow fades out as
    /// it goes on. None for a zoomed photo, and under the crop tool.
    pub fn drawn(&self) -> Option<(TextureHandle, Rect)> {
        self.drawn.clone()
    }

    /// Lets go of the picture kept for a fade: the viewer has shut, and a texture held
    /// here would be held for the rest of the session.
    pub fn let_go(&mut self) {
        self.drawn = None;
    }

    /// Whether a keyword is being typed into the info panel's field: the keys are the
    /// field's then.
    pub fn typing(&self) -> bool {
        self.info.typing()
    }

    /// Draws the viewer over the whole window and answers what was done in it. The zoom
    /// and the pan are `zoom`'s, which the keys, the wheel, a pinch, a double click, a
    /// drag and the slider change here.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &ViewerData<'_>,
        zoom: &mut Zoom,
        crop: Option<Cropped<'_>>,
    ) -> Vec<ViewerAction> {
        let window = ui.ctx().content_rect();
        let painter = ui.painter().clone();
        // The one colour in photon that no token holds: the ground a photo is judged
        // against is not a theme's to choose.
        painter.rect_filled(window, 0.0, Color32::BLACK);
        if let Some(crop) = crop {
            return self.cropping(ui, data, window, crop);
        }
        self.crop_since = None;
        if !data.caption.is_empty() {
            self.edited = data.edited;
        }
        let edited = self.edited;

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
        // A face's rectangle on a picture drawn at `at`: in the picture as it is drawn,
        // zoom and pan in it.
        let boxes = |at: Rect| -> Vec<Rect> {
            let image = Frame {
                left: at.left(),
                top: at.top(),
                width: at.width(),
                height: at.height(),
            };
            (data.faces.iter())
                .map(|face| {
                    let place = face_box(face.within, image);
                    Rect::from_min_size(
                        pos2(place.left, place.top),
                        vec2(place.width, place.height),
                    )
                })
                .collect()
        };
        // At rest in a slideshow the controls are away, and a press where they were is
        // the photo's.
        let quiet = data.show.is_some_and(|show| show.quiet);
        let away = |rect: Rect| if quiet { Rect::NOTHING } else { rect };
        let controls = Controls {
            // What a press in this frame was aimed at is what the last frame drew, not
            // what this one is about to: in the frame after the panel is toggled, the
            // window resized or another picture landed, the faces are elsewhere, and a
            // press asked of where they are going to be was on another face than the
            // one under the pointer.
            faces: std::mem::take(&mut self.faces),
            prev: away(side(window.left() + EDGE)),
            next: away(side(within.right() - EDGE - NAV)),
            close: away(Rect::from_min_size(
                pos2(window.right() - EDGE - CLOSE, window.top() + EDGE),
                vec2(CLOSE, CLOSE),
            )),
            bar: away(bar_place(ui, window, data, edited, self.bar)),
            slider: (!data.video && !quiet).then(|| slider_place(window)),
            strip: (data.strip.is_some() && !quiet).then(|| strip_place(window, area)),
            caption: caption.as_ref().map(|(place, _)| *place),
            panel,
        };
        if !quiet {
            self.bar = Some(controls.bar.width());
        }
        // The photo's own surface, under the controls: with something there to be hit,
        // egui hands no press that lands a few points beside a button to that button,
        // and what a press was is decided in one place, by where it was.
        ui.interact(window, Id::new("viewer-photo"), Sense::CLICK);
        let mut actions = self.input(ui, data, zoom, area, &controls, zooms);

        let drawn = shows.then(|| picture(ui, data, area, zoom)).flatten();
        self.drawn = (drawn.filter(|_| zoom.is_fit())).map(|(texture, at)| (texture.clone(), at));
        if let Some((texture, at)) = drawn {
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            (painter.with_clip_rect(within)).image(texture.id(), at, whole, Color32::WHITE);
        }
        // The photo the show has left, over the one arriving, as it fades.
        if let Some((texture, at, seen)) = data.outgoing {
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let tint = Color32::WHITE.gamma_multiply(seen.clamp(0.0, 1.0));
            (painter.with_clip_rect(within)).image(texture.id(), at, whole, tint);
        }
        // The faces, on the picture as this frame draws it; with no picture there is
        // nothing for one to be on. An unnamed face photon found takes a press while
        // the outlines show: as much of it as is in the photo's area.
        let boxes = drawn.map(|(_, at)| boxes(at)).unwrap_or_default();
        let pressable: Vec<(usize, i64, Rect)> = (data.faces.iter().zip(&boxes).enumerate())
            .filter(|_| data.info_open)
            .filter_map(|(number, (face, place))| match (face.key, face.face_id) {
                (None, Some(id)) => Some((number, id, place.intersect(within))),
                _ => None,
            })
            .filter(|(.., part)| part.is_positive())
            .collect();
        self.faces = (boxes.iter().enumerate())
            .map(|(number, at)| Aimed {
                at: *at,
                on: on(data.faces, Some(number)),
                press: (pressable.iter())
                    .find(|(of, ..)| *of == number)
                    .map(|(.., part)| *part),
            })
            .collect();
        // Drawn while the panel is open: over the photo, under everything else - made
        // before the buttons, the bar and the panel.
        if data.info_open && !boxes.is_empty() {
            let over = |at: Pos2| controls.over(at);
            let pressed = faces(ui, within, data.faces, &boxes, &pressable, over);
            // Not under a dialog or the menu, and not in the frame the menu is asked
            // for: on a Mac the first button with Control asks for it, and its click is
            // not the outline's as well.
            let menu = (actions.iter()).any(|action| matches!(action, ViewerAction::Menu { .. }));
            if !data.paused && !menu {
                actions.extend(pressed.map(ViewerAction::NameFace));
            }
        }
        if let Some(message) = data.message {
            said(ui, within, message);
        }

        if quiet {
            ui.ctx().set_cursor_icon(CursorIcon::None);
        }
        // The one before and the one after, at the window's sides.
        if !quiet
            && round(
                ui,
                controls.prev,
                "viewer-prev",
                Icon::ChevronLeft,
                "Previous photo",
                data.can_prev,
            )
        {
            actions.push(ViewerAction::Step(-1));
        }
        if !quiet
            && round(
                ui,
                controls.next,
                "viewer-next",
                Icon::ChevronRight,
                "Next photo",
                data.can_next,
            )
        {
            actions.push(ViewerAction::Step(1));
        }
        if !quiet
            && round(
                ui,
                controls.close,
                "viewer-close",
                Icon::X,
                "Close viewer",
                true,
            )
        {
            actions.push(ViewerAction::Close);
        }

        if let (Some(line), Some((place, galley))) = (data.photo_caption, caption) {
            photo_caption(ui, place, galley, line);
        }
        if !quiet {
            actions.extend(bar(ui, controls.bar, data, edited));
        }
        if let (Some(place), Some(shown)) = (controls.strip, data.strip.as_ref()) {
            let acts = self.strip(ui, place, shown);
            actions.extend(acts.into_iter().map(ViewerAction::Video));
        }
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

    /// The viewer under the crop tool: the picture turned and not cropped, fitted to
    /// the whole window, with the tool's rectangle over it and the tool's bar at the
    /// foot. The buttons to the photos on either side, the zoom's slider, the photo's
    /// caption, its faces and the info panel are not drawn; the button that closes is.
    ///
    /// **The keyboard is the tool's**: Enter applies, Escape cancels, and nothing else
    /// is answered - no step, no star, no hide - so that nothing leaves the photo under
    /// the rectangle. The wheel does nothing and the other button asks for no menu.
    fn cropping(
        &mut self,
        ui: &mut egui::Ui,
        data: &ViewerData<'_>,
        window: Rect,
        crop: Cropped<'_>,
    ) -> Vec<ViewerAction> {
        self.drawn = None;
        // What was held of the photo's own gestures is over.
        self.press = None;
        self.clicked = None;
        self.wheel = 0.0;
        self.faces.clear();
        let mut actions = Vec::new();
        // The surface under the controls, as ever: a press beside a button is not
        // handed to the button.
        ui.interact(window, Id::new("viewer-photo"), Sense::CLICK);

        let whole = Frame {
            left: window.left(),
            top: window.top(),
            width: window.width(),
            height: window.height(),
        };
        let fitted = contained(crop.size.0 as f32, crop.size.1 as f32, whole);
        let picture = Rect::from_min_size(
            pos2(fitted.left, fitted.top),
            vec2(fitted.width, fitted.height),
        );
        // The picture on hand, where it lies in that box.
        let at = Rect::from_min_max(
            picture.min + crop.part.min.to_vec2() * picture.size(),
            picture.min + crop.part.max.to_vec2() * picture.size(),
        );
        if let Some((texture, at)) = drawn(ui, data, at, true) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            (ui.painter().with_clip_rect(window)).image(texture.id(), at, uv, Color32::WHITE);
        }

        let close = Rect::from_min_size(
            pos2(window.right() - EDGE - CLOSE, window.top() + EDGE),
            vec2(CLOSE, CLOSE),
        );
        let bar = crop_bar_place(ui, window);
        crop_view::show(
            ui,
            crop.tool,
            &CropData {
                picture,
                over: &[bar, close],
                busy: crop.busy,
            },
        );
        if round(ui, close, "viewer-close", Icon::X, "Close viewer", true) {
            actions.push(ViewerAction::Close);
        }
        // The bar before the keys are read: its dropdown takes the ones it answers out
        // of the frame - the Enter that chose a ratio, the Escape that shut the list -
        // and those are not the tool's as well.
        let (now, pressed, released) = ui.input(|input| {
            (
                input.time,
                input.pointer.any_pressed(),
                input.pointer.any_released(),
            )
        });
        let double = ui
            .ctx()
            .options(|options| options.input_options.max_double_click_delay);
        let since = now - *self.crop_since.get_or_insert(now);
        if pressed && since < double {
            self.crop_deaf = true;
        }
        let deaf = self.crop_deaf;
        if released {
            self.crop_deaf = false;
        }
        let pressed = self.crop_bar(ui, bar, crop.tool, crop.busy, deaf);
        actions.extend(pressed);
        let events = ui.input(|input| input.events.clone());
        for event in &events {
            match event {
                Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if !(modifiers.command || modifiers.ctrl || modifiers.alt) => match key {
                    Key::Enter if !crop.busy => actions.push(ViewerAction::CropApply),
                    Key::Escape => actions.push(ViewerAction::CropCancel),
                    _ => {}
                },
                Event::PointerButton {
                    button: PointerButton::Extra1,
                    pressed: true,
                    ..
                } => actions.push(ViewerAction::Close),
                _ => {}
            }
        }
        actions
    }

    /// The crop tool's bar: the ratio the rectangle is held to, "Whole photo", "Cancel"
    /// and "Apply". The first two change the tool here; the others are answered. While
    /// the tool's write is on its way only "Cancel" works - it closes the tool, and the
    /// write lands all the same, as in the Svelte viewer - and while the second press of
    /// the double click that opened the tool is down (`deaf`), nothing does.
    fn crop_bar(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        tool: &mut CropTool,
        busy: bool,
        deaf: bool,
    ) -> Vec<ViewerAction> {
        glass(ui, rect, R[3]);
        let pad = (BAR - TOOL) / 2.0;
        let labels: Vec<&str> = ASPECTS.iter().map(|(label, _)| *label).collect();
        let mut pressed = Vec::new();
        let mut left = rect.left() + pad;
        let mut place = |width: f32| {
            let at = Rect::from_min_size(pos2(left, rect.top() + pad), vec2(width, TOOL));
            left += width + TOOL_GAP;
            at
        };
        let select = place(select_view::width(ui, &labels));
        let chosen = self.ratio.show(
            ui,
            select,
            &SelectData {
                id: "crop-ratio",
                label: CROP_RATIO,
                options: &labels,
                selected: Some(tool.aspect()),
                disabled: busy || deaf,
                hint: None,
                // The bar is at the foot of the window.
                above: true,
                // On the viewer's glass, which is dark in both themes.
                palette: Some(&DARK),
            },
        );
        if let Some(index) = chosen {
            tool.set_aspect(index);
        }
        if worded(
            ui,
            place(word_width(ui, WHOLE_PHOTO)),
            "crop-whole",
            WHOLE_PHOTO,
            !busy,
            false,
        ) && !deaf
        {
            tool.clear();
        }
        if worded(
            ui,
            place(word_width(ui, CANCEL)),
            "crop-cancel",
            CANCEL,
            true,
            false,
        ) && !deaf
        {
            pressed.push(ViewerAction::CropCancel);
        }
        if worded(
            ui,
            place(word_width(ui, APPLY)),
            "crop-apply",
            APPLY,
            !busy,
            true,
        ) && !deaf
        {
            pressed.push(ViewerAction::CropApply);
        }
        pressed
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
        // Under a dialog or the viewer's own menu nothing here is the viewer's: a press
        // held is over, and what the wheel had turned is not kept for after.
        if data.paused {
            self.press = None;
            self.wheel = 0.0;
            return actions;
        }
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
        // (A video that plays is clicked, to play and pause: its press stands.)
        if !zooms && data.strip.is_none() {
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
        let top = |at: Pos2| ui.ctx().layer_id_at(at) == Some(ui.layer_id());
        let on_photo = |at: Pos2| !controls.hold(at) && top(at);
        let mac = cfg!(target_os = "macos");
        // Whether the first button is down, on the photo or on anything else - the
        // slider, which follows it: the other button asks for no menu under it. Not
        // once the window has been left with the button down (`let_go`): its release
        // may never be heard of, egui goes on calling it down, and the menu would be
        // asked for in vain until the first button was pressed again.
        let first_down = ui.input(|input| input.pointer.primary_down());
        let within = Rect::from_min_size(pos2(area.left, area.top), vec2(area.width, area.height));
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
        let typing = self.info.typing();
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
                    // A video's keys (`video-player.svelte.ts`'s `handleKey`): Space, L, M
                    // and Shift with an arrow, five seconds - the plain arrows stay the
                    // viewer's. While its position bar is dragged the keyboard is the
                    // drag's: Escape calls it off, and nothing else leaves from under it.
                    if let Some(strip) = data.strip.as_ref().filter(|_| data.show.is_none()) {
                        if strip.scrubbing {
                            if *key == Key::Escape && !repeat {
                                actions.push(ViewerAction::Video(Act::ScrubAbandon));
                            }
                            continue;
                        }
                        let act = match key {
                            Key::ArrowLeft if modifiers.shift => Some(Act::SeekBy(-SEEK_STEP)),
                            Key::ArrowRight if modifiers.shift => Some(Act::SeekBy(SEEK_STEP)),
                            Key::Space if !repeat => Some(Act::Toggle),
                            Key::L if !repeat => Some(Act::Loop),
                            Key::M if !repeat => Some(Act::Mute),
                            _ => None,
                        };
                        if let Some(act) = act {
                            actions.push(ViewerAction::Video(act));
                            continue;
                        }
                    }
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
                        // Escape leaves a slideshow first and the viewer second, so the
                        // photo the show stopped on is still there to look at.
                        Key::Escape if !repeat && data.show.is_some() => {
                            Some(ViewerAction::Slideshow)
                        }
                        Key::Escape | Key::Backspace if !repeat => Some(ViewerAction::Close),
                        Key::S if !repeat && (data.can_show || data.show.is_some()) => {
                            Some(ViewerAction::Slideshow)
                        }
                        Key::Space if !repeat && data.show.is_some() => Some(ViewerAction::Pause),
                        Key::Period if !repeat && types_period => Some(ViewerAction::Star),
                        Key::H if !repeat => Some(ViewerAction::Hide),
                        Key::I if !repeat => Some(ViewerAction::Info),
                        // A turn is a write and a render: at the key's press, not for
                        // as long as it is held. Shift turns the other way.
                        Key::R if !repeat && data.editable => {
                            Some(ViewerAction::Turn(!modifiers.shift))
                        }
                        Key::C if !repeat && data.editable => Some(ViewerAction::Crop),
                        _ => None,
                    });
                }
                // Over the info panel the wheel is the panel's, which is as tall as
                // what it has to say, and steps to no other photo. A notch is forty
                // points, whatever the system's: nothing in the panel is far away, and
                // by the system's lines - which the grid goes by - a Windows set to
                // scroll none a notch could not reach the panel's foot at all, the
                // panel having no other way to be scrolled.
                Event::MouseWheel { unit, delta, .. } if over_panel => {
                    self.info.turned(match unit {
                        egui::MouseWheelUnit::Line => delta.y * PANEL_NOTCH,
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
                // The other button asks for the photo's menu where it goes down, as on a
                // photo of the grid (`grid/view.rs`): on the photo or on a face of it,
                // under no control and nothing that lies over the viewer, and not while
                // the first button is down. Which face it went down on is asked of
                // every face, whether or not the panel shows their outlines.
                Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers,
                } if asks_for_menu(*button, *modifiers, mac) => {
                    let held = self.press.is_some() || (first_down && !self.let_go);
                    if !held && !controls.over(*pos) && top(*pos) {
                        // A face is under the press where it is in the photo's area:
                        // one that a zoom has pushed past the area's edge is not there
                        // to be pressed, in the black beside the panel.
                        let frames: Vec<Frame> = (controls.faces.iter())
                            .map(|face| Frame {
                                left: face.at.left(),
                                top: face.at.top(),
                                width: face.at.width(),
                                height: face.at.height(),
                            })
                            .collect();
                        let hit = (within.contains(*pos))
                            .then(|| face_at(pos.x, pos.y, &frames))
                            .flatten();
                        actions.push(ViewerAction::Menu {
                            at: *pos,
                            on: hit.map_or(On::Nowhere, |hit| controls.faces[hit].on),
                        });
                    }
                }
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
                    // A click on a video's picture plays it or pauses it.
                    if data.strip.is_some() {
                        actions.push(ViewerAction::Video(Act::Toggle));
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
/// the middle of the area and moved by the pan. None while there is no picture.
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
    drawn(ui, data, at, zoom.is_fit())
}

/// The texture to draw at `at`, and where exactly: the copy made for exactly this size
/// where the photo is `fit` and there is one; else the level that is at least as large
/// as what is drawn; else the preview. None while there is none of them.
fn drawn<'a>(
    ui: &egui::Ui,
    data: &ViewerData<'a>,
    at: Rect,
    fit: bool,
) -> Option<(&'a TextureHandle, Rect)> {
    if data.levels.is_empty() {
        return data.preview.map(|preview| (preview, at));
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
    if fit && let Some(copy) = data.fitted {
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

/// The room a face's name has round it on its plate, sideways and up and down.
const PLATE_PAD: (f32, f32) = (6.0, 1.0);
/// What the outline of a face that can be named says it is for.
pub const NAME_FACE: &str = "Name this face";

/// The faces, over the photo: each an outline, a named one with its name on a plate
/// under it, cut off at the photo's area `within`. `pressable` are the unnamed ones
/// photon found, each with its number among `faces`, its id and the part of its outline
/// that takes a press; `over` says whether a point is on a control that lies over the
/// photo. The id of the one that was clicked is answered.
///
/// The line, its ink and the plate's ground are the photo's own colours in both themes,
/// as the photo's ground is: they are drawn on the photo, which no theme changes.
fn faces(
    ui: &egui::Ui,
    within: Rect,
    faces: &[Drawn<'_>],
    boxes: &[Rect],
    pressable: &[(usize, i64, Rect)],
    over: impl Fn(Pos2) -> bool,
) -> Option<i64> {
    // An unnamed face photon found is a button. No stop of the Tab key, like every
    // control of the viewer; and named by its number among the faces, not by its id:
    // two outlines can be of one detection, and under one name both were one widget,
    // at the first one's place.
    let buttons: Vec<egui::Response> = (pressable.iter())
        .map(|(number, _, part)| {
            let button = ui.interact(*part, Id::new(("viewer-face", *number)), Sense::CLICK);
            button.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, NAME_FACE));
            button
        })
        .collect();
    // Which of them a point is on is not egui's to say. egui gives a click to the last
    // widget made under it that takes one, whatever lies over it that takes none - the
    // bar's glass and its caption, a button that leads nowhere - and of two faces one
    // inside the other to the later, not the smaller. So: on no control, and of the
    // outlines that hold the point the smallest, as the menu has it. (In the photo's
    // area it is already: a button is as much of its outline as is in the area.)
    let which = |at: Pos2| -> Option<usize> {
        if over(at) {
            return None;
        }
        let frames: Vec<Frame> = (pressable.iter())
            .map(|(number, ..)| {
                let face = boxes[*number];
                Frame {
                    left: face.left(),
                    top: face.top(),
                    width: face.width(),
                    height: face.height(),
                }
            })
            .collect();
        face_at(at.x, at.y, &frames)
    };
    let pointer = ui.input(|input| input.pointer.latest_pos());
    let lit = (buttons.iter().any(egui::Response::hovered))
        .then(|| pointer.and_then(which))
        .flatten()
        .map(|hit| pressable[hit].0);
    if lit.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let named = (buttons.iter())
        .find(|button| button.clicked())
        .and_then(egui::Response::interact_pointer_pos)
        .and_then(which)
        .map(|hit| pressable[hit].1);

    let painter = ui.painter().with_clip_rect(within);
    for (number, (face, place)) in faces.iter().zip(boxes).enumerate() {
        let lit = lit == Some(number);
        let line = if lit { DARK.accent } else { PHOTO_LINE };
        painter.rect_stroke(
            *place,
            R[0],
            Stroke::new(1.0, color(SHADOW_INK)),
            StrokeKind::Outside,
        );
        painter.rect_stroke(
            *place,
            R[0],
            Stroke::new(2.0, color(line)),
            StrokeKind::Inside,
        );
        // The name, on a plate under the outline's left corner: as wide as the name
        // is, which is known once it is painted, so the plate's ground is put under it
        // afterwards. The whole name - the photo's area cuts a plate off, not an
        // ellipsis.
        let Some(name) = face.name else {
            continue;
        };
        let font = fonts::regular(T[1]);
        let tall = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font)) + 2.0 * PLATE_PAD.1;
        let ground = painter.add(egui::Shape::Noop);
        let wide = text::paint_line(
            ui,
            &painter,
            (place.left() + PLATE_PAD.0, place.bottom() + tall / 2.0),
            f32::INFINITY,
            name,
            font,
            color(DARK.text),
        ) + 2.0 * PLATE_PAD.0;
        let plate = Rect::from_min_size(place.left_bottom(), vec2(wide, tall));
        painter.set(ground, egui::Shape::rect_filled(plate, R[0], color(SCRIM)));
        // Told to whoever reads the window without seeing it: what of it is in sight.
        let seen = plate.intersect(within);
        if seen.is_positive() {
            ui.interact(seen, Id::new(("viewer-face-name", number)), Sense::hover())
                .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, name));
        }
    }
    named
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

/// A tool of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Star,
    TurnLeft,
    TurnRight,
    Crop,
    Original,
    Slideshow,
    Info,
}

/// The bar's tools from left to right, each with how far it stands from the first one's
/// left edge and how wide it is: the star; the turns, the crop and - while the photo has
/// an edit - "Original"; the info panel. A video has nothing to turn or crop, and no
/// tools for it.
fn tools(ui: &egui::Ui, data: &ViewerData<'_>, edited: bool) -> Vec<(Tool, f32, f32)> {
    let mut groups = vec![vec![Tool::Star]];
    if !data.video {
        let mut edit = vec![Tool::TurnLeft, Tool::TurnRight, Tool::Crop];
        edit.extend(edited.then_some(Tool::Original));
        groups.push(edit);
    }
    groups.push(vec![Tool::Slideshow, Tool::Info]);
    let mut placed = Vec::new();
    let mut left = 0.0;
    for (number, group) in groups.iter().enumerate() {
        if number > 0 {
            left += GROUP_GAP - TOOL_GAP;
        }
        for tool in group {
            let width = match tool {
                Tool::Original => word_width(ui, ORIGINAL),
                _ => TOOL,
            };
            placed.push((*tool, left, width));
            left += width + TOOL_GAP;
        }
    }
    placed
}

/// Where the bar at the foot of the window stands: as wide as its tools and its caption,
/// which is the part that gives way when the window is narrow. With no caption - the
/// library has not said yet what the photo stepped to is - as wide as it `was`: shrunk
/// to its tools for the frame or two a step's answer takes, the bar and the buttons in
/// it jumped twice at every step.
fn bar_place(
    ui: &egui::Ui,
    window: Rect,
    data: &ViewerData<'_>,
    edited: bool,
    was: Option<f32>,
) -> Rect {
    let wanted = ui
        .painter()
        .layout_no_wrap(
            data.caption.to_owned(),
            fonts::regular(T[1]),
            color(DARK.text_dim),
        )
        .size()
        .x;
    let pad = (BAR - TOOL) / 2.0;
    let tools = (tools(ui, data, edited).last()).map_or(0.0, |(_, left, width)| left + width);
    let most = (window.width() - 2.0 * BESIDE_BAR).max(tools + 2.0 * pad);
    let own = pad + tools + S[1] + wanted + S[2];
    let width = (was.filter(|_| data.caption.is_empty()).unwrap_or(own)).min(most);
    Rect::from_min_size(
        pos2(
            window.center().x - width / 2.0,
            window.bottom() - EDGE - BAR,
        ),
        vec2(width, BAR),
    )
}

/// Where the zoom's slider stands: in the corner beside the bar.
/// Where a video's strip stands: over the bar, in the middle of the photo's area and as
/// wide as it leaves room for between the buttons at its sides, up to 720 points - as
/// `VideoControls.svelte` stands, beside the info panel too.
fn strip_place(window: Rect, area: Frame) -> Rect {
    let room = area.width - 2.0 * (EDGE + NAV + S[2]);
    let width = room.clamp(0.0, STRIP_WIDE);
    let height = TOOL + 2.0 * S[1];
    Rect::from_min_size(
        pos2(
            area.left + (area.width - width) / 2.0,
            window.bottom() - EDGE - BAR - S[2] - height,
        ),
        vec2(width, height),
    )
}

/// The widest a video's strip stands.
const STRIP_WIDE: f32 = 720.0;
/// How wide its two times are, and its volume.
const STRIP_TIME: f32 = 44.0;
const STRIP_VOLUME: f32 = 80.0;

impl ViewerView {
    /// A video's strip: play and pause, the time, the position bar, the length, mute,
    /// the volume and the loop - `VideoControls.svelte`. No stop of the Tab key, as no
    /// control here is: every one has a key. The position bar is dragged by the first
    /// button for as long as it is held, and paused under it; Escape calls the drag off
    /// (`input`), and so does the window being left with the button down.
    fn strip(&mut self, ui: &egui::Ui, place: Rect, shown: &Strip) -> Vec<Act> {
        glass(ui, place, R[3]);
        let row = place.shrink2(vec2(S[2], S[1]));
        let mut acts = Vec::new();
        let square = |x: f32| Rect::from_min_size(pos2(x, row.top()), vec2(TOOL, TOOL));
        let (play_icon, play_label) = if shown.playing {
            (Icon::Pause, "Pause")
        } else {
            (Icon::Play, "Play")
        };
        if tool(
            ui,
            square(row.left()),
            "video-play",
            play_icon,
            play_label,
            true,
            false,
        ) {
            acts.push(Act::Toggle);
        }
        let painter = ui.painter();
        let time = |x: f32, words: String| {
            painter.text(
                pos2(x + STRIP_TIME / 2.0, row.center().y),
                egui::Align2::CENTER_CENTER,
                words,
                fonts::regular(T[1]),
                color(DARK.text_dim),
            );
        };
        time(row.left() + TOOL + S[2], clock(shown.time));
        // From the right: the loop, the volume, mute, an upright, the length.
        let looped = row.right() - TOOL;
        let volume = Rect::from_min_size(
            pos2(looped - S[2] - STRIP_VOLUME, row.top()),
            vec2(STRIP_VOLUME, TOOL),
        );
        let mute = volume.left() - S[2] - TOOL;
        let upright = mute - S[2];
        let length = upright - S[2] - STRIP_TIME;
        if tool(
            ui,
            square(looped),
            "video-loop",
            Icon::Repeat,
            "Loop",
            true,
            shown.prefs.looping,
        ) {
            acts.push(Act::Loop);
        }
        let (mute_icon, mute_label) = if shown.prefs.muted {
            (Icon::VolumeX, "Unmute")
        } else {
            (Icon::Volume2, "Mute")
        };
        if tool(
            ui,
            square(mute),
            "video-mute",
            mute_icon,
            mute_label,
            true,
            shown.prefs.muted,
        ) {
            acts.push(Act::Mute);
        }
        painter.line_segment(
            [
                pos2(upright, row.center().y - 9.0),
                pos2(upright, row.center().y + 9.0),
            ],
            Stroke::new(1.0, color(GLASS_LINE)),
        );
        time(
            length,
            shown.duration.map_or_else(|| "-:--".to_owned(), clock),
        );

        let primary = ui.input(|input| input.pointer.primary_down());
        // (A drag Escape called off is begun again by nothing while the button is still
        // down: egui ends a widget's drag at Escape, so the bar is no longer held.)
        let follows = primary && !self.let_go;
        let moved = ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO);
        let along = |track: Rect, at: Pos2| ((at.x - track.left()) / track.width()).clamp(0.0, 1.0);

        // The position bar: a line inside a reach the strip's whole height.
        let left = row.left() + TOOL + S[2] + STRIP_TIME + S[2];
        let reach = Rect::from_min_max(pos2(left, row.top()), pos2(length - S[2], row.bottom()));
        let track = Rect::from_center_size(reach.center(), vec2(reach.width(), 4.0));
        let known = shown.duration.is_some();
        let sense = if known {
            Sense::CLICK | Sense::DRAG
        } else {
            Sense::hover()
        };
        let response = ui.interact(reach, Id::new("video-seek"), sense);
        let fraction = shown
            .duration
            .map_or(0.0, |whole| (shown.time / whole).clamp(0.0, 1.0))
            as f32;
        response.widget_info(|| WidgetInfo::slider(known, f64::from(fraction), "Position"));
        let held = response.is_pointer_button_down_on() && follows;
        match (
            response.interact_pointer_pos().filter(|_| held),
            shown.scrubbing,
        ) {
            (Some(at), false) => acts.push(Act::ScrubBegin(f64::from(along(track, at)))),
            (Some(at), true) if moved => acts.push(Act::ScrubTo(f64::from(along(track, at)))),
            (Some(_), true) => {}
            // Let go of: where the drag left it. The window left with the button down:
            // called off, as Escape calls it off - the user did not choose where it
            // stopped.
            (None, true) if self.let_go => acts.push(Act::ScrubAbandon),
            (None, true) if !primary => acts.push(Act::ScrubEnd),
            (None, _) => {}
        }
        let dim = if known { 1.0 } else { 0.4 };
        painter.rect_filled(track, 2.0, color(GLASS_LINE).gamma_multiply(dim));
        painter.rect_filled(
            Rect::from_min_max(
                track.min,
                pos2(track.left() + fraction * track.width(), track.bottom()),
            ),
            2.0,
            color(DARK.accent),
        );

        // The volume: where it is pressed and dragged, it stands; muted, it shows none.
        let response = ui.interact(volume, Id::new("video-volume"), Sense::CLICK | Sense::DRAG);
        let level = if shown.prefs.muted {
            0.0
        } else {
            shown.prefs.volume as f32
        };
        response.widget_info(|| WidgetInfo::slider(true, f64::from(level), "Volume"));
        if response.is_pointer_button_down_on()
            && follows
            && let Some(at) = response.interact_pointer_pos()
        {
            let to = f64::from(along(volume, at));
            if (to - shown.prefs.volume).abs() > f64::EPSILON || shown.prefs.muted {
                acts.push(Act::Volume(to));
            }
        }
        let line = Rect::from_center_size(volume.center(), vec2(volume.width(), 4.0));
        painter.rect_filled(line, 2.0, color(GLASS_LINE));
        painter.rect_filled(
            Rect::from_min_max(
                line.min,
                pos2(line.left() + level * line.width(), line.bottom()),
            ),
            2.0,
            color(DARK.accent),
        );
        acts
    }
}

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

/// A tool of the bar that is an icon: lit while it is `on`, dimmed and deaf where it
/// does not work. Answers a press.
fn tool(
    ui: &egui::Ui,
    rect: Rect,
    id: &str,
    icon: Icon,
    label: &str,
    works: bool,
    on: bool,
) -> bool {
    let sense = if works { Sense::CLICK } else { Sense::hover() };
    let response = ui.interact(rect, Id::new(id), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, works, on, label));
    let lit = works && (on || response.hovered());
    if lit {
        ui.painter().rect_filled(rect, R[3], color(DARK.hover));
    }
    let tint = match (works, lit) {
        (false, _) => color(DARK.text_dim).gamma_multiply(0.4),
        (true, true) => color(DARK.text),
        (true, false) => color(DARK.text_dim),
    };
    icon.paint(ui, rect, 16.0, false, tint);
    response.clicked()
}

/// How wide a tool that is a word is.
fn word_width(ui: &egui::Ui, word: &str) -> f32 {
    let words =
        (ui.painter()).layout_no_wrap(word.to_owned(), fonts::regular(T[1]), color(DARK.text));
    (words.size().x + 2.0 * S[2]).ceil()
}

/// Where the crop tool's bar stands: in the viewer's own bar's place, as wide as its
/// dropdown and its three buttons.
fn crop_bar_place(ui: &egui::Ui, window: Rect) -> Rect {
    let pad = (BAR - TOOL) / 2.0;
    let labels: Vec<&str> = ASPECTS.iter().map(|(label, _)| *label).collect();
    let width = select_view::width(ui, &labels)
        + [WHOLE_PHOTO, CANCEL, APPLY]
            .iter()
            .map(|word| TOOL_GAP + word_width(ui, word))
            .sum::<f32>()
        + 2.0 * pad;
    Rect::from_min_size(
        pos2(
            window.center().x - width / 2.0,
            window.bottom() - EDGE - BAR,
        ),
        vec2(width, BAR),
    )
}

/// A tool of the bar that is a word: the `primary` one on the accent, one that does
/// not work dimmed and deaf. Answers a press.
fn worded(ui: &egui::Ui, rect: Rect, id: &str, word: &str, works: bool, primary: bool) -> bool {
    let sense = if works { Sense::CLICK } else { Sense::hover() };
    let response = ui.interact(rect, Id::new(id), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, works, word));
    let dim = if works { 1.0 } else { 0.4 };
    if primary {
        (ui.painter()).rect_filled(rect, R[3], color(DARK.accent).gamma_multiply(dim));
    } else if works && response.hovered() {
        ui.painter().rect_filled(rect, R[3], color(DARK.hover));
    }
    let ink = if primary { DARK.on_accent } else { DARK.text };
    let tint = color(ink).gamma_multiply(dim);
    let words = (ui.painter()).layout_no_wrap(word.to_owned(), fonts::regular(T[1]), tint);
    let at = rect.center() - words.size() / 2.0;
    ui.painter().galley(at, words, tint);
    response.clicked()
}

/// The bar at the foot of the window: its tools - the star, the turns and the crop, the
/// info panel - and the caption. Answers what was pressed in it.
fn bar(ui: &egui::Ui, rect: Rect, data: &ViewerData<'_>, edited: bool) -> Vec<ViewerAction> {
    let font = fonts::regular(T[1]);
    let pad = (BAR - TOOL) / 2.0;
    glass(ui, rect, R[3]);
    let mut pressed = Vec::new();
    let mut right = rect.left() + pad;
    for (tool_, left, width) in tools(ui, data, edited) {
        let place = Rect::from_min_size(rect.min + vec2(pad + left, pad), vec2(width, TOOL));
        // The line between two groups, in the middle of the room between them.
        if place.left() - right > TOOL_GAP {
            let middle = pos2((right + place.left()) / 2.0, rect.center().y);
            ui.painter().rect_filled(
                Rect::from_center_size(middle, vec2(1.0, GROUP_LINE)),
                0.0,
                color(GLASS_LINE),
            );
        }
        right = place.right();
        let edits = data.editable;
        match tool_ {
            Tool::Star => {
                let works = data.starred.is_some();
                let starred = data.starred == Some(true);
                let sense = if works { Sense::CLICK } else { Sense::hover() };
                let response = ui.interact(place, Id::new("viewer-star"), sense);
                let label = if starred { "Unstar" } else { "Star" };
                response.widget_info(|| {
                    WidgetInfo::selected(WidgetType::Button, works, starred, label)
                });
                if works && response.hovered() {
                    ui.painter().rect_filled(place, R[3], color(DARK.hover));
                }
                let tint = match (works, starred, response.hovered()) {
                    (false, _, _) => color(DARK.text_dim).gamma_multiply(0.4),
                    (true, true, _) => color(DARK.star),
                    (true, false, true) => color(DARK.text),
                    (true, false, false) => color(DARK.text_dim),
                };
                Icon::Star.paint(ui, place, 16.0, starred, tint);
                if response.clicked() {
                    pressed.push(ViewerAction::Star);
                }
            }
            Tool::TurnLeft => {
                let icon = Icon::RotateCcw;
                if tool(
                    ui,
                    place,
                    "viewer-turn-left",
                    icon,
                    "Rotate left",
                    edits,
                    false,
                ) {
                    pressed.push(ViewerAction::Turn(false));
                }
            }
            Tool::TurnRight => {
                let icon = Icon::RotateCw;
                if tool(
                    ui,
                    place,
                    "viewer-turn-right",
                    icon,
                    "Rotate right",
                    edits,
                    false,
                ) {
                    pressed.push(ViewerAction::Turn(true));
                }
            }
            Tool::Crop => {
                if tool(ui, place, "viewer-crop", Icon::Crop, "Crop", edits, false) {
                    pressed.push(ViewerAction::Crop);
                }
            }
            Tool::Original => {
                if worded(ui, place, "viewer-original", ORIGINAL, edits, false) {
                    pressed.push(ViewerAction::Original);
                }
            }
            // A show not running is started by it, and one that is is paused or played.
            Tool::Slideshow => {
                let (icon, label) = match data.show {
                    None => (Icon::Play, "Start slideshow"),
                    Some(Showing { playing: true, .. }) => (Icon::Pause, "Pause slideshow"),
                    Some(Showing { playing: false, .. }) => (Icon::Play, "Resume slideshow"),
                };
                let works = data.can_show || data.show.is_some();
                if tool(ui, place, "viewer-slideshow", icon, label, works, false) {
                    pressed.push(match data.show {
                        None => ViewerAction::Slideshow,
                        Some(_) => ViewerAction::Pause,
                    });
                }
            }
            // The info panel's button: lit while the panel is open.
            Tool::Info => {
                let label = "Photo information";
                if tool(
                    ui,
                    place,
                    "viewer-info",
                    Icon::Info,
                    label,
                    true,
                    data.info_open,
                ) {
                    pressed.push(ViewerAction::Info);
                }
            }
        }
    }

    let left = right + S[1];
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
        editable: bool,
        edited: bool,
        /// What the library has said of the photo, for the open panel.
        item: Option<photon_engine::commands::ViewerItem>,
        /// The photo's width in pixels as the library says it; nought until it has.
        width: u32,
        video: bool,
        /// The photo's faces, and whether something has the window for itself.
        faces: Vec<Drawn<'static>>,
        paused: bool,
        can_show: bool,
        show: Option<Showing>,
        /// The picture a show has left, where, and how much of it is seen.
        outgoing: Option<(TextureHandle, Rect, f32)>,
        /// A video's strip, while it plays.
        strip: Option<Strip>,
        /// The colour each texture of the last frame was painted with.
        tints: Vec<(egui::TextureId, Color32)>,
        /// The crop tool, while it is open: the size of the picture it is drawn on,
        /// where the picture on hand lies in it, and whether its write is on its way.
        crop: Option<(CropTool, (u32, u32), Rect, bool)>,
        zoom: Zoom,
        /// The textures painted in the last frame, each with where.
        images: Vec<(egui::TextureId, Rect)>,
        /// What each of them was cut off at.
        clips: Vec<Rect>,
        texts: Vec<(String, Rect)>,
        fills: Vec<(Rect, Color32)>,
        /// The rectangles drawn as a line: where, in what colour, and cut off at what.
        lines: Vec<(Rect, Color32, Rect)>,
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
                editable: true,
                edited: false,
                item: None,
                width: 0,
                video: false,
                faces: Vec::new(),
                paused: false,
                can_show: true,
                show: None,
                outgoing: None,
                strip: None,
                tints: Vec::new(),
                crop: None,
                zoom: Zoom::default(),
                images: Vec::new(),
                clips: Vec::new(),
                texts: Vec::new(),
                fills: Vec::new(),
                lines: Vec::new(),
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
                editable: self.editable,
                edited: self.edited,
                info: (self.item.as_ref()).map(|item| InfoData {
                    item,
                    albums: &[],
                    finds_faces: false,
                    looking: false,
                    zone: &zone,
                }),
                faces: &self.faces,
                paused: self.paused,
                can_show: self.can_show,
                show: self.show,
                outgoing: (self.outgoing.as_ref()).map(|(texture, at, seen)| (texture, *at, *seen)),
                strip: self.strip,
            };
            let (view, zoom) = (&mut self.view, &mut self.zoom);
            let mut crop = self.crop.as_mut().map(|(tool, size, part, busy)| Cropped {
                tool,
                size: *size,
                part: *part,
                busy: *busy,
            });
            let mut actions = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| actions = view.show(ui, &data, zoom, crop.take()));
            });
            full.textures_delta.clear();
            self.cursor = full.platform_output.cursor_icon;
            self.images.clear();
            self.tints.clear();
            self.clips.clear();
            self.texts.clear();
            self.fills.clear();
            self.lines.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => self
                        .texts
                        .push((text.galley.text().to_owned(), text.visual_bounding_rect())),
                    egui::Shape::Rect(rect) => match &rect.brush {
                        Some(brush) => {
                            self.tints.push((brush.fill_texture_id, rect.fill));
                            self.images.push((brush.fill_texture_id, rect.rect));
                            self.clips.push(clipped.clip_rect);
                        }
                        None => {
                            self.fills.push((rect.rect, rect.fill));
                            if rect.stroke.width > 0.0 {
                                let line = (rect.rect, rect.stroke.color, clipped.clip_rect);
                                self.lines.push(line);
                            }
                        }
                    },
                    egui::Shape::Mesh(mesh) => {
                        if let Some(vertex) = mesh.vertices.first() {
                            self.tints.push((mesh.texture_id, vertex.color));
                        }
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
        assert!((caption.center().x - 500.0).abs() < 120.0, "{caption:?}");
        assert!(caption.left() > star_at(&f).x + 5.0 * TOOL, "{caption:?}");
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

    /// A video playing, a second into its four, with its poster.
    fn video() -> Fixture {
        let mut f = photo();
        f.video = true;
        f.strip = Some(Strip {
            playing: true,
            time: 1.0,
            duration: Some(4.0),
            prefs: crate::playback::Prefs::default(),
            scrubbing: false,
        });
        f.frame(Vec::new());
        f
    }

    fn act(act: Act) -> ViewerAction {
        ViewerAction::Video(act)
    }

    // `video-player.svelte.ts`'s keys: Space, L, M, and Shift with an arrow - the plain
    // arrows stay the viewer's.
    #[test]
    fn a_videos_keys_play_seek_loop_and_mute() {
        let mut f = video();
        assert_eq!(f.key(Key::Space), [act(Act::Toggle)]);
        assert_eq!(f.key(Key::L), [act(Act::Loop)]);
        assert_eq!(f.key(Key::M), [act(Act::Mute)]);
        assert_eq!(
            f.key_with(Key::ArrowRight, Modifiers::SHIFT),
            [act(Act::SeekBy(SEEK_STEP))]
        );
        assert_eq!(
            f.key_with(Key::ArrowLeft, Modifiers::SHIFT),
            [act(Act::SeekBy(-SEEK_STEP))]
        );
        assert_eq!(f.key(Key::ArrowRight), [ViewerAction::Step(1)]);
        // A video that does not play has no keys of its own.
        f.strip = None;
        assert_eq!(f.key(Key::Space), []);
        assert_eq!(f.key(Key::M), []);
    }

    // While its bar is dragged the keyboard is the drag's: Escape calls it off, and
    // nothing leaves from under it.
    #[test]
    fn while_a_videos_bar_is_dragged_the_keys_are_the_drags() {
        let mut f = video();
        let time = f.drew("0:01").expect("the time");
        let bar = pos2(time.right() + S[2] + 40.0, time.center().y);
        f.press(bar);
        f.strip.as_mut().unwrap().scrubbing = true;
        assert_eq!(f.key(Key::ArrowRight), []);
        assert_eq!(f.key(Key::Space), []);
        assert_eq!(f.key(Key::H), []);
        assert_eq!(f.key(Key::Escape), [act(Act::ScrubAbandon)]);
        f.strip.as_mut().unwrap().scrubbing = false;
        // Called off, the drag is over while the button is still down: not begun again
        // under it, nor ended when it comes up.
        assert_eq!(
            f.frame(vec![Event::PointerMoved(bar + vec2(20.0, 0.0))]),
            []
        );
        assert_eq!(f.frame(vec![primary(bar, false)]), []);
        // A press after it is a drag again.
        f.point(bar);
        let again = f.frame(vec![primary(bar, true)]);
        assert!(
            matches!(again.as_slice(), [ViewerAction::Video(Act::ScrubBegin(_))]),
            "{again:?}"
        );
    }

    #[test]
    fn a_click_on_a_videos_picture_plays_or_pauses_it() {
        let mut f = video();
        assert_eq!(f.click(pos2(700.0, 300.0)), [act(Act::Toggle)]);
        // And two are two, not a double click's zoom.
        assert_eq!(f.click(pos2(700.0, 300.0)), [act(Act::Toggle)]);
        assert_eq!(f.zoomed(), (1.0, (0.0, 0.0)));
    }

    // The strip says the time and the length, and its button and its bar act: a press on
    // the bar begins a drag at that place, and is not the picture's click.
    #[test]
    fn a_videos_strip_says_its_times_and_its_controls_act() {
        let mut f = video();
        let time = f.drew("0:01").expect("the time");
        assert!(f.drew("0:04").is_some(), "the length");
        let play = pos2(time.left() - S[2] - TOOL / 2.0, time.center().y);
        assert_eq!(f.click(play), [act(Act::Toggle)]);
        let bar = pos2(time.right() + S[2] + 40.0, time.center().y);
        f.point(bar);
        let pressed = f.frame(vec![primary(bar, true)]);
        assert!(
            matches!(pressed.as_slice(), [ViewerAction::Video(Act::ScrubBegin(along))] if *along > 0.0 && *along < 0.5),
            "{pressed:?}"
        );
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
        // Its button is the last tool of the bar.
        f.frame(Vec::new());
        let tools: Vec<Rect> = (f.images.iter())
            .map(|(_, at)| *at)
            .filter(|at| at.center().y > 650.0 && at.center().x < 500.0)
            .collect();
        assert_eq!(
            tools.len(),
            6,
            "the star, the turns, the crop, the show, the panel's"
        );
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
        assert_eq!(with.len(), 6);
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
        assert!(f.view.typing());
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
        assert!(!f.view.typing());
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

    // A trackpad's points move the panel by themselves, and not by a notch each.
    #[test]
    fn a_trackpads_points_move_the_panel_by_themselves() {
        use egui::MouseWheelUnit::Point;
        let mut f = informed();
        f.size = vec2(1000.0, 400.0);
        f.frame(Vec::new());
        let top = f.drew("DATES").expect("in sight").top();
        f.point(pos2(800.0, 200.0));
        f.wheel(Point, -7.0, Modifiers::NONE);
        f.frame(Vec::new());
        assert_eq!(top - f.drew("DATES").expect("still in sight").top(), 7.0);
    }

    // ---- The faces ----

    /// Anna's face, which photon has under her plate; and a face photon found that
    /// nobody has named. Side by side in the middle of the photo.
    fn anna() -> Drawn<'static> {
        Drawn {
            key: Some("p:1"),
            name: Some("Anna"),
            face_id: Some(5),
            within: [0.25, 0.25, 0.5, 0.75],
        }
    }

    fn stranger() -> Drawn<'static> {
        Drawn {
            key: None,
            name: None,
            face_id: Some(9),
            within: [0.5, 0.25, 0.75, 0.75],
        }
    }

    /// The photo beside the open panel, with the two faces on it: fitted, it is 652
    /// points by 326, from (0, 187).
    fn with_faces() -> Fixture {
        let mut f = informed();
        f.faces = vec![anna(), stranger()];
        f.frame(Vec::new());
        f
    }

    const ANNA: Rect = Rect::from_min_max(pos2(163.0, 268.5), pos2(326.0, 431.5));
    const STRANGER: Rect = Rect::from_min_max(pos2(326.0, 268.5), pos2(489.0, 431.5));

    fn same(a: Rect, b: Rect) -> bool {
        near(a.left(), b.left())
            && near(a.top(), b.top())
            && near(a.right(), b.right())
            && near(a.bottom(), b.bottom())
    }

    /// The colour of the outline drawn at `place`, if one is: the line, which is two
    /// points wide, and not the ink round it.
    fn outline(f: &Fixture, place: Rect) -> Option<Color32> {
        (f.lines.iter())
            .find(|(at, tint, _)| same(*at, place) && *tint != color(SHADOW_INK))
            .map(|(_, tint, _)| *tint)
    }

    fn other(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    /// What a press of the other button at `at` answers.
    fn right_press(f: &mut Fixture, at: Pos2) -> Vec<ViewerAction> {
        f.point(at);
        let actions = f.frame(vec![other(at, true)]);
        f.frame(vec![other(at, false)]);
        actions
    }

    fn menu(at: Pos2, on: On) -> Vec<ViewerAction> {
        vec![ViewerAction::Menu { at, on }]
    }

    // While the panel is open every face is outlined where it is on the photo as
    // drawn, in the line that is the photo's own in both themes, with ink round it; a
    // named one has its name on a plate under its left corner. Cut off at the photo's
    // area. With the panel shut there is none of it.
    #[test]
    fn the_faces_are_outlined_while_the_panel_is_open_and_a_named_one_has_its_plate() {
        let mut f = with_faces();
        assert_eq!(outline(&f, ANNA), Some(color(PHOTO_LINE)));
        assert_eq!(outline(&f, STRANGER), Some(color(PHOTO_LINE)));
        let ink = (f.lines.iter())
            .filter(|(_, tint, _)| *tint == color(SHADOW_INK))
            .count();
        assert_eq!(ink, 2, "each outline has its ink");
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(BESIDE_PANEL, 700.0));
        for (at, _, clip) in &f.lines {
            if same(*at, ANNA) || same(*at, STRANGER) {
                assert_eq!(*clip, area, "cut off at the photo's area");
            }
        }
        // Her name, on a plate that begins at the outline's left bottom corner.
        let name = (f.texts.iter())
            .map(|(text, at)| (text.as_str(), *at))
            .find(|(text, at)| *text == "Anna" && at.left() < BESIDE_PANEL)
            .map(|(_, at)| at)
            .expect("her plate");
        assert!(name.top() >= ANNA.bottom() && name.left() >= ANNA.left() + PLATE_PAD.0 - 1.0);
        let plate = (f.fills.iter())
            .find(|(at, fill)| *fill == color(SCRIM) && at.min == ANNA.left_bottom())
            .map(|(at, _)| *at)
            .expect("the plate's ground");
        assert!(plate.contains_rect(name), "{plate:?} holds {name:?}");
        assert!(near(plate.right(), name.right() + PLATE_PAD.0) || plate.right() > name.right());

        f.info_open = false;
        f.frame(Vec::new());
        assert!(
            f.lines
                .iter()
                .all(|(_, tint, _)| *tint != color(PHOTO_LINE)),
            "no outline with the panel shut"
        );
        assert!(f.drew("Anna").is_none());
    }

    // An outline is on the photo as it is drawn: zoomed to twice and moved fifty points
    // to the left, the photo is 1,304 by 652 from (-376, 24), and so is the face on it.
    #[test]
    fn an_outline_is_on_the_photo_as_it_is_zoomed_and_moved() {
        let mut f = with_faces();
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.zoom.pan_to(-50.0, 0.0, (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        let moved = Rect::from_min_size(pos2(-50.0, 187.0), vec2(326.0, 326.0));
        assert_eq!(outline(&f, moved), Some(color(PHOTO_LINE)), "{:?}", f.lines);
        assert_eq!(outline(&f, ANNA), None);
    }

    // No picture, no outline: nothing is on screen for it to be on.
    #[test]
    fn nothing_is_outlined_where_no_picture_is_drawn() {
        let mut f = with_faces();
        f.message = Some(("This photo can't be shown.", false));
        f.frame(Vec::new());
        assert!(
            f.lines
                .iter()
                .all(|(_, tint, _)| *tint != color(PHOTO_LINE))
        );
        // And the other button's press is on no face.
        assert_eq!(
            right_press(&mut f, STRANGER.center()),
            menu(STRANGER.center(), On::Nowhere)
        );
    }

    // The outline of a face nobody has named is pressed to name it: lit under the
    // pointer, with the hand that says it is a button. The press is not the photo's -
    // two of them zoom nothing.
    #[test]
    fn a_click_on_an_unnamed_outline_names_that_face_and_is_not_the_photos() {
        let mut f = with_faces();
        f.point(STRANGER.center());
        f.frame(Vec::new());
        assert_eq!(outline(&f, STRANGER), Some(color(DARK.accent)));
        assert_eq!(f.cursor, CursorIcon::PointingHand);
        assert_eq!(outline(&f, ANNA), Some(color(PHOTO_LINE)));
        assert_eq!(f.click(STRANGER.center()), [ViewerAction::NameFace(9)]);
        assert_eq!(f.click(STRANGER.center()), [ViewerAction::NameFace(9)]);
        assert!(f.zoom.is_fit(), "no double click on the photo");
        // On a zoomed photo no pan begins on it either.
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        // (The stranger's outline is from (326, 187) now, 326 points wide.)
        let at = pos2(400.0, 350.0);
        f.press(at);
        f.point(at + vec2(-60.0, 0.0));
        f.release(at + vec2(-60.0, 0.0));
        assert_eq!(f.zoomed(), (2.0, (0.0, 0.0)));
    }

    // A named face's outline is no button, and neither is one that is Picasa's alone:
    // a press there is the photo's, and two of them are a double click.
    #[test]
    fn an_outline_that_names_nothing_takes_no_press() {
        for face in [
            anna(),
            Drawn {
                face_id: None,
                ..stranger()
            },
        ] {
            let mut f = informed();
            let place = if face.key.is_some() { ANNA } else { STRANGER };
            f.faces = vec![face];
            f.frame(Vec::new());
            f.point(place.center());
            f.frame(Vec::new());
            assert_eq!(outline(&f, place), Some(color(PHOTO_LINE)), "not lit");
            assert_eq!(f.click(place.center()), []);
            f.click(place.center());
            assert!(!f.zoom.is_fit(), "the photo's double click");
        }
    }

    // With the panel shut the faces are not drawn, and none takes a press: the photo is
    // then 1,000 points by 500 from (0, 100), and the stranger's face at (500, 225).
    #[test]
    fn with_the_panel_shut_no_face_takes_a_press() {
        let mut f = photo();
        f.faces = vec![anna(), stranger()];
        f.frame(Vec::new());
        let at = pos2(625.0, 350.0);
        assert_eq!(f.click(at), []);
        f.click(at);
        assert!(!f.zoom.is_fit(), "the photo's double click");
    }

    // The other button asks for the photo's menu where it goes down, and says what it
    // went down on: a face by what it is - whether or not the panel shows the outlines -
    // or the photo.
    #[test]
    fn the_other_button_asks_for_the_menu_of_what_it_went_down_on() {
        let mut f = with_faces();
        let (hers, his) = (ANNA.center(), STRANGER.center());
        assert_eq!(right_press(&mut f, his), menu(his, On::Unnamed(9)));
        assert_eq!(right_press(&mut f, hers), menu(hers, On::Person(1)));
        // On the photo beside the faces, and on the black over it.
        for at in [pos2(120.0, 350.0), pos2(120.0, 60.0)] {
            assert_eq!(right_press(&mut f, at), menu(at, On::Nowhere));
        }
        // With the panel shut the faces are where the photo is, fitted to the window.
        f.info_open = false;
        f.frame(Vec::new());
        let his = pos2(625.0, 350.0);
        assert_eq!(right_press(&mut f, his), menu(his, On::Unnamed(9)));
        let hers = pos2(375.0, 350.0);
        assert_eq!(right_press(&mut f, hers), menu(hers, On::Person(1)));
    }

    // Of two faces under the pointer, the one inside the other.
    #[test]
    fn of_a_face_inside_another_the_menu_is_the_smaller_ones() {
        let mut f = informed();
        f.faces = vec![
            Drawn {
                within: [0.1, 0.1, 0.9, 0.9],
                ..anna()
            },
            stranger(),
        ];
        f.frame(Vec::new());
        let at = STRANGER.center();
        assert_eq!(right_press(&mut f, at), menu(at, On::Unnamed(9)));
        let at = pos2(100.0, 350.0);
        assert_eq!(right_press(&mut f, at), menu(at, On::Person(1)));
    }

    // Not on what lies over the photo - the bar, a button, the panel - and not while
    // the first button holds the photo: its release would act under the menu.
    #[test]
    fn the_other_button_asks_for_nothing_on_a_control_or_under_a_held_press() {
        let mut f = with_faces();
        let bar = star_at(&f);
        assert_eq!(right_press(&mut f, bar), []);
        assert_eq!(right_press(&mut f, panel().center()), []);
        assert_eq!(right_press(&mut f, pos2(1000.0 - 30.0, 30.0)), [], "close");
        f.press(ANNA.center());
        assert_eq!(f.frame(vec![other(ANNA.center(), true)]), []);
        f.frame(vec![other(ANNA.center(), false)]);
        f.release(ANNA.center());
        assert_eq!(
            right_press(&mut f, ANNA.center()),
            menu(ANNA.center(), On::Person(1))
        );
    }

    // An outline that reaches under the bar is the bar's there, and one that a zoom
    // has pushed past the photo's area is pressed only in the area.
    #[test]
    fn an_outline_is_pressed_only_where_nothing_lies_over_it_and_inside_the_photos_area() {
        // A square photo beside the panel is 652 points from (0, 24), and reaches under
        // the bar at the foot of the window.
        let mut f = Fixture::new().with_preview([1000, 1000]);
        f.info_open = true;
        f.item = Some(crate::viewer_info_view::testing::full());
        f.faces = vec![Drawn {
            within: [0.3, 0.8, 0.95, 1.0],
            ..stranger()
        }];
        f.frame(Vec::new());
        let bar = star_at(&f);
        let face = Rect::from_min_max(pos2(195.6, 545.6), pos2(619.4, 676.0));
        assert!(
            face.contains(bar),
            "the test's bar {bar:?} lies over its face"
        );
        assert_eq!(f.click(bar), [ViewerAction::Star]);
        assert_eq!(f.click(pos2(250.0, 560.0)), [ViewerAction::NameFace(9)]);

        // Zoomed to twice and moved a hundred points right, the stranger's outline runs
        // from 426 to 752: past the area's edge at 652, the gap, and under the panel.
        let mut f = with_faces();
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.zoom.pan_to(100.0, 0.0, (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        // (Above the button for the next photo, which stands at the area's edge.)
        assert_eq!(f.click(pos2(600.0, 250.0)), [ViewerAction::NameFace(9)]);
        assert_eq!(f.click(pos2(BESIDE_PANEL + 6.0, 250.0)), [], "in the gap");
        assert_eq!(f.click(pos2(700.0, 500.0)), [], "on the panel");
    }

    // Under a dialog or the viewer's own menu the wheel and the buttons are not the
    // viewer's: nothing steps, no menu is asked for, no face is named, no photo moves.
    // And what the wheel had turned towards the next photo is not kept for after.
    #[test]
    fn paused_the_viewer_hears_neither_the_wheel_nor_a_button() {
        use egui::MouseWheelUnit::{Line, Point};
        let mut f = with_faces();
        f.paused = true;
        f.point(pos2(120.0, 350.0));
        assert_eq!(f.wheel(Line, -1.0, Modifiers::NONE), []);
        assert_eq!(right_press(&mut f, STRANGER.center()), []);
        assert_eq!(f.click(STRANGER.center()), []);
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        // (On the photo, clear of the button for the photo before.)
        let at = pos2(120.0, 350.0);
        f.press(at);
        f.point(at + vec2(60.0, 0.0));
        f.release(at + vec2(60.0, 0.0));
        assert_eq!(f.zoomed(), (2.0, (0.0, 0.0)));
        // A press that was held when it began is over.
        f.paused = false;
        f.press(at);
        f.paused = true;
        f.frame(Vec::new());
        f.paused = false;
        f.point(at + vec2(60.0, 0.0));
        assert_eq!(f.zoomed(), (2.0, (0.0, 0.0)));
        f.release(at + vec2(60.0, 0.0));

        // Six tenths of a notch, a pause, and six tenths more: no photo on.
        assert_eq!(f.wheel(Point, -60.0, Modifiers::NONE), []);
        f.paused = true;
        f.frame(Vec::new());
        f.paused = false;
        assert_eq!(f.wheel(Point, -60.0, Modifiers::NONE), []);
        assert_eq!(
            f.wheel(Point, -60.0, Modifiers::NONE),
            [ViewerAction::Step(1)]
        );
    }

    // A menu asked for in the frame an outline's click comes up in is asked for alone:
    // the menu and the dialog are not opened together. Both on the outline itself -
    // which is a Mac's press, where the first button with Control asks for the menu
    // and its click is the outline's.
    #[test]
    fn a_menu_asked_for_in_the_frame_an_outline_is_clicked_is_asked_for_alone() {
        let mut f = with_faces();
        let at = STRANGER.center();
        f.press(at);
        let actions = f.frame(vec![primary(at, false), other(at, true)]);
        assert_eq!(actions, menu(at, On::Unnamed(9)));
    }

    // The photo held and carried over an outline: the hand stays closed and the
    // outline is not lit - it is the photo that is in the hand, and letting go there
    // names nobody.
    #[test]
    fn an_outline_is_not_lit_under_a_photo_that_is_being_moved() {
        let mut f = with_faces();
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        // Zoomed, Anna's outline runs from 0 to 326 and the stranger's from 326 on.
        let from = pos2(200.0, 250.0);
        f.press(from);
        let over = pos2(420.0, 250.0);
        f.point(over);
        f.frame(Vec::new());
        assert_eq!(f.cursor, CursorIcon::Grabbing);
        assert!(
            f.lines
                .iter()
                .all(|(_, tint, _)| *tint != color(DARK.accent)),
            "no outline is lit"
        );
        assert_eq!(f.frame(vec![primary(over, false)]), []);
    }
    // ---- The faces, where a review found the edges ----

    /// A square photo: beside the panel it is 652 points from (0, 24) and reaches under
    /// the bar; a face nobody has named over its foot.
    fn square_photo() -> Fixture {
        let mut f = Fixture::new().with_preview([1000, 1000]);
        f.info_open = true;
        f.item = Some(crate::viewer_info_view::testing::full());
        f.faces = vec![Drawn {
            within: [0.3, 0.8, 0.95, 1.0],
            ..stranger()
        }];
        f.frame(Vec::new());
        f
    }

    // A click on the bar beside its buttons names no face under it. The bar's caption
    // is a widget that senses no click, and its glass is no widget at all: egui hands
    // the click to the outline beneath, and it is the view that says the bar was in the
    // way.
    #[test]
    fn a_click_on_the_bars_caption_over_an_outline_names_no_face() {
        let mut f = square_photo();
        let caption = (f.texts.iter())
            .find(|(text, _)| text.starts_with("a.jpg"))
            .map(|(_, at)| at.center())
            .expect("the bar's caption");
        let face = Rect::from_min_max(pos2(195.6, 545.6), pos2(619.4, 676.0));
        assert!(face.contains(caption), "{caption:?} is over the face");
        assert_eq!(f.click(caption), [], "a click on the bar's words");
    }

    // The same for a control that is in its place and takes no press: the button for the
    // next photo, on the last photo - where a hand that has clicked its way through a
    // folder clicks once more.
    #[test]
    fn a_click_on_a_dimmed_button_over_an_outline_names_no_face() {
        let mut f = with_faces();
        f.can_next = false;
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.zoom.pan_to(100.0, 0.0, (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        // The button for the next photo: 40 points, 12 from the area's edge at 652.
        let next = pos2(BESIDE_PANEL - 12.0 - 20.0, 350.0);
        assert_eq!(f.click(next), [], "a click on the dimmed button");
    }

    // And with the button working, to show the point is the button's: it steps.
    #[test]
    fn the_next_button_over_an_outline_steps() {
        let mut f = with_faces();
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.zoom.pan_to(100.0, 0.0, (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        let next = pos2(BESIDE_PANEL - 12.0 - 20.0, 350.0);
        assert_eq!(f.click(next), [ViewerAction::Step(1)]);
    }

    // The other button's press is on a face only where the face is in the photo's area,
    // as the first button's is: in the black between the area and the panel, where a
    // zoomed outline is cut off, the menu is the photo's and not that face's.
    #[test]
    fn the_other_button_in_the_gap_beside_the_panel_is_on_no_face() {
        let mut f = with_faces();
        f.zoom.to(2.0, Pan::default(), (BESIDE_PANEL, 700.0));
        f.zoom.pan_to(100.0, 0.0, (BESIDE_PANEL, 700.0));
        f.frame(Vec::new());
        let gap = pos2(BESIDE_PANEL + 6.0, 250.0);
        assert_eq!(right_press(&mut f, gap), menu(gap, On::Nowhere));
    }

    // No menu while the first button is down on anything: held on the slider, the
    // slider would go on following the pointer under the open menu.
    #[test]
    fn the_other_button_asks_for_nothing_while_the_first_holds_the_slider() {
        let mut f = with_faces();
        let place = slider_place(Rect::from_min_size(Pos2::ZERO, WINDOW));
        let on_slider = pos2(place.left() + 60.0, place.center().y);
        f.press(on_slider);
        assert!(!f.zoom.is_fit(), "the slider is held, and zoomed");
        let at = pos2(120.0, 60.0);
        assert_eq!(
            f.frame(vec![other(at, true)]),
            [],
            "no menu under a held button"
        );
    }

    // A press is asked of the faces as the last frame drew them. In the frame after the
    // panel is opened the picture is about to be drawn beside the panel, and was drawn
    // in the whole window: asked of where the faces are going to be, a press on Anna's
    // face was taken for the stranger's.
    #[test]
    fn a_press_in_the_frame_after_the_panel_opens_is_asked_of_what_was_drawn() {
        let mut f = photo();
        f.item = Some(crate::viewer_info_view::testing::full());
        f.faces = vec![anna(), stranger()];
        f.frame(Vec::new());
        // Fitted to the window the photo is 1,000 by 500 from (0, 100): Anna's face is
        // from 250 to 500, the stranger's from 500 to 750.
        let on_anna = pos2(400.0, 350.0);
        f.point(on_anna);
        // The panel opens (the application has acted on `i`), and the press arrives in
        // the same frame: it was made on what the last frame drew.
        f.info_open = true;
        let actions = f.frame(vec![other(on_anna, true)]);
        assert_eq!(actions, menu(on_anna, On::Person(1)));
    }

    // Two outlines Picasa drew over one detection carry the same id (`viewer_item` gives
    // each the detection beneath it), and each is pressed where it is: named by that id
    // they were two widgets under one name, of which egui kept one rectangle.
    #[test]
    fn two_outlines_of_one_detection_are_both_pressed() {
        let mut f = informed();
        f.faces = vec![
            Drawn {
                within: [0.05, 0.25, 0.25, 0.75],
                ..stranger()
            },
            stranger(),
        ];
        f.frame(Vec::new());
        // The first outline: from 32.6 to 163 of the photo's 652 points.
        assert_eq!(f.click(pos2(100.0, 350.0)), [ViewerAction::NameFace(9)]);
        // And the second, which is drawn like the first.
        assert_eq!(f.click(STRANGER.center()), [ViewerAction::NameFace(9)]);
    }

    // And the pointer on the one does not light the other.
    #[test]
    fn two_outlines_of_one_detection_are_not_lit_together() {
        let mut f = informed();
        f.faces = vec![
            Drawn {
                within: [0.05, 0.25, 0.25, 0.75],
                ..stranger()
            },
            stranger(),
        ];
        f.frame(Vec::new());
        f.point(pos2(100.0, 350.0));
        f.frame(Vec::new());
        assert_eq!(outline(&f, STRANGER), Some(color(PHOTO_LINE)));
    }
    // An outline is no stop of the Tab key, like every control of the viewer.
    // (`nothing_in_the_viewer_takes_the_keyboard` draws a viewer with no picture, no
    // panel and no face, and held the rule for no outline.)
    #[test]
    fn an_outline_is_no_stop_of_the_tab_key() {
        let mut f = with_faces();
        // (The stranger's: an outline's widget is named by its number among the faces.)
        let outline = Id::new(("viewer-face", 1_usize));
        for _ in 0..6 {
            f.key(Key::Tab);
            assert_ne!(f.ctx.memory(|memory| memory.focused()), Some(outline));
        }
    }
    // Of two outlines one inside the other a click names the smaller, as the menu is
    // the smaller one's (`face_at`): left to egui the click went to whichever was made
    // last, and a face inside a larger one that comes after it in the list could not be
    // pressed at all.
    #[test]
    fn a_click_on_a_face_inside_another_names_the_face_the_menu_is_of() {
        let mut f = informed();
        f.faces = vec![
            stranger(),
            Drawn {
                face_id: Some(7),
                within: [0.1, 0.1, 0.9, 0.9],
                ..stranger()
            },
        ];
        f.frame(Vec::new());
        let at = STRANGER.center();
        assert_eq!(right_press(&mut f, at), menu(at, On::Unnamed(9)));
        assert_eq!(f.click(at), [ViewerAction::NameFace(9)]);
    }
    // At a display scale of two, and at one and a half, the outlines are where they are
    // at one - they are placed in points - and are pressed there.
    #[test]
    fn the_outlines_are_in_points_at_any_display_scale() {
        for scale in [1.5, 2.0] {
            let mut f = informed();
            f.scale = scale;
            f.faces = vec![anna(), stranger()];
            f.frame(Vec::new());
            f.frame(Vec::new());
            assert_eq!(outline(&f, STRANGER), Some(color(PHOTO_LINE)), "{scale}");
            assert_eq!(f.click(STRANGER.center()), [ViewerAction::NameFace(9)]);
            let at = ANNA.center();
            assert_eq!(right_press(&mut f, at), menu(at, On::Person(1)));
        }
    }

    // The window left with the first button down on the slider: its release is never
    // heard of, and egui goes on calling the button down. The other button asks for the
    // menu all the same - the slider follows nothing any more.
    #[test]
    fn the_other_button_asks_again_once_the_window_was_left_under_a_held_button() {
        let mut f = with_faces();
        let slider = slider_place(Rect::from_min_size(Pos2::ZERO, WINDOW)).center();
        f.press(slider);
        let at = pos2(120.0, 350.0);
        assert_eq!(f.frame(vec![other(at, true)]), [], "held: no menu");
        f.frame(vec![other(at, false)]);
        f.frame(vec![Event::WindowFocused(false)]);
        f.frame(vec![Event::WindowFocused(true), Event::PointerMoved(at)]);
        assert_eq!(f.frame(vec![other(at, true)]), menu(at, On::Nowhere));
    }
    /// The bar's icons, from left to right: where each was drawn.
    fn bar_tools(f: &Fixture) -> Vec<Rect> {
        let mut tools: Vec<Rect> = (f.images.iter())
            .map(|(_, at)| *at)
            .filter(|at| at.center().y > 650.0 && at.width() < 30.0)
            .collect();
        tools.sort_by(|a, b| a.center().x.total_cmp(&b.center().x));
        tools
    }

    // The star; the turns and the crop; the slideshow's and the panel's buttons: three
    // groups, a line between two, and each button answers what it is for.
    #[test]
    fn the_turns_and_the_crop_are_in_the_bar_between_the_star_and_the_panels_button() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let tools = bar_tools(&f);
        assert_eq!(tools.len(), 6);
        assert_eq!(f.click(tools[0].center()), [ViewerAction::Star]);
        assert_eq!(f.click(tools[1].center()), [ViewerAction::Turn(false)]);
        assert_eq!(f.click(tools[2].center()), [ViewerAction::Turn(true)]);
        assert_eq!(f.click(tools[3].center()), [ViewerAction::Crop]);
        assert_eq!(f.click(tools[4].center()), [ViewerAction::Slideshow]);
        assert_eq!(f.click(tools[5].center()), [ViewerAction::Info]);
        let step = |n: usize| tools[n + 1].center().x - tools[n].center().x;
        assert!(near(step(1), TOOL + TOOL_GAP) && near(step(2), TOOL + TOOL_GAP));
        assert!(near(step(4), TOOL + TOOL_GAP));
        assert!(near(step(0), TOOL + GROUP_GAP) && near(step(3), TOOL + GROUP_GAP));
        // The lines, each in the middle of the room between two groups.
        f.frame(Vec::new());
        let line = color(GLASS_LINE);
        let mut lines: Vec<f32> = (f.fills.iter())
            .filter(|(at, fill)| *fill == line && near(at.width(), 1.0))
            .map(|(at, _)| {
                assert!(near(at.height(), GROUP_LINE));
                at.center().x
            })
            .collect();
        lines.sort_by(f32::total_cmp);
        let between = |n: usize| (tools[n].center().x + tools[n + 1].center().x) / 2.0;
        assert_eq!(lines.len(), 2);
        // (An icon stands on whole pixels, the line where the bar's own edge puts it.)
        let about = |a: f32, b: f32| (a - b).abs() < 0.6;
        assert!(
            about(lines[0], between(0)) && about(lines[1], between(3)),
            "{lines:?} {} {}",
            between(0),
            between(3)
        );
    }

    // "Original" stands after the crop's button while the photo has an edit, and only
    // then: the panel's button moves along for it.
    #[test]
    fn original_is_offered_while_the_photo_has_an_edit() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.drew(ORIGINAL), None);
        let without = bar_tools(&f);
        f.edited = true;
        f.frame(Vec::new());
        let word = f.drew(ORIGINAL).expect("the button");
        let with = bar_tools(&f);
        assert!(with[3].right() < word.left() && word.right() < with[4].left());
        let moved = (with[4].center().x - with[3].center().x)
            - (without[4].center().x - without[3].center().x);
        assert!(moved > word.width() + TOOL_GAP, "{moved}");
        assert_eq!(f.click(word.center()), [ViewerAction::Original]);
    }

    // A photo that cannot be edited - nothing is known of it yet, it has left the view,
    // it cannot be shown - has the tools where they are and none of them does anything,
    // nor does a key of theirs. The other tools go on working.
    #[test]
    fn a_photo_that_cannot_be_edited_has_its_tools_in_place_and_they_do_nothing() {
        let mut f = Fixture::new();
        f.edited = true;
        f.frame(Vec::new());
        let places = bar_tools(&f);
        let word = f.drew(ORIGINAL).unwrap();
        f.editable = false;
        f.frame(Vec::new());
        assert_eq!(bar_tools(&f), places);
        assert_eq!(f.drew(ORIGINAL), Some(word));
        for at in [places[1], places[2], places[3], word] {
            assert_eq!(f.click(at.center()), [], "{at:?}");
        }
        assert_eq!(f.key(Key::R), []);
        assert_eq!(f.key_with(Key::R, Modifiers::SHIFT), []);
        assert_eq!(f.key(Key::C), []);
        assert_eq!(f.click(places[0].center()), [ViewerAction::Star]);
        assert_eq!(f.click(places[5].center()), [ViewerAction::Info]);
    }

    // A video is neither turned nor cropped, and its bar has no tools for it: the star,
    // and the slideshow's and the panel's buttons, with one line between.
    #[test]
    fn a_video_has_no_tools_to_turn_or_crop_it() {
        let mut f = Fixture::new();
        f.video = true;
        f.editable = false;
        f.edited = true;
        f.frame(Vec::new());
        let tools = bar_tools(&f);
        assert_eq!(tools.len(), 3);
        assert_eq!(f.drew(ORIGINAL), None);
        assert!(near(
            tools[1].center().x - tools[0].center().x,
            TOOL + GROUP_GAP
        ));
        assert_eq!(f.click(tools[2].center()), [ViewerAction::Info]);
    }

    // `R` turns to the right and Shift+R to the left; `C` opens the crop tool. Each at
    // its press and not for as long as it is held - a turn is a write and a render -
    // and not as a chord.
    #[test]
    fn the_keys_turn_and_open_the_crop_tool() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::R), [ViewerAction::Turn(true)]);
        assert_eq!(
            f.key_with(Key::R, Modifiers::SHIFT),
            [ViewerAction::Turn(false)]
        );
        assert_eq!(f.key(Key::C), [ViewerAction::Crop]);
        assert_eq!(f.key_with(Key::R, Modifiers::CTRL), []);
        assert_eq!(f.key_with(Key::C, Modifiers::CTRL), []);
        for key in [Key::R, Key::C] {
            let held = |repeat| Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat,
                modifiers: Modifiers::NONE,
            };
            assert_eq!(f.frame(vec![held(false)]).len(), 1);
            assert_eq!(f.frame(vec![held(true)]), [], "{key:?} held");
            f.frame(vec![Event::Key {
                key,
                physical_key: Some(key),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }
    }

    const ALL: Rect = Rect {
        min: pos2(0.0, 0.0),
        max: pos2(1.0, 1.0),
    };

    /// The photo of `photo()` under the crop tool, with no crop: twice as wide as tall,
    /// in a box of 1000 by 500 from (0, 100).
    fn cropping() -> Fixture {
        let mut f = photo();
        f.crop = Some((CropTool::begin(None, 2400, 1200), (2400, 1200), ALL, false));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        f
    }

    fn tool(f: &Fixture) -> CropTool {
        f.crop.as_ref().unwrap().0
    }

    // The rectangle's box is the picture turned and not cropped, fitted to the whole
    // window, by the size the tool is told - not by the texture on hand, which for a
    // photo with a crop is of the part the crop keeps, and is drawn in that part of the
    // box until the picture without the crop is made.
    #[test]
    fn under_the_tool_the_picture_on_hand_is_drawn_in_its_part_of_the_box() {
        let mut f = Fixture::new().with_levels(&[[1200, 1200]]);
        let part = Rect::from_min_max(pos2(0.25, 0.0), pos2(0.75, 1.0));
        let cropped = Some([16384, 0, 49151, 65535]);
        f.crop = Some((
            CropTool::begin(cropped, 2400, 1200),
            (2400, 1200),
            part,
            false,
        ));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        let (_, at) = f.photo().expect("the picture");
        assert!(
            same(
                at,
                Rect::from_min_max(pos2(250.0, 100.0), pos2(750.0, 600.0))
            ),
            "{at:?}"
        );
        // The dimming is over the box, not over the picture on hand: left of it too.
        let scrim = color(SCRIM);
        let dimmed = (f.fills.iter())
            .filter(|(_, fill)| *fill == scrim)
            .fold(Rect::NOTHING, |all, (at, _)| all.union(*at));
        assert!(
            near(dimmed.left(), 0.0) && near(dimmed.right(), 1000.0),
            "{dimmed:?}"
        );
        // The picture without its crop has landed: all of the box.
        let mut f = photo();
        f.crop = Some((
            CropTool::begin(cropped, 2400, 1200),
            (2400, 1200),
            ALL,
            false,
        ));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        let (_, at) = f.photo().unwrap();
        assert!(
            same(
                at,
                Rect::from_min_max(pos2(0.0, 100.0), pos2(1000.0, 600.0))
            ),
            "{at:?}"
        );
    }

    // The box is the whole window's, whatever the info panel was: the tool has the
    // window.
    #[test]
    fn under_the_tool_the_box_is_fitted_to_the_whole_window() {
        let mut f = cropping();
        f.info_open = true;
        f.frame(Vec::new());
        let (_, at) = f.photo().unwrap();
        assert!(
            same(
                at,
                Rect::from_min_max(pos2(0.0, 100.0), pos2(1000.0, 600.0))
            ),
            "{at:?}"
        );
        assert_eq!(f.drew("Info"), None, "and no panel is drawn");
    }

    // Under the tool the viewer's own controls are away - the buttons at the sides,
    // the zoom's slider, the bar with its caption, the photo's own caption, the faces -
    // and the tool's bar stands in the bar's place. The button that closes stays.
    #[test]
    fn under_the_tool_only_its_bar_and_the_button_that_closes_are_drawn() {
        let mut f = cropping();
        f.photo_caption = Some("Lisbon, at dusk");
        f.info_open = true;
        f.faces = vec![stranger()];
        f.frame(Vec::new());
        for words in [WHOLE_PHOTO, CANCEL, APPLY, "Free"] {
            let at = f.drew(words).unwrap_or_else(|| panic!("{words}"));
            assert!(at.top() > 700.0 - EDGE - BAR && at.bottom() < 700.0 - EDGE);
        }
        assert_eq!(f.drew(&f.caption.clone()), None);
        assert_eq!(f.drew("Lisbon, at dusk"), None);
        assert_eq!(f.drew("100%"), None, "the slider");
        assert_eq!(bar_tools(&f).len(), 1, "the dropdown's chevron and no tool");
        assert!(
            f.lines
                .iter()
                .all(|(_, line, _)| *line != color(DARK.accent))
        );
        let side = |left: f32| pos2(left + NAV / 2.0, 350.0);
        // (A press there is on the rectangle, and moves nothing: it is the whole
        // picture already.)
        assert_eq!(f.click(side(EDGE)), []);
        assert_eq!(f.click(side(1000.0 - EDGE - NAV)), []);
        let close = pos2(1000.0 - EDGE - CLOSE / 2.0, EDGE + CLOSE / 2.0);
        assert_eq!(f.click(close), [ViewerAction::Close]);
    }

    // The keyboard is the tool's: Enter applies and Escape cancels, each at its press;
    // no key of the viewer's is answered, the wheel steps to no photo and zooms
    // nothing, and the other button asks for no menu. The mouse's back button closes,
    // as the cross does.
    #[test]
    fn under_the_tool_the_keys_are_enter_and_escape_and_nothing_else() {
        let mut f = cropping();
        assert_eq!(f.key(Key::Enter), [ViewerAction::CropApply]);
        assert_eq!(f.key(Key::Escape), [ViewerAction::CropCancel]);
        for key in [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::Home,
            Key::End,
            Key::Backspace,
            Key::Period,
            Key::H,
            Key::I,
            Key::R,
            Key::C,
            Key::Plus,
        ] {
            assert_eq!(f.key(key), [], "{key:?}");
        }
        assert_eq!(f.zoomed().0, 1.0);
        assert_eq!(f.key_with(Key::Enter, Modifiers::CTRL), []);
        // (egui works out for itself whether a key is held: it has to be.)
        for key in [Key::Enter, Key::Escape] {
            let event = |pressed, repeat| Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat,
                modifiers: Modifiers::NONE,
            };
            assert_eq!(f.frame(vec![event(true, false)]).len(), 1);
            assert_eq!(f.frame(vec![event(true, true)]), [], "{key:?} held");
            f.frame(vec![event(false, false)]);
        }
        let line = egui::MouseWheelUnit::Line;
        assert_eq!(f.wheel(line, -3.0, Modifiers::NONE), []);
        assert_eq!(f.wheel(line, 3.0, Modifiers::CTRL), []);
        assert_eq!(f.zoomed().0, 1.0);
        assert_eq!(right_press(&mut f, pos2(500.0, 300.0)), []);
        let back = Event::PointerButton {
            pos: pos2(500.0, 300.0),
            button: PointerButton::Extra1,
            pressed: true,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(f.frame(vec![back]), [ViewerAction::Close]);
        // Its write on its way, Enter sends no second one; Escape still cancels.
        f.crop.as_mut().unwrap().3 = true;
        assert_eq!(f.key(Key::Enter), []);
        assert_eq!(f.key(Key::Escape), [ViewerAction::CropCancel]);
    }

    // "Cancel" and "Apply" are answered; "Whole photo" puts the rectangle back to the
    // whole picture, free, here. With the write on its way only "Cancel" does anything.
    #[test]
    fn the_tools_bar_cancels_applies_and_takes_the_whole_photo() {
        let mut f = photo();
        let cropped = Some([16384, 16384, 49151, 49151]);
        f.crop = Some((
            CropTool::begin(cropped, 2400, 1200),
            (2400, 1200),
            ALL,
            false,
        ));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        let at = |f: &Fixture, words: &str| f.drew(words).unwrap().center();
        let (whole, cancel, apply) = (at(&f, WHOLE_PHOTO), at(&f, CANCEL), at(&f, APPLY));
        assert_eq!(f.click(cancel), [ViewerAction::CropCancel]);
        assert_eq!(f.click(apply), [ViewerAction::CropApply]);
        // "Apply" stands on the accent.
        assert!(
            f.fills
                .iter()
                .any(|(rect, fill)| *fill == color(DARK.accent)
                    && rect.contains(apply)
                    && rect.height() == TOOL)
        );
        f.crop.as_mut().unwrap().3 = true;
        f.frame(Vec::new());
        assert_eq!(f.click(apply), []);
        assert_eq!(f.click(whole), []);
        assert_ne!(tool(&f).wire(), None, "the rectangle stands");
        assert_eq!(f.click(cancel), [ViewerAction::CropCancel]);
        f.crop.as_mut().unwrap().3 = false;
        f.frame(Vec::new());
        assert_eq!(f.click(whole), []);
        assert_eq!(tool(&f).wire(), None, "the whole picture, which is no crop");
    }

    // The ratio is chosen from the dropdown, whose list opens over the bar, in the dark
    // palette whatever the window's theme; and the keys the list answers are not the
    // tool's as well - the Escape that shuts the list does not cancel the crop, the
    // Enter that takes an option does not apply it.
    #[test]
    fn the_ratio_is_chosen_from_a_list_that_opens_upwards_and_keeps_its_keys() {
        let mut f = cropping();
        f.ctx.set_theme(egui::Theme::Light);
        f.frame(Vec::new());
        let control = f.drew("Free").unwrap().center();
        assert_eq!(f.click(control), []);
        (0..30).for_each(|_| {
            f.frame(Vec::new());
        });
        let square = (f.texts.iter())
            .find(|(words, _)| words == "1:1")
            .map(|(_, at)| *at)
            .expect("the list");
        assert!(
            square.bottom() < 700.0 - EDGE - BAR,
            "over the bar: {square:?}"
        );
        assert!(f.fills.iter().any(|(_, fill)| *fill == color(DARK.raised)));
        assert_eq!(f.click(square.center()), []);
        assert_eq!(tool(&f).aspect(), 2);
        let rect = tool(&f).rect();
        // A square of pixels on a picture twice as wide as tall: half as wide in
        // fractions as it is tall.
        assert!(
            (rect.width() * 2.0 - rect.height()).abs() < 1e-9,
            "{rect:?}"
        );
        // By the keys: the list opened, an option taken with Enter, the list shut with
        // Escape - and neither is the tool's.
        // (A frame first: the last one still drew the list, with its own "1:1".)
        f.frame(Vec::new());
        let control = f.drew("1:1").unwrap().center();
        assert_eq!(f.click(control), []);
        assert!(f.view.ratio.open());
        assert_eq!(f.key(Key::ArrowDown), []);
        assert_eq!(f.key(Key::Enter), [], "the list's Enter");
        assert_eq!(tool(&f).aspect(), 3);
        assert_eq!(f.key(Key::Enter), [], "which opens the list again");
        assert_eq!(f.key(Key::Escape), [], "the list's Escape");
        assert_eq!(tool(&f).aspect(), 3);
    }

    // A drag on the rectangle is the tool's; a press on the tool's bar or on the button
    // that closes is not, though the rectangle reaches under both.
    #[test]
    fn under_the_tool_a_drag_moves_the_rectangle_and_a_press_on_a_control_does_not() {
        let mut f = photo();
        let cropped = Some([16384, 16384, 49151, 49151]);
        f.crop = Some((
            CropTool::begin(cropped, 2400, 1200),
            (2400, 1200),
            ALL,
            false,
        ));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        // The handle on its right edge, a tenth of the picture to the right.
        f.press(pos2(750.0, 350.0));
        f.frame(vec![Event::PointerMoved(pos2(850.0, 350.0))]);
        f.release(pos2(850.0, 350.0));
        assert!(
            (tool(&f).rect().right - 0.85).abs() < 1e-3,
            "{:?}",
            tool(&f).rect()
        );
        // A picture as tall as the window: its lower half reaches under the bar, its
        // upper half under the button that closes.
        let mut f = Fixture::new().with_levels(&[[1000, 700]]);
        let lower = Some([0, 32768, 65535, 65535]);
        f.crop = Some((CropTool::begin(lower, 1000, 700), (1000, 700), ALL, false));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        let glass = color(GLASS);
        let bar = (f.fills.iter())
            .find(|(rect, fill)| *fill == glass && rect.height() == BAR)
            .map(|(rect, _)| *rect)
            .expect("the tool's bar");
        let before = tool(&f).rect();
        f.press(pos2(bar.left() + 1.0, bar.center().y));
        f.frame(vec![Event::PointerMoved(pos2(
            bar.left() + 1.0,
            bar.center().y - 100.0,
        ))]);
        assert_eq!(tool(&f).rect(), before);
        f.release(pos2(bar.left() + 1.0, bar.center().y - 100.0));
        let upper = Some([0, 0, 65535, 32768]);
        f.crop = Some((CropTool::begin(upper, 1000, 700), (1000, 700), ALL, false));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        let before = tool(&f).rect();
        let close = pos2(1000.0 - EDGE - CLOSE / 2.0, EDGE + CLOSE / 2.0);
        f.press(close);
        f.frame(vec![Event::PointerMoved(close + vec2(0.0, 100.0))]);
        assert_eq!(tool(&f).rect(), before);
    }

    // What was held of the photo's own gestures when the tool opened is over: a click
    // before it is no half of a double click after it, and the wheel's turn is not
    // kept.
    #[test]
    fn the_tool_ends_what_was_held_of_the_photos_own_gestures() {
        let mut f = photo();
        let at = pos2(500.0, 300.0);
        assert_eq!(f.click(at), []);
        f.wheel(egui::MouseWheelUnit::Point, -60.0, Modifiers::NONE);
        f.crop = Some((CropTool::begin(None, 2400, 1200), (2400, 1200), ALL, false));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        f.crop = None;
        f.click(at);
        assert!(f.zoom.is_fit(), "no double click");
        let more = f.wheel(egui::MouseWheelUnit::Point, -60.0, Modifiers::NONE);
        assert_eq!(more, [], "the turn began again");
    }

    // With its write on its way the tool takes no drag: the rectangle being stored is the
    // one on screen.
    #[test]
    fn under_a_tool_whose_write_is_on_its_way_a_drag_moves_nothing() {
        let mut f = photo();
        let cropped = Some([16384, 16384, 49151, 49151]);
        f.crop = Some((
            CropTool::begin(cropped, 2400, 1200),
            (2400, 1200),
            ALL,
            true,
        ));
        f.frame(Vec::new());
        // (Past the second press of a double click that opened it.)
        f.time += 0.5;
        let before = tool(&f).rect();
        f.press(pos2(750.0, 350.0));
        f.frame(vec![Event::PointerMoved(pos2(850.0, 350.0))]);
        f.release(pos2(850.0, 350.0));
        assert_eq!(tool(&f).rect(), before);
    }

    // A press in the frame after the tool closes is asked of what the last frame drew,
    // which was the tool, with no face on it: not of the faces outlined before it opened,
    // which stood beside the open panel.
    #[test]
    fn a_press_after_the_tool_is_asked_of_no_face_from_before_it() {
        let mut f = with_faces();
        f.crop = Some((CropTool::begin(None, 2400, 1200), (2400, 1200), ALL, false));
        f.frame(Vec::new());
        f.crop = None;
        f.info_open = false;
        let at = STRANGER.center();
        let actions = f.frame(vec![Event::PointerMoved(at), other(at, true)]);
        assert_eq!(actions, menu(at, On::Nowhere));
    }

    // A press held on the photo when the tool opened is over: its button coming up after
    // the tool has closed is no click, and no first half of a double click.
    #[test]
    fn a_press_held_when_the_tool_opened_is_no_click_after_it() {
        let mut f = photo();
        let at = pos2(500.0, 300.0);
        f.press(at);
        f.crop = Some((CropTool::begin(None, 2400, 1200), (2400, 1200), ALL, false));
        f.frame(Vec::new());
        f.crop = None;
        f.release(at);
        f.click(at);
        assert!(f.zoom.is_fit(), "no double click");
    }

    // The second press of the double click that opened the tool lands on its bar, and
    // presses nothing there; a press after a double click's time does.
    #[test]
    fn the_tools_bar_is_deaf_to_the_second_click_that_opened_it() {
        let mut f = photo();
        f.crop = Some((CropTool::begin(None, 2400, 1200), (2400, 1200), ALL, false));
        f.frame(Vec::new());
        let cancel = f.drew(CANCEL).unwrap().center();
        assert_eq!(f.click(cancel), []);
        f.time += 0.5;
        assert_eq!(f.click(cancel), [ViewerAction::CropCancel]);
        // A tool opened again is new: deaf again, its dropdown too.
        let tool = f.crop.take();
        f.frame(Vec::new());
        f.time += 1.0;
        f.crop = tool;
        f.frame(Vec::new());
        assert_eq!(f.click(cancel), []);
        f.click(f.drew("Free").unwrap().center());
        assert!(!f.view.ratio.open());
    }

    // At a step nothing is known of the photo stepped to, and "Original" stays where it
    // was for that round trip, so that the buttons after it do not move; once the library
    // has said the photo has no edit, it goes.
    #[test]
    fn original_stays_while_nothing_is_known_of_the_photo_stepped_to() {
        let mut f = Fixture::new();
        f.edited = true;
        f.frame(Vec::new());
        let places = bar_tools(&f);
        assert!(f.drew(ORIGINAL).is_some());
        f.edited = false;
        f.editable = false;
        let caption = std::mem::take(&mut f.caption);
        f.frame(Vec::new());
        assert_eq!(bar_tools(&f), places);
        assert!(f.drew(ORIGINAL).is_some());
        f.caption = caption;
        f.frame(Vec::new());
        assert_eq!(f.drew(ORIGINAL), None);
    }
    fn playing() -> Option<Showing> {
        Some(Showing {
            playing: true,
            quiet: false,
        })
    }

    // The bar's button starts a show; during one it pauses it and plays it again, and
    // says which it will do. It takes no press while nothing is known of the photo.
    #[test]
    fn the_slideshows_button_starts_a_show_and_pauses_and_plays_one() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        let button = bar_tools(&f)[4].center();
        assert_eq!(f.click(button), [ViewerAction::Slideshow]);
        f.show = playing();
        f.frame(Vec::new());
        assert_eq!(f.click(button), [ViewerAction::Pause]);
        f.show = Some(Showing {
            playing: false,
            quiet: false,
        });
        f.frame(Vec::new());
        assert_eq!(f.click(button), [ViewerAction::Pause]);
        f.show = None;
        f.can_show = false;
        f.frame(Vec::new());
        assert_eq!(f.click(button), []);
    }

    // `S` starts a show and stops it; Space pauses and plays one; Escape stops one first
    // and closes the viewer only after; Backspace closes either way. Each at its press.
    #[test]
    fn the_keys_of_a_show() {
        let mut f = Fixture::new();
        f.frame(Vec::new());
        assert_eq!(f.key(Key::S), [ViewerAction::Slideshow]);
        assert_eq!(f.key(Key::Space), []);
        assert_eq!(f.key(Key::Escape), [ViewerAction::Close]);
        f.can_show = false;
        assert_eq!(f.key(Key::S), []);
        f.show = playing();
        assert_eq!(f.key(Key::S), [ViewerAction::Slideshow]);
        assert_eq!(f.key(Key::Space), [ViewerAction::Pause]);
        assert_eq!(f.key(Key::Escape), [ViewerAction::Slideshow]);
        assert_eq!(f.key(Key::Backspace), [ViewerAction::Close]);
        for key in [Key::S, Key::Space] {
            let event = |pressed, repeat| Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat,
                modifiers: Modifiers::NONE,
            };
            assert_eq!(f.frame(vec![event(true, false)]).len(), 1);
            assert_eq!(f.frame(vec![event(true, true)]), [], "{key:?} held");
            f.frame(vec![event(false, false)]);
        }
    }

    // At rest in a show the controls are away and the pointer is hidden: the bar, the
    // slider, the buttons at the sides and the cross; a press where they were is the
    // photo's.
    #[test]
    fn at_rest_in_a_show_the_controls_are_away() {
        let mut f = photo();
        f.show = Some(Showing {
            playing: true,
            quiet: true,
        });
        f.frame(Vec::new());
        assert_eq!(f.drew(&f.caption.clone()), None);
        assert_eq!(f.drew("100%"), None);
        assert!(bar_tools(&f).is_empty());
        assert_eq!(f.cursor, CursorIcon::None);
        let side = pos2(EDGE + NAV / 2.0, 350.0);
        let close = pos2(1000.0 - EDGE - CLOSE / 2.0, EDGE + CLOSE / 2.0);
        assert!(
            !f.fills.iter().any(|(_, fill)| *fill == color(GLASS)),
            "no glass"
        );
        assert_eq!(f.click(side), []);
        // A double click where the cross was zooms: the photo's.
        f.click(close);
        f.click(close);
        assert!(!f.zoom.is_fit());
        f.key(Key::Num0);
        // Awake, they are back.
        f.show = playing();
        f.frame(Vec::new());
        assert!(f.drew(&f.caption.clone()).is_some());
        assert_eq!(f.click(close), [ViewerAction::Close]);
    }

    // The photo a show has left is drawn over the arriving one, in the photo's area, as
    // much of it as is still seen.
    #[test]
    fn the_photo_a_show_left_is_drawn_over_the_one_arriving() {
        let mut f = photo();
        let left = texture(&f.ctx, "left", [10, 5]);
        let at = Rect::from_min_max(pos2(0.0, 100.0), pos2(1000.0, 600.0));
        f.outgoing = Some((left.clone(), at, 0.25));
        f.frame(Vec::new());
        let order: Vec<egui::TextureId> = f.images.iter().map(|(id, _)| *id).collect();
        let photo = order
            .iter()
            .position(|id| *id != left.id() && *id == f.photo().unwrap().0);
        let over = order.iter().position(|id| *id == left.id()).expect("drawn");
        assert!(photo.is_some_and(|photo| photo < over), "over the photo");
        let tint = (f.tints.iter()).find(|(id, _)| *id == left.id()).unwrap().1;
        assert_eq!(tint, Color32::WHITE.gamma_multiply(0.25));
        assert!(
            f.images
                .iter()
                .any(|(id, place)| *id == left.id() && *place == at)
        );
    }

    // What the last frame drew at fit is kept, for a show to fade out; a zoomed photo
    // and the crop tool keep nothing.
    #[test]
    fn the_picture_drawn_at_fit_is_kept_for_a_show_to_fade() {
        let mut f = photo();
        let (id, at) = f.photo().unwrap();
        let (kept, place) = f.view.drawn().expect("kept");
        assert_eq!((kept.id(), place), (id, at));
        f.key(Key::Plus);
        assert!(f.view.drawn().is_none(), "zoomed");
        f.key(Key::Num0);
        assert!(f.view.drawn().is_some());
        f.crop = Some((CropTool::begin(None, 2400, 1200), (2400, 1200), ALL, false));
        f.frame(Vec::new());
        assert!(f.view.drawn().is_none(), "under the crop tool");
    }

    // The bar keeps the width it had awake while the controls are away: woken in a frame
    // that knows nothing of the photo stepped to, it stands where it stood, not at the
    // width of a rectangle that is none.
    #[test]
    fn the_bar_woken_after_a_rest_keeps_its_width() {
        let mut f = photo();
        let glass = color(GLASS);
        let bar = |f: &Fixture| {
            (f.fills.iter())
                .find(|(rect, fill)| *fill == glass && rect.height() == BAR)
                .map(|(rect, _)| *rect)
        };
        let awake = bar(&f).unwrap();
        f.show = Some(Showing {
            playing: true,
            quiet: true,
        });
        f.frame(Vec::new());
        f.caption.clear();
        f.show = playing();
        f.frame(Vec::new());
        assert_eq!(bar(&f), Some(awake));
    }
}
