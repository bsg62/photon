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
    viewer_layout::{BAR, CLOSE, EDGE, Frame, NAV, contained},
    viewer_levels::level_for,
};
use eframe::egui::{
    self, Color32, Event, Id, Key, PointerButton, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    WidgetInfo, WidgetType, pos2, vec2,
};

/// A tool in the bar: the star.
const TOOL: f32 = 30.0;
/// The room the zoom's slider takes at the bar's right, and the same kept on its left so
/// that the bar stays in the middle: the bar is no wider than what is left between.
const BESIDE_BAR: f32 = 214.0;
/// How far the message keeps from the window's sides: clear of the two buttons there.
const MESSAGE_SIDE: f32 = 64.0;
/// The photo's own caption, over the bar.
const CAPTION_BOTTOM: f32 = 64.0;

/// What the user did in a frame of the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerAction {
    Close,
    /// One photo on (1) or back (-1).
    Step(i64),
    First,
    Last,
    Star,
    Hide,
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
    /// Whether the photo is starred; none while that is not known, or for a photo that
    /// cannot be - the button is then there and takes no press.
    pub starred: Option<bool>,
    pub can_prev: bool,
    pub can_next: bool,
}

#[derive(Default)]
pub struct ViewerView {
    /// What the wheel has turned towards the next photo and not yet reached.
    wheel: f32,
}

impl ViewerView {
    /// Draws the viewer over the whole window and answers what was done in it.
    pub fn show(&mut self, ui: &egui::Ui, data: &ViewerData<'_>) -> Vec<ViewerAction> {
        let window = ui.ctx().content_rect();
        let painter = ui.painter().clone();
        // The one colour in photon that no token holds: the ground a photo is judged
        // against is not a theme's to choose.
        painter.rect_filled(window, 0.0, Color32::BLACK);
        let mut actions = self.keys(ui);

        let area = Frame {
            left: window.left(),
            top: window.top(),
            width: window.width(),
            height: window.height(),
        };
        let shows = !data.message.is_some_and(|message| !message.over);
        if shows && let Some((texture, at)) = picture(ui, data, area) {
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), at, whole, Color32::WHITE);
        }
        if let Some(message) = data.message {
            said(ui, window, message);
        }

        // The one before and the one after, at the window's sides.
        let side = |left: f32| {
            Rect::from_min_size(pos2(left, window.center().y - NAV / 2.0), vec2(NAV, NAV))
        };
        if round(
            ui,
            side(window.left() + EDGE),
            "viewer-prev",
            Icon::ChevronLeft,
            "Previous photo",
            data.can_prev,
        ) {
            actions.push(ViewerAction::Step(-1));
        }
        if round(
            ui,
            side(window.right() - EDGE - NAV),
            "viewer-next",
            Icon::ChevronRight,
            "Next photo",
            data.can_next,
        ) {
            actions.push(ViewerAction::Step(1));
        }
        let corner = Rect::from_min_size(
            pos2(window.right() - EDGE - CLOSE, window.top() + EDGE),
            vec2(CLOSE, CLOSE),
        );
        if round(ui, corner, "viewer-close", Icon::X, "Close viewer", true) {
            actions.push(ViewerAction::Close);
        }

        if let Some(line) = data.photo_caption {
            photo_caption(ui, window, line);
        }
        if bar(ui, window, data) {
            actions.push(ViewerAction::Star);
        }
        actions
    }

    /// The frame's keys, its wheel and the mouse's back button, in the order they came.
    ///
    /// A key held down repeats, and a repeat steps again - but not the ones that close
    /// or write: Escape held from whatever it has just closed must not close the viewer
    /// with it, and each press of `.` or `H` is a write. A chord with Ctrl, Cmd or Alt
    /// is not the viewer's.
    fn keys(&mut self, ui: &egui::Ui) -> Vec<ViewerAction> {
        let mut actions = Vec::new();
        let events = ui.input(|input| input.events.clone());
        for event in &events {
            match event {
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
                    actions.extend(match key {
                        Key::ArrowLeft => Some(ViewerAction::Step(-1)),
                        Key::ArrowRight => Some(ViewerAction::Step(1)),
                        Key::Home => Some(ViewerAction::First),
                        Key::End => Some(ViewerAction::Last),
                        Key::Escape | Key::Backspace if !repeat => Some(ViewerAction::Close),
                        Key::Period if !repeat && types_period => Some(ViewerAction::Star),
                        Key::H if !repeat => Some(ViewerAction::Hide),
                        _ => None,
                    });
                }
                // The wheel goes from photo to photo: a notch a photo, whatever the
                // system's lines a notch are, since nothing here scrolls. With Ctrl or
                // Cmd it is the zoom's.
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if !modifiers.command && !modifiers.ctrl => {
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
}

/// How many pixels the fitted copy may differ from what is drawn and still be the copy
/// for this size.
const SAME_SIZE: f32 = 1.5;

/// How many points of a trackpad's stream are one notch of a wheel: what the Svelte
/// viewer takes for one photo.
const POINTS_A_NOTCH: f32 = 100.0;

/// The texture to draw and where, fitted into `area`: the copy made for exactly this
/// size, where there is one; else the level that is at least as large as what is drawn;
/// else the preview. None while there is none of them.
fn picture<'a>(
    ui: &egui::Ui,
    data: &ViewerData<'a>,
    area: Frame,
) -> Option<(&'a TextureHandle, Rect)> {
    let texture = (data.levels.first().map(|level| level.texture)).or(data.preview)?;
    let [width, height] = texture.size();
    let fitted = contained(width as f32, height as f32, area);
    let at = Rect::from_min_size(
        pos2(fitted.left, fitted.top),
        vec2(fitted.width, fitted.height),
    );
    if data.levels.is_empty() {
        return Some((texture, at));
    }
    let scale = ui.ctx().pixels_per_point();
    let drawn = vec2(fitted.width, fitted.height) * scale;
    // The copy scaled for this size is drawn pixel for pixel, on whole pixels: a
    // linear filter then has nothing to do, and what a proper one made of the photo is
    // what is seen. A pixel's difference is the roundings - of the box to whole pixels,
    // of the copy's own size; more is a window that has changed size since, and the
    // levels until the copy for the new one is made.
    if let Some(copy) = data.fitted {
        let [width, height] = copy.size();
        let own = vec2(width as f32, height as f32);
        if (own.x - drawn.x).abs() < SAME_SIZE && (own.y - drawn.y).abs() < SAME_SIZE {
            let corner = ((at.center().to_vec2() * scale - own / 2.0).round() / scale).to_pos2();
            return Some((copy, Rect::from_min_size(corner, own / scale)));
        }
    }
    let widths: Vec<u32> = data.levels.iter().map(|level| level.width).collect();
    Some((data.levels[level_for(&widths, drawn.x)].texture, at))
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

/// The bar at the foot of the window: the star, and the caption, which is the part that
/// gives way when the window is narrow. Answers whether the star was pressed.
fn bar(ui: &egui::Ui, window: Rect, data: &ViewerData<'_>) -> bool {
    let font = fonts::regular(T[1]);
    let wanted = ui
        .painter()
        .layout_no_wrap(data.caption.to_owned(), font.clone(), color(DARK.text_dim))
        .size()
        .x;
    let pad = (BAR - TOOL) / 2.0;
    let most = (window.width() - 2.0 * BESIDE_BAR).max(TOOL + 2.0 * pad);
    let width = (pad + TOOL + S[1] + wanted + S[2]).min(most);
    let rect = Rect::from_min_size(
        pos2(
            window.center().x - width / 2.0,
            window.bottom() - EDGE - BAR,
        ),
        vec2(width, BAR),
    );
    glass(ui, rect, R[3]);

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

    let left = star.right() + S[1];
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
    response.clicked()
}

/// The photo's own caption, over the bar and centred like it: two lines at most, so that
/// a long one does not climb over the photo. The info panel has it whole.
fn photo_caption(ui: &egui::Ui, window: Rect, line: &str) {
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
        /// The textures painted in the last frame, each with where.
        images: Vec<(egui::TextureId, Rect)>,
        texts: Vec<(String, Rect)>,
        fills: Vec<(Rect, Color32)>,
        repaint: std::time::Duration,
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
                images: Vec::new(),
                texts: Vec::new(),
                fills: Vec::new(),
                repaint: std::time::Duration::MAX,
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
            let data = ViewerData {
                levels: &levels,
                fitted: self.fitted.as_ref(),
                preview: self.preview.as_ref(),
                message: (self.message).map(|(text, over)| Message { text, over }),
                caption: &self.caption,
                photo_caption: self.photo_caption,
                starred: self.starred,
                can_prev: self.can_prev,
                can_next: self.can_next,
            };
            let view = &mut self.view;
            let mut actions = Vec::new();
            let mut full = self.ctx.run_ui(input, |ui| {
                crate::icons::install(ui.ctx());
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| actions = view.show(ui, &data));
            });
            full.textures_delta.clear();
            self.images.clear();
            self.texts.clear();
            self.fills.clear();
            for clipped in &full.shapes {
                each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Text(text) => self
                        .texts
                        .push((text.galley.text().to_owned(), text.visual_bounding_rect())),
                    egui::Shape::Rect(rect) => match &rect.brush {
                        Some(brush) => self.images.push((brush.fill_texture_id, rect.rect)),
                        None => self.fills.push((rect.rect, rect.fill)),
                    },
                    egui::Shape::Mesh(mesh) => {
                        self.images.push((mesh.texture_id, mesh.calc_bounds()));
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
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)])
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
        // In the middle, with the star before it.
        assert!((caption.center().x - 500.0).abs() < 30.0, "{caption:?}");
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
}
