//! The line under the photo in the viewer, in Picasa's order: the file's name, when it was
//! taken, its size in pixels and in bytes, and where it stands in its section. `caption.ts`
//! and `photo-caption.ts` in Rust, with their tests' cases.

use jiff::{Timestamp, tz::TimeZone};

const SEPARATOR: &str = " · ";
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// What the caption is made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Captioned<'a> {
    pub file_name: &'a str,
    /// Capture time in seconds: the camera's own clock, kept as if it were UTC.
    pub taken_at: i64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    /// Bytes.
    pub size: i64,
}

/// KB below a megabyte, MB with one decimal above. Binary units, as the file managers
/// photon sits beside report.
pub fn format_size(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes < 1_048_576.0 {
        format!("{} KB", (bytes / 1024.0).round())
    } else {
        format!("{:.1} MB", bytes / 1_048_576.0)
    }
}

/// A capture time as photon shows it: "Jul 14, 2026, 12:30 PM".
///
/// Read in UTC on purpose. `taken_at` is the camera's own clock stored as UTC seconds, so
/// read in the machine's zone the hour would move by that zone's offset: a photo taken at
/// 12:30 would say 14:30 in Berlin. A time no calendar holds says nothing.
pub fn format_taken(taken_at: i64) -> String {
    let Ok(instant) = Timestamp::from_second(taken_at) else {
        return String::new();
    };
    let at = instant.to_zoned(TimeZone::UTC);
    let (hour, half) = match at.hour() {
        0 => (12, "AM"),
        hour @ 1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        hour => (hour - 12, "PM"),
    };
    format!(
        "{} {}, {}, {hour}:{:02} {half}",
        MONTHS[(at.month().clamp(1, 12) - 1) as usize],
        at.day(),
        at.year(),
        at.minute()
    )
}

/// A photo's size in pixels as it is seen. An unedited photo's are stored as the file has
/// them, with the orientation beside them; five to eight are the four that involve a
/// quarter turn. An edited photo arrives upright, with an orientation of one.
pub fn oriented(width: u32, height: u32, orientation: u8) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    }
}

/// The caption: "IMG_0412.jpg · Jul 14, 2026, 12:30 PM · 4000 × 3000 · 2.4 MB (3 / 41)".
/// `position` is the photo's place in its section and how many that holds; none for a
/// photo the view no longer holds, which has no place in it.
pub fn caption(of: Captioned<'_>, position: Option<(usize, usize)>) -> String {
    let (width, height) = oriented(of.width, of.height, of.orientation);
    let mut caption = [
        of.file_name.to_owned(),
        format_taken(of.taken_at),
        format!("{width} × {height}"),
        format_size(of.size),
    ]
    .join(SEPARATOR);
    if let Some((index, count)) = position.filter(|(_, count)| *count > 0) {
        caption.push_str(&format!(" ({index} / {count})"));
    }
    caption
}

/// The photo's own caption as the line over the bar shows it: line breaks and runs of
/// space become one space, since the line is short and a break would spend it on
/// nothing. None for no caption - nothing is drawn, not an empty strip.
pub fn caption_line(caption: Option<&str>) -> Option<String> {
    let line = caption?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo() -> Captioned<'static> {
        Captioned {
            file_name: "IMG_0412.jpg",
            // 2026-07-14 12:30:00, as the camera's clock had it.
            taken_at: 1_784_032_200,
            width: 4000,
            height: 3000,
            orientation: 1,
            size: 2_516_582,
        }
    }

    #[test]
    fn the_caption_is_name_time_pixels_size_and_place() {
        assert_eq!(
            caption(photo(), Some((3, 41))),
            "IMG_0412.jpg · Jul 14, 2026, 12:30 PM · 4000 × 3000 · 2.4 MB (3 / 41)"
        );
    }

    // A photo the view no longer holds has no place in it, and a section of none neither.
    #[test]
    fn a_photo_with_no_place_has_none_said() {
        let said = "IMG_0412.jpg · Jul 14, 2026, 12:30 PM · 4000 × 3000 · 2.4 MB";
        assert_eq!(caption(photo(), None), said);
        assert_eq!(caption(photo(), Some((0, 0))), said);
    }

    #[test]
    fn a_photo_stored_on_its_side_is_measured_as_it_is_seen() {
        let turned = Captioned {
            orientation: 6,
            ..photo()
        };
        assert!(caption(turned, None).contains("3000 × 4000"));
        for upright in [1, 2, 3, 4] {
            assert_eq!(oriented(4000, 3000, upright), (4000, 3000));
        }
        for sideways in [5, 6, 7, 8] {
            assert_eq!(oriented(4000, 3000, sideways), (3000, 4000));
        }
    }

    #[test]
    fn a_time_is_read_on_the_cameras_clock_by_twelve_hours() {
        // Midnight, the morning, noon, the evening.
        assert_eq!(
            format_taken(1_784_032_200 - 12 * 3600 - 30 * 60),
            "Jul 14, 2026, 12:00 AM"
        );
        assert_eq!(
            format_taken(1_784_032_200 - 3 * 3600 - 25 * 60),
            "Jul 14, 2026, 9:05 AM"
        );
        assert_eq!(format_taken(1_784_032_200), "Jul 14, 2026, 12:30 PM");
        assert_eq!(
            format_taken(1_784_032_200 + 11 * 3600 + 29 * 60),
            "Jul 14, 2026, 11:59 PM"
        );
        assert_eq!(format_taken(0), "Jan 1, 1970, 12:00 AM");
        // A time no calendar holds.
        assert_eq!(format_taken(i64::MAX), "");
    }

    #[test]
    fn a_size_is_in_kilobytes_below_a_megabyte_and_megabytes_above() {
        assert_eq!(format_size(0), "0 KB");
        assert_eq!(format_size(1536), "2 KB");
        assert_eq!(format_size(1_048_575), "1024 KB");
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(26_109_542), "24.9 MB");
        assert_eq!(format_size(-5), "0 KB");
    }

    #[test]
    fn a_photos_own_caption_is_one_line_or_nothing() {
        assert_eq!(
            caption_line(Some("  Lisbon,\n\n  at   dusk ")).as_deref(),
            Some("Lisbon, at dusk")
        );
        assert_eq!(caption_line(Some(" \n ")), None);
        assert_eq!(caption_line(None), None);
    }
}
