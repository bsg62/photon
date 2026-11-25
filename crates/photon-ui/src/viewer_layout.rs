//! Where things stand in the viewer: the photo's area and the photo fitted into it.
//! `viewer-layout.ts` and `faces.ts`'s `containedBox`, as far as this pull request draws.

/// A rectangle in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// How far the controls stand from the window's edges.
pub const EDGE: f32 = 12.0;
/// The bar at the foot of the window: its height, with the caption and the tools in it.
pub const BAR: f32 = 38.0;
/// The button that closes the viewer, in the corner.
pub const CLOSE: f32 = 32.0;
/// The buttons for the photo before and the one after, at the window's sides.
pub const NAV: f32 = 40.0;
/// The info panel's width, and how far it stands from the window's right edge: clear of
/// the button that closes, in the corner. The panel is placed and the photo's area worked
/// out from the same numbers, so the room the photo is given up and the panel it is given
/// up to cannot come to disagree.
pub const INFO_WIDTH: f32 = 280.0;
pub const INFO_RIGHT: f32 = 56.0;
/// The black left between the photo's area and the panel.
pub const INFO_GAP: f32 = 12.0;
/// How far the panel's foot stands from the window's: clear of the bar and the slider.
pub const INFO_BOTTOM: f32 = 56.0;
/// How far the photo's own caption stands from the window's foot: over the bar.
pub const CAPTION_BOTTOM: f32 = 64.0;
/// The room kept at the window's foot for the bar, where the photo is kept clear of it:
/// the bar and the black on either side of it, as far from the photo as from the edge.
pub const BAR_ROOM: f32 = EDGE + BAR + EDGE;

/// What is known of the photo's own caption, for the room kept under the photo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Caption {
    /// The library has not said yet what the photo is: a step's round trip.
    NotKnown,
    /// It has none, or none is drawn over the photo (the info panel has it, or the crop
    /// tool is open).
    Without,
    /// One is drawn over the bar, this many points high.
    Lines(f32),
}

/// The room kept at the window's foot, under the photo: nothing where the photo is not
/// kept `clear` of the bar, the bar's, or up to the top of a caption drawn over it. Where
/// the caption is not known yet, the room it `was`: at every step the library's answer
/// is a round trip away, and the photo shrank to the bar's room and grew back for each
/// captioned photo stepped to - the jump `bar_place`'s `was` ends for the bar.
pub fn foot_room(clear: bool, caption: Caption, was: Option<f32>) -> f32 {
    if !clear {
        return 0.0;
    }
    match caption {
        Caption::NotKnown => was.unwrap_or(BAR_ROOM),
        Caption::Without => BAR_ROOM,
        Caption::Lines(height) => CAPTION_BOTTOM + height + EDGE,
    }
}

/// How much of the window's right side the info panel takes from the photo, `shown` of
/// the way open: nought shut, one open, and between while it slides. The photo is fitted
/// beside the panel, not under it: under it the panel covered a third of a landscape
/// photo, and the faces it lists were among what it covered.
pub fn info_room(shown: f32) -> f32 {
    shown * (INFO_WIDTH + INFO_RIGHT + INFO_GAP)
}

/// The area the photo is fitted into, in a window of this frame: all of it, less the
/// info panel's room as far as the panel is `shown` and the `foot` kept under the photo
/// (`foot_room`). What the zoom and the pan are measured in, and from whose middle - in
/// every frame of a slide, so the photo shrinks and grows with the panel instead of
/// jumping at either end of it.
pub fn photo_area(window: Frame, shown: f32, foot: f32) -> Frame {
    Frame {
        width: (window.width - info_room(shown)).max(0.0),
        height: (window.height - foot).max(0.0),
        ..window
    }
}

/// Where the info panel stands in the window, `shown` of the way in: from just past the
/// window's right edge to its place clear of the corner. It comes in whole, at its own
/// width, so what it says is not squashed and wrapped again as it slides.
pub fn info_place(window: Frame, shown: f32) -> Frame {
    Frame {
        left: window.left + window.width - shown * (INFO_RIGHT + INFO_WIDTH),
        top: window.top + EDGE,
        width: INFO_WIDTH,
        height: (window.height - EDGE - INFO_BOTTOM).max(0.0),
    }
}

/// The rectangle a picture `width` by `height` takes in `frame`, fitted: scaled to touch
/// the frame on its longer relative side and centred on the other. A small picture is
/// drawn larger, as the web view's `object-fit: contain` draws it. Nothing gives nothing,
/// not a rectangle of no number.
pub fn contained(width: f32, height: f32, frame: Frame) -> Frame {
    if !(width > 0.0 && height > 0.0 && frame.width > 0.0 && frame.height > 0.0) {
        return Frame {
            left: frame.left,
            top: frame.top,
            width: 0.0,
            height: 0.0,
        };
    }
    let scale = (frame.width / width).min(frame.height / height);
    let (width, height) = (width * scale, height * scale);
    Frame {
        left: frame.left + (frame.width - width) / 2.0,
        top: frame.top + (frame.height - height) / 2.0,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Frame = Frame {
        left: 0.0,
        top: 0.0,
        width: 1000.0,
        height: 700.0,
    };

    #[test]
    fn a_photo_is_fitted_to_the_side_it_reaches_first_and_centred_on_the_other() {
        // Wider than the window is: its width fills it.
        let wide = contained(4000.0, 2000.0, WINDOW);
        assert_eq!((wide.left, wide.width, wide.height), (0.0, 1000.0, 500.0));
        assert_eq!(wide.top, 100.0);
        // Upright: its height does.
        let tall = contained(2000.0, 4000.0, WINDOW);
        assert_eq!((tall.top, tall.height, tall.width), (0.0, 700.0, 350.0));
        assert_eq!(tall.left, 325.0);
        // In a frame that does not begin at the corner.
        let moved = contained(
            100.0,
            100.0,
            Frame {
                left: 50.0,
                top: 20.0,
                width: 400.0,
                height: 200.0,
            },
        );
        assert_eq!(
            (moved.left, moved.top, moved.width, moved.height),
            (150.0, 20.0, 200.0, 200.0)
        );
    }

    #[test]
    fn the_info_panel_takes_its_room_from_the_photo_only_while_it_is_open() {
        assert_eq!(info_room(0.0), 0.0);
        assert_eq!(info_room(1.0), INFO_WIDTH + INFO_RIGHT + INFO_GAP);
        let window = Frame {
            left: 10.0,
            top: 20.0,
            width: 1280.0,
            height: 800.0,
        };
        // The whole window with the panel closed; what the panel leaves, at the
        // window's full height and from its own corner, while it is open.
        assert_eq!(photo_area(window, 0.0, 0.0), window);
        assert_eq!(
            photo_area(window, 1.0, 0.0),
            Frame {
                width: 1280.0 - 348.0,
                ..window
            }
        );
        // Never less than nothing, in a window narrower than the panel's room.
        let narrow = Frame {
            width: 200.0,
            height: -5.0,
            ..window
        };
        let area = photo_area(narrow, 1.0, 0.0);
        assert_eq!((area.width, area.height), (0.0, 0.0));
    }

    #[test]
    fn the_info_panel_stands_beside_the_photos_area_clear_of_the_corner_and_the_bar() {
        let window = Frame {
            left: 10.0,
            top: 20.0,
            width: 1280.0,
            height: 800.0,
        };
        let panel = info_place(window, 1.0);
        assert_eq!(
            panel,
            Frame {
                left: 10.0 + 1280.0 - 56.0 - 280.0,
                top: 32.0,
                width: 280.0,
                height: 800.0 - 12.0 - 56.0,
            }
        );
        // The gap between the photo's area and the panel is black.
        let area = photo_area(window, 1.0, 0.0);
        assert_eq!(panel.left - (area.left + area.width), INFO_GAP);
        // A window too low for it has a panel of no height, not of less.
        assert_eq!(
            info_place(
                Frame {
                    height: 40.0,
                    ..window
                },
                1.0
            )
            .height,
            0.0
        );
    }

    // Part of the way open, the panel takes as much of its room from the photo, and
    // stands as far in from the window's right edge: shut it is just past that edge, and
    // open where it has always stood.
    #[test]
    fn a_panel_part_way_takes_part_of_its_room() {
        let window = Frame {
            left: 10.0,
            top: 20.0,
            width: 1280.0,
            height: 800.0,
        };
        assert_eq!(
            photo_area(window, 0.5, 0.0).width,
            window.width - 0.5 * info_room(1.0)
        );
        let shut = info_place(window, 0.0);
        assert_eq!(shut.left, window.left + window.width);
        assert_eq!(shut.width, INFO_WIDTH);
        assert_eq!(
            info_place(window, 1.0),
            Frame {
                left: 10.0 + 1280.0 - 56.0 - 280.0,
                top: 32.0,
                width: 280.0,
                height: 800.0 - 12.0 - 56.0,
            }
        );
        // Half way, half way in, at its own width; and never over the photo's area.
        let half = info_place(window, 0.5);
        assert_eq!(half.left, 10.0 + 1280.0 - 0.5 * (56.0 + 280.0));
        assert_eq!(half.width, INFO_WIDTH);
        let area = photo_area(window, 0.5, 0.0);
        assert!(half.left > area.left + area.width);
    }

    #[test]
    fn the_room_at_the_foot_is_the_bars_or_up_to_the_caption() {
        assert_eq!(foot_room(false, Caption::Lines(20.0), Some(99.0)), 0.0);
        assert_eq!(foot_room(true, Caption::Without, Some(99.0)), 62.0);
        assert_eq!(
            foot_room(true, Caption::Lines(20.0), None),
            64.0 + 20.0 + 12.0
        );
    }

    // At a step the caption is not known for the round trip the library's answer takes:
    // the room stays what it was, so a photo does not jump to the bar's room and back.
    #[test]
    fn the_room_is_kept_across_a_step_until_the_caption_is_known() {
        assert_eq!(foot_room(true, Caption::NotKnown, Some(96.0)), 96.0);
        assert_eq!(foot_room(true, Caption::NotKnown, None), BAR_ROOM);
    }

    #[test]
    fn the_photos_area_stops_the_room_above_the_foot() {
        let window = Frame {
            left: 10.0,
            top: 20.0,
            width: 1280.0,
            height: 800.0,
        };
        assert_eq!(
            photo_area(window, 1.0, 62.0),
            Frame {
                width: 1280.0 - 348.0,
                height: 800.0 - 62.0,
                ..window
            }
        );
    }

    #[test]
    fn a_window_lower_than_the_room_has_an_area_of_nothing() {
        let low = Frame {
            left: 0.0,
            top: 0.0,
            width: 500.0,
            height: 40.0,
        };
        assert_eq!(photo_area(low, 0.0, BAR_ROOM).height, 0.0);
    }

    #[test]
    fn a_small_photo_is_drawn_larger_to_fit() {
        let small = contained(100.0, 50.0, WINDOW);
        assert_eq!((small.width, small.height), (1000.0, 500.0));
    }

    #[test]
    fn nothing_is_fitted_as_nothing() {
        for (width, height, frame) in [
            (0.0, 100.0, WINDOW),
            (100.0, 0.0, WINDOW),
            (
                100.0,
                100.0,
                Frame {
                    width: 0.0,
                    ..WINDOW
                },
            ),
            (f32::NAN, 100.0, WINDOW),
        ] {
            let none = contained(width, height, frame);
            assert_eq!((none.width, none.height), (0.0, 0.0));
        }
    }
}
