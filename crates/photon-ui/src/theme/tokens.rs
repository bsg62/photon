//! Every colour in photon, and the scales - the source, as the Svelte photon's
//! `tokens.css` was until the switch-over, held to WCAG 2's contrast by its tests (the
//! assertions that stylesheet's test made).

/// A colour, written as a stylesheet writes one: straight (not premultiplied) red, green,
/// blue and alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

/// `#rrggbb`.
const fn rgb(hex: u32) -> Rgba {
    Rgba((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 0xff)
}

/// `#rrggbbaa`.
const fn rgba(hex: u32) -> Rgba {
    Rgba(
        (hex >> 24) as u8,
        (hex >> 16) as u8,
        (hex >> 8) as u8,
        hex as u8,
    )
}

/// The colours that differ between the light and the dark theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub surface: Rgba,
    pub chrome: Rgba,
    pub raised: Rgba,
    pub field: Rgba,
    pub field_hover: Rgba,
    pub hover: Rgba,
    pub line: Rgba,
    pub text: Rgba,
    pub text_dim: Rgba,
    pub accent: Rgba,
    pub accent_soft: Rgba,
    pub on_accent: Rgba,
    pub danger: Rgba,
    pub star: Rgba,
}

pub const LIGHT: Palette = Palette {
    surface: rgb(0xffffff),
    chrome: rgb(0xebebed),
    raised: rgb(0xffffff),
    field: rgba(0x0000000d),
    field_hover: rgba(0x00000018),
    hover: rgba(0x0000000a),
    line: rgba(0x00000018),
    text: rgb(0x1f1f23),
    text_dim: rgb(0x5f5f67),
    accent: rgb(0x1f6fd6),
    accent_soft: rgba(0x1f6fd633),
    on_accent: rgb(0xffffff),
    danger: rgb(0xbf302b),
    star: rgb(0xe0a100),
};

pub const DARK: Palette = Palette {
    surface: rgb(0x1b1b1e),
    chrome: rgb(0x2c2c31),
    raised: rgb(0x36363c),
    field: rgba(0xffffff12),
    field_hover: rgba(0xffffff1f),
    hover: rgba(0xffffff0d),
    line: rgba(0xffffff14),
    text: rgb(0xededf0),
    text_dim: rgb(0xa6a6af),
    accent: rgb(0x62a0ea),
    accent_soft: rgba(0x62a0ea33),
    on_accent: rgb(0x111111),
    danger: rgb(0xff8d8d),
    star: rgb(0xffd24a),
};

/// Ink for shadows cast onto photos, which are not themed: dark in both themes.
pub const SHADOW_INK: Rgba = rgba(0x000000b3);
/// Drawn onto a photo: a tile's copies mark and video badge. The same in both themes,
/// because the photo under it is.
pub const PHOTO_LINE: Rgba = rgba(0xffffffe6);
/// Dims whatever is behind. Dark in both themes.
pub const SCRIM: Rgba = rgba(0x000000a6);

/// What the viewer's controls stand on. The viewer is dark in both themes, so its glass
/// is in the dark theme's block alone. Nine tenths opaque on its own: a web view blurs
/// what is behind it as well, and the opacity is what reads where it cannot.
pub const GLASS: Rgba = rgba(0x26262be6);
pub const GLASS_LINE: Rgba = rgba(0xffffff1a);
/// A link on the glass, which over a white photo is a far lighter ground than any panel:
/// lighter than the accent, of the same hue.
pub const ACCENT_GLASS: Rgba = rgb(0x77aef0);

/// A shadow: how far it is offset, how far it is blurred, and its ink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub ink: Rgba,
}

/// `--shadow-menu`: what lies under a message, a menu, a list. Dark ink in both themes: a
/// shadow reads dark against a light surface too.
pub const SHADOW_MENU: Shadow = Shadow {
    x: 0.0,
    y: 6.0,
    blur: 24.0,
    ink: rgba(0x00000040),
};

/// `--shadow-dialog`: what lies under a dialog, which stands further off the window
/// than a menu does.
pub const SHADOW_DIALOG: Shadow = Shadow {
    x: 0.0,
    y: 16.0,
    blur: 48.0,
    ink: rgba(0x00000059),
};

/// Corner radii, `--r-1` to `--r-4`.
pub const R: [f32; 4] = [4.0, 6.0, 8.0, 12.0];
/// Spacing, `--s-1` to `--s-6`.
pub const S: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];
/// Type sizes, `--t-1` to `--t-5`.
pub const T: [f32; 5] = [11.0, 12.0, 13.0, 15.0, 18.0];

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2's relative luminance of a colour, its alpha ignored.
    fn luminance(colour: Rgba) -> f64 {
        let channel = |value: u8| {
            let v = f64::from(value) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(colour.0) + 0.7152 * channel(colour.1) + 0.0722 * channel(colour.2)
    }

    fn contrast(a: Rgba, b: Rgba) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        let (light, dark) = if a > b { (a, b) } else { (b, a) };
        (light + 0.05) / (dark + 0.05)
    }

    /// `top` laid over an opaque `ground`.
    fn over(top: Rgba, ground: Rgba) -> Rgba {
        let alpha = f64::from(top.3) / 255.0;
        let mix =
            |t: u8, g: u8| (f64::from(t) * alpha + f64::from(g) * (1.0 - alpha)).round() as u8;
        Rgba(
            mix(top.0, ground.0),
            mix(top.1, ground.1),
            mix(top.2, ground.2),
            0xff,
        )
    }

    const WHITE: Rgba = rgb(0xffffff);

    #[test]
    fn the_two_spellings_of_a_colour_are_read_alike() {
        assert_eq!(rgb(0x1f6fd6), Rgba(0x1f, 0x6f, 0xd6, 0xff));
        assert_eq!(rgba(0x1f6fd633), Rgba(0x1f, 0x6f, 0xd6, 0x33));
    }

    #[test]
    fn the_contrast_helpers_agree_with_the_wcag_reference_values() {
        assert!((contrast(rgb(0x000000), WHITE) - 21.0).abs() < 1e-6);
        assert!((contrast(rgb(0x777777), WHITE) - 4.48).abs() < 0.01);
    }

    #[test]
    fn each_theme_is_readable_where_its_colours_are_used() {
        for (name, t) in [("light", LIGHT), ("dark", DARK)] {
            for ground in [t.surface, t.chrome] {
                assert!(contrast(t.text, ground) >= 4.5, "{name}: text");
                assert!(contrast(t.text_dim, ground) >= 4.5, "{name}: dim text");
                assert!(contrast(t.danger, ground) >= 4.5, "{name}: danger");
                assert!(contrast(t.accent, ground) >= 3.0, "{name}: the accent");
            }
            assert!(
                contrast(t.on_accent, t.accent) >= 4.5,
                "{name}: a primary button"
            );
            // A hovered row still shows its dim count; an active row's count is in the
            // full colour, dim text not reaching 4.5 over the soft accent.
            assert!(
                contrast(t.text_dim, over(t.hover, t.chrome)) >= 4.5,
                "{name}: hovered"
            );
            assert!(
                contrast(t.text, over(t.accent_soft, t.chrome)) >= 4.5,
                "{name}: active"
            );
            // "Delete…" on a menu, hovered or not, and "Remove…" in Settings, hovered.
            assert!(
                contrast(t.danger, t.raised) >= 4.5,
                "{name}: danger on a menu"
            );
            assert!(
                contrast(t.danger, over(t.hover, t.raised)) >= 4.5,
                "{name}: hovered item"
            );
            assert!(
                contrast(t.danger, over(t.field_hover, t.surface)) >= 4.5,
                "{name}: Remove"
            );
        }
    }

    // The viewer's glass over the brightest photo: its words, dim ones too, and its links,
    // which have an accent of their own - the app's reaches only 4.10 there.
    #[test]
    fn the_viewers_glass_is_readable_over_a_white_photo() {
        let ground = over(GLASS, WHITE);
        assert!(contrast(DARK.text_dim, ground) >= 4.5);
        assert!(contrast(DARK.text, ground) >= 4.5);
        assert!(contrast(ACCENT_GLASS, ground) >= 4.5);
        assert!(
            contrast(ACCENT_GLASS, DARK.surface) >= 4.5,
            "still the link colour"
        );
    }
}
