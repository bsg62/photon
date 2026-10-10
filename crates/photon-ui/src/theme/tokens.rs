//! Every colour in photon, and the scales: `ui/src/tokens.css` as constants.
//!
//! Until the switch-over that file is the source and this one a copy, held to it by
//! `the_tokens_are_the_stylesheets`. Afterwards this is the source, and the contrast
//! assertions of `lib/tokens.test.ts` move here with it.

/// A colour as the stylesheet writes it: straight (not premultiplied) red, green, blue and
/// alpha.
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

impl Palette {
    /// Each colour under its name in the stylesheet.
    pub fn named(&self) -> [(&'static str, Rgba); 14] {
        [
            ("--surface", self.surface),
            ("--chrome", self.chrome),
            ("--raised", self.raised),
            ("--field", self.field),
            ("--field-hover", self.field_hover),
            ("--hover", self.hover),
            ("--line", self.line),
            ("--text", self.text),
            ("--text-dim", self.text_dim),
            ("--accent", self.accent),
            ("--accent-soft", self.accent_soft),
            ("--on-accent", self.on_accent),
            ("--danger", self.danger),
            ("--star", self.star),
        ]
    }
}

/// Ink for shadows cast onto photos, which are not themed: dark in both themes.
pub const SHADOW_INK: Rgba = rgba(0x000000b3);
/// Drawn onto a photo: a tile's copies mark and video badge. The same in both themes,
/// because the photo under it is.
pub const PHOTO_LINE: Rgba = rgba(0xffffffe6);
/// Dims whatever is behind. Dark in both themes.
pub const SCRIM: Rgba = rgba(0x000000a6);

/// The three colours no theme changes, under their names in the stylesheet.
pub const UNTHEMED: [(&str, Rgba); 3] = [
    ("--shadow-ink", SHADOW_INK),
    ("--photo-line", PHOTO_LINE),
    ("--scrim", SCRIM),
];

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

/// Corner radii, `--r-1` to `--r-4`.
pub const R: [f32; 4] = [4.0, 6.0, 8.0, 12.0];
/// Spacing, `--s-1` to `--s-6`.
pub const S: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];
/// Type sizes, `--t-1` to `--t-5`.
pub const T: [f32; 5] = [11.0, 12.0, 13.0, 15.0, 18.0];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const CSS: &str = include_str!("../../../../ui/src/tokens.css");

    /// The custom properties declared in `css` from `start` to the end of the block it is
    /// in, by name.
    fn properties(css: &str, start: usize) -> HashMap<&str, &str> {
        let body = &css[start..];
        let body = &body[..body.find('}').unwrap()];
        body.lines()
            .filter_map(|line| line.trim().strip_suffix(';')?.split_once(':'))
            .filter(|(name, _)| name.starts_with("--"))
            .map(|(name, value)| (name.trim(), value.trim()))
            .collect()
    }

    /// The custom properties of the block of `css` that opens with `selector`.
    fn block<'a>(css: &'a str, selector: &str) -> HashMap<&'a str, &'a str> {
        let start = css
            .find(selector)
            .unwrap_or_else(|| panic!("no block for {selector}"));
        properties(css, start + selector.len())
    }

    /// The bare `:root` block that holds the scales and the unthemed colours: the block
    /// that declares the first radius. Found by what it holds and not by its selector
    /// with the line after it, which is two lines only where a line ends in `\n` alone.
    fn scales(css: &str) -> HashMap<&str, &str> {
        let first = css.find("--r-1").expect("no --r-1 in the stylesheet");
        let open = css[..first].rfind('{').expect("--r-1 is in no block");
        properties(css, open + 1)
    }

    fn parse(value: &str) -> Rgba {
        let hex = value.strip_prefix('#').unwrap_or_else(|| panic!("{value}"));
        let number = u32::from_str_radix(hex, 16).unwrap();
        match hex.len() {
            6 => rgb(number),
            8 => rgba(number),
            _ => panic!("{value} is neither #rrggbb nor #rrggbbaa"),
        }
    }

    #[test]
    fn the_two_spellings_of_a_colour_are_read_alike() {
        assert_eq!(rgb(0x1f6fd6), Rgba(0x1f, 0x6f, 0xd6, 0xff));
        assert_eq!(rgba(0x1f6fd633), Rgba(0x1f, 0x6f, 0xd6, 0x33));
        assert_eq!(parse("#1f6fd6"), rgb(0x1f6fd6));
        assert_eq!(parse("#1f6fd633"), rgba(0x1f6fd633));
    }

    #[test]
    fn the_tokens_are_the_stylesheets() {
        for (selector, palette) in [
            ("[data-theme='light'] {", LIGHT),
            ("[data-theme='dark'] {", DARK),
        ] {
            let css = block(CSS, selector);
            for (name, colour) in palette.named() {
                assert_eq!(parse(css[name]), colour, "{name} in {selector}");
            }
        }
        // The scales and the unthemed colours are in the bare `:root` block.
        let root = scales(CSS);
        for (name, colour) in UNTHEMED {
            assert_eq!(parse(root[name]), colour, "{name}");
        }
        let px = |name: &str| {
            root[name]
                .strip_suffix("px")
                .unwrap()
                .parse::<f32>()
                .unwrap()
        };
        // A shadow is "x y blur colour".
        let shadow: Vec<&str> = root["--shadow-menu"].split_whitespace().collect();
        let length = |text: &str| text.trim_end_matches("px").parse::<f32>().unwrap();
        assert_eq!(
            Shadow {
                x: length(shadow[0]),
                y: length(shadow[1]),
                blur: length(shadow[2]),
                ink: parse(shadow[3]),
            },
            SHADOW_MENU
        );
        for (i, radius) in R.iter().enumerate() {
            assert_eq!(px(&format!("--r-{}", i + 1)), *radius);
        }
        for (i, space) in S.iter().enumerate() {
            assert_eq!(px(&format!("--s-{}", i + 1)), *space);
        }
        for (i, size) in T.iter().enumerate() {
            assert_eq!(px(&format!("--t-{}", i + 1)), *size);
        }
    }

    // A Windows checkout has CRLF line endings, and `include_str!` hands them over as they
    // are. The scales' block is found by its first line and the one after, which is two
    // lines only where a line ends in `\n` alone: the first version found nothing there.
    #[test]
    fn the_stylesheet_is_read_with_either_line_ending() {
        let unix = CSS.replace("\r\n", "\n");
        let windows = unix.replace('\n', "\r\n");
        assert_eq!(scales(&windows), scales(&unix));
        assert_eq!(scales(&unix)["--r-1"], "4px");
    }
}
