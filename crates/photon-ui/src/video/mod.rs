//! A video, through the system's own player: a poster frame at a time, and a player with
//! its sound whose frames come out as RGBA, turned as the video is meant to be seen (spec
//! `2026-10-14-photon-native-video-design.md`). One backend for each system - GStreamer,
//! AVFoundation, Media Foundation - behind the same few calls; what is common to them is
//! here, and has no system in it.
//!
//! photon reads a video and writes none: every backend opens the file to read it.

#[cfg(target_os = "macos")]
#[path = "avf.rs"]
mod system;
#[cfg(target_os = "linux")]
#[path = "gst.rs"]
mod system;
#[cfg(windows)]
#[path = "mf.rs"]
mod system;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub use system::{Player, can_play, poster};

/// A frame of a video, as RGBA with its rows packed, and the time in it that it shows.
#[derive(Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// Seconds into the video.
    pub at: f64,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Picture({}x{} at {:.3} s)",
            self.width, self.height, self.at
        )
    }
}

impl Picture {
    /// A picture from rows of `stride` bytes, of which each row's first `width * 4` are
    /// its pixels. `swap` reads them as BGRA, which two of the three systems hand over.
    /// The fourth byte is made opaque whatever it was: a video has no alpha, and Media
    /// Foundation's RGB32 leaves that byte undefined - often nought, a frame drawn
    /// transparent.
    pub fn from_rows(
        width: u32,
        height: u32,
        at: f64,
        data: &[u8],
        stride: usize,
        swap: bool,
    ) -> Self {
        let row = width as usize * 4;
        let mut rgba = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            for px in data[y * stride..y * stride + row].as_chunks::<4>().0 {
                if swap {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        Picture {
            width,
            height,
            at,
            rgba,
        }
    }

    /// The colour at a pixel.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

/// Why a video could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// The system has no decoder for it - HEVC on a Windows without Microsoft's extension,
    /// anything on a Linux without GStreamer's plugins. Not the file's fault: installing
    /// one later makes it readable.
    Unsupported(String),
    /// The file is broken, or the system failed on it.
    Broken(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unsupported(why) | Failure::Broken(why) => f.write_str(why),
        }
    }
}

/// Where in a video its poster is taken: a second in, or a tenth of the way for a video
/// shorter than ten seconds - its first frame is often black. The web view's rule
/// (`video-thumbnailer.svelte.ts`).
pub fn poster_time(duration_ms: Option<i64>) -> f64 {
    let whole = duration_ms.unwrap_or(0).max(0) as f64 / 1000.0;
    if whole <= 0.0 {
        0.0
    } else {
        (whole / 10.0).min(1.0)
    }
}

/// `picture` turned clockwise by `quarters` quarter turns: what a backend whose framework
/// leaves a video as it is stored does with the turn the file asks for.
pub fn turn(picture: Picture, quarters: u32) -> Picture {
    let quarters = quarters % 4;
    if quarters == 0 {
        return picture;
    }
    let (w, h) = (picture.width as usize, picture.height as usize);
    let (nw, nh) = if quarters % 2 == 1 { (h, w) } else { (w, h) };
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            // Where the pixel at (x, y) lands, turned clockwise.
            let (nx, ny) = match quarters {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            let from = (y * w + x) * 4;
            let to = (ny * nw + nx) * 4;
            rgba[to..to + 4].copy_from_slice(&picture.rgba[from..from + 4]);
        }
    }
    Picture {
        width: nw as u32,
        height: nh as u32,
        at: picture.at,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture 2 wide and 3 high whose pixels are numbered 0 to 5 row by row.
    fn numbered() -> Picture {
        Picture {
            width: 2,
            height: 3,
            at: 1.5,
            rgba: (0..6u8).flat_map(|n| [n, n, n, 255]).collect(),
        }
    }

    fn numbers(picture: &Picture) -> Vec<u8> {
        picture
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| px[0])
            .collect()
    }

    // 0 1        4 2 0        5 4        1 3 5
    // 2 3   ->   5 3 1   ,    3 2   ,    0 2 4
    // 4 5                     1 0
    #[test]
    fn a_picture_is_turned_clockwise_by_quarters() {
        let once = turn(numbered(), 1);
        assert_eq!((once.width, once.height), (3, 2));
        assert_eq!(numbers(&once), [4, 2, 0, 5, 3, 1]);
        let twice = turn(numbered(), 2);
        assert_eq!((twice.width, twice.height), (2, 3));
        assert_eq!(numbers(&twice), [5, 4, 3, 2, 1, 0]);
        let thrice = turn(numbered(), 3);
        assert_eq!((thrice.width, thrice.height), (3, 2));
        assert_eq!(numbers(&thrice), [1, 3, 5, 0, 2, 4]);
        assert_eq!(turn(numbered(), 4), numbered());
        assert_eq!(once.at, 1.5);
    }

    #[test]
    fn rows_are_packed_bgra_is_read_as_bgra_and_every_pixel_is_opaque() {
        // Two rows of one pixel each, eight bytes apart, the second with no alpha.
        let data = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 0];
        let plain = Picture::from_rows(1, 2, 0.0, &data, 8, false);
        assert_eq!(plain.rgba, [1, 2, 3, 255, 5, 6, 7, 255]);
        let swapped = Picture::from_rows(1, 2, 0.0, &data, 8, true);
        assert_eq!(swapped.rgba, [3, 2, 1, 255, 7, 6, 5, 255]);
    }

    #[test]
    fn a_poster_is_a_second_in_or_a_tenth_of_a_short_video() {
        assert_eq!(poster_time(Some(60_000)), 1.0);
        assert_eq!(poster_time(Some(4_000)), 0.4);
        assert_eq!(poster_time(Some(0)), 0.0);
        assert_eq!(poster_time(None), 0.0);
        assert_eq!(poster_time(Some(-5)), 0.0);
    }
}
