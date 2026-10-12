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
