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

    /// The website's pages (`site/`), hand-written with no build step: their stylesheets
    /// say their palette is the app's, and this is what holds them to it. It had drifted
    /// before - two tokens were approximations nobody had compared.
    const PAGES: [(&str, &str); 2] = [
        ("index.html", include_str!("../../../../site/index.html")),
        ("404.html", include_str!("../../../../site/404.html")),
    ];

    /// The pages' tokens that are their own, with no colour of the app's to be.
    const OWN: [&str; 1] = ["--shadow"];

    /// The app's colour for a page's token, by the stylesheet's name for it.
    fn token(theme: &Palette, name: &str) -> Option<Rgba> {
        Some(match name {
            "--surface" => theme.surface,
            "--chrome" => theme.chrome,
            "--raised" => theme.raised,
            "--field" => theme.field,
            "--field-hover" => theme.field_hover,
            "--hover" => theme.hover,
            "--line" => theme.line,
            "--text" => theme.text,
            "--text-dim" => theme.text_dim,
            "--accent" => theme.accent,
            "--accent-soft" => theme.accent_soft,
            "--on-accent" => theme.on_accent,
            "--danger" => theme.danger,
            "--star" => theme.star,
            _ => return None,
        })
    }

    /// `#rrggbb` or `#rrggbbaa`.
    fn hex(value: &str) -> Option<Rgba> {
        let digits = value.strip_prefix('#')?;
        let number = u32::from_str_radix(digits, 16).ok()?;
        match digits.len() {
            6 => Some(rgb(number)),
            8 => Some(rgba(number)),
            _ => None,
        }
    }

    /// A page's CSS: every `<style>` block, since a second would override the first, with
    /// its comments taken out.
    fn page_css(html: &str) -> String {
        let mut css = String::new();
        let mut rest = html;
        while let Some(at) = rest.find("<style") {
            let open = at + rest[at..].find('>').unwrap() + 1;
            let close = open + rest[open..].find("</style>").unwrap();
            css.push_str(&rest[open..close]);
            css.push('\n');
            rest = &rest[close..];
        }
        assert!(!css.is_empty(), "the page has no <style> block");
        let mut bare = String::new();
        let mut rest = css.as_str();
        while let Some(at) = rest.find("/*") {
            bare.push_str(&rest[..at]);
            rest = &rest[at + rest[at..].find("*/").unwrap() + 2..];
        }
        bare.push_str(rest);
        bare
    }

    /// The custom properties a block declares, by name.
    fn declared(block: &str) -> Vec<(String, String)> {
        (block.split(';'))
            .filter_map(|declaration| declaration.split_once(':'))
            .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
            .filter(|(name, _)| name.starts_with("--"))
            .collect()
    }

    /// The inside of the first `{ }` after `from` in `css`, and where it ends.
    fn block(css: &str, from: usize) -> (&str, usize) {
        let open = from + css[from..].find('{').unwrap() + 1;
        let close = open + css[open..].find('}').unwrap();
        (&css[open..close], close + 1)
    }

    struct Themes {
        light: Vec<(String, String)>,
        dark: Vec<(String, String)>,
        /// Tokens declared anywhere else: a second `:root`, or a `--line` on `body`, would
        /// override the two blocks compared and be compared with nothing.
        strays: Vec<String>,
    }

    /// A page's own tokens: its `:root` block outside any media query, and the one inside
    /// `prefers-color-scheme: dark`.
    fn themes(css: &str) -> Themes {
        let media = css
            .find("@media (prefers-color-scheme: dark)")
            .expect("the page has no dark block");
        let root = media + css[media..].find(":root").expect("no dark :root");
        let (dark, end) = block(css, root);
        let media_end = end + css[end..].find('}').unwrap() + 1;
        let outside = format!("{}{}", &css[..media], &css[media_end..]);
        let at = outside
            .find(":root")
            .expect("the page has no light :root block");
        let (light, end) = block(&outside, at);
        let strays = declared(&format!("{}{}", &outside[..at], &outside[end..]))
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        Themes {
            light: declared(light),
            dark: declared(dark),
            strays,
        }
    }

    #[test]
    fn the_websites_colours_are_the_apps() {
        for (page, html) in PAGES {
            let found = themes(&page_css(html));
            for (theme, palette, tokens) in
                [("light", LIGHT, &found.light), ("dark", DARK, &found.dark)]
            {
                let shared: Vec<_> = (tokens.iter())
                    .filter(|(name, _)| !OWN.contains(&name.as_str()))
                    .collect();
                assert!(!shared.is_empty(), "{page}: no {theme} tokens");
                for (name, value) in shared {
                    assert_eq!(
                        hex(value),
                        token(&palette, name),
                        "{page}: {name} ({theme}) is {value}"
                    );
                }
            }
        }
    }

    #[test]
    fn each_page_declares_every_token_for_both_themes_and_nowhere_else() {
        for (page, html) in PAGES {
            let found = themes(&page_css(html));
            let names = |tokens: &[(String, String)]| {
                let mut names: Vec<String> = tokens.iter().map(|(name, _)| name.clone()).collect();
                names.sort();
                names
            };
            // One missing from the dark block is no error anywhere: dark shows the light one.
            assert_eq!(names(&found.dark), names(&found.light), "{page}");
            assert!(found.strays.is_empty(), "{page}: {:?}", found.strays);
        }
    }

    #[test]
    fn each_page_reads_no_token_it_does_not_declare() {
        for (page, html) in PAGES {
            let css = page_css(html);
            let declared: Vec<String> = themes(&css)
                .light
                .into_iter()
                .map(|(name, _)| name)
                .collect();
            // `var(--chrome)` left behind after the token went computes to nothing, silently,
            // however it is spelled: with spaces inside, or a fallback that hides it.
            let read: Vec<&str> = (css.match_indices("var("))
                .map(|(at, _)| css[at + 4..].trim_start())
                .map(|rest| {
                    rest.split(|c: char| !(c.is_alphanumeric() || c == '-'))
                        .next()
                        .unwrap()
                })
                .collect();
            assert!(!read.is_empty(), "{page}");
            for name in read {
                assert!(declared.iter().any(|known| known == name), "{page}: {name}");
            }
        }
    }

    #[test]
    fn a_colour_is_read_in_both_spellings() {
        assert_eq!(hex("#1f6fd6"), Some(LIGHT.accent));
        assert_eq!(hex("#62a0ea33"), Some(DARK.accent_soft));
        assert_eq!(hex("1f6fd6"), None);
    }
}
