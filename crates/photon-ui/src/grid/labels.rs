//! What the grid writes: a header's name, count and month, and a video's running time.
//!
//! Dates are written in English. The Svelte UI wrote them in the system's locale, through
//! the browser; photon takes no ICU, so a locale is a decision for the sub-project that
//! brings the sidebar, which reads the same instants.

use jiff::{Timestamp, civil::Weekday, tz::TimeZone};
use photon_core::{grid::Period, library::Folder};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn month_name(month: i64) -> &'static str {
    MONTHS[(month.clamp(1, 12) - 1) as usize]
}

/// `1234567` as `1,234,567`.
pub fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn photo_count(count: usize) -> String {
    if count == 1 {
        "1 photo".to_owned()
    } else {
        format!("{} photos", grouped(count))
    }
}

/// What photon calls a folder: the user's alias, else its directory name.
pub fn folder_label(folder: &Folder) -> &str {
    folder.alias.as_deref().unwrap_or(&folder.name)
}

/// What a folder's header says after its name: how many of its photos the view holds, and
/// the month its oldest one was taken - "23 photos · July 2026".
///
/// The oldest, not a range to the newest: a photo with no date of its own is dated by its
/// file, so one scan copied over yesterday would stretch a folder from 1998 "to" this
/// month. The month is read in `zone`, the viewer's own.
pub fn folder_summary(count: usize, taken_at_min: i64, zone: &TimeZone) -> String {
    let count = photo_count(count);
    match Timestamp::from_second(taken_at_min) {
        Ok(instant) => {
            let local = instant.to_zoned(zone.clone());
            format!(
                "{count} · {} {}",
                month_name(i64::from(local.month())),
                local.year()
            )
        }
        // A date no calendar holds: the count alone.
        Err(_) => count,
    }
}

/// A period's header: "2024", "June 2024" or "Saturday, June 15, 2024". From the section's
/// own numbers, never from an instant, which would be read again in the viewer's zone.
pub fn period_label(period: Period) -> String {
    let Some(month) = period.month else {
        return period.year.to_string();
    };
    let month_and_year = |day: Option<u32>| match day {
        Some(day) => format!("{} {day}, {}", month_name(i64::from(month)), period.year),
        None => format!("{} {}", month_name(i64::from(month)), period.year),
    };
    let Some(day) = period.day else {
        return month_and_year(None);
    };
    let weekday = i16::try_from(period.year)
        .ok()
        .zip(i8::try_from(month).ok())
        .zip(i8::try_from(day).ok())
        .and_then(|((year, month), day)| jiff::civil::Date::new(year, month, day).ok())
        .map(|date| match date.weekday() {
            Weekday::Monday => "Monday",
            Weekday::Tuesday => "Tuesday",
            Weekday::Wednesday => "Wednesday",
            Weekday::Thursday => "Thursday",
            Weekday::Friday => "Friday",
            Weekday::Saturday => "Saturday",
            Weekday::Sunday => "Sunday",
        });
    match weekday {
        Some(weekday) => format!("{weekday}, {}", month_and_year(Some(day))),
        None => month_and_year(Some(day)),
    }
}

/// A video's running time: "0:07", "12:34", "1:02:03".
pub fn format_duration(ms: i64) -> String {
    let total = ms.max(0) / 1000;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_is_grouped_in_threes() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(300_000), "300,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(photo_count(1), "1 photo");
        assert_eq!(photo_count(0), "0 photos");
        assert_eq!(photo_count(23), "23 photos");
        assert_eq!(photo_count(12_345), "12,345 photos");
    }

    #[test]
    fn a_folder_is_called_by_its_alias_when_it_has_one() {
        let mut folder = Folder {
            id: 1,
            watched_id: 1,
            parent_id: None,
            path: "/photos/2024-06".to_owned(),
            name: "2024-06".to_owned(),
            hidden: false,
            alias: None,
        };
        assert_eq!(folder_label(&folder), "2024-06");
        folder.alias = Some("Summer".to_owned());
        assert_eq!(folder_label(&folder), "Summer");
    }

    // 2026-07-01 00:30 UTC: July in UTC, still June five hours west.
    const JULY_FIRST: i64 = 1_782_865_800;

    #[test]
    fn a_folders_month_is_read_in_the_viewers_zone() {
        assert_eq!(
            folder_summary(23, JULY_FIRST, &TimeZone::UTC),
            "23 photos · July 2026"
        );
        let west = TimeZone::fixed(jiff::tz::offset(-5));
        assert_eq!(folder_summary(1, JULY_FIRST, &west), "1 photo · June 2026");
        assert_eq!(folder_summary(5, i64::MAX, &TimeZone::UTC), "5 photos");
    }

    #[test]
    fn a_period_is_named_by_its_own_numbers() {
        let period = |month, day| Period {
            year: 2024,
            month,
            day,
        };
        assert_eq!(period_label(period(None, None)), "2024");
        assert_eq!(period_label(period(Some(6), None)), "June 2024");
        assert_eq!(
            period_label(period(Some(6), Some(15))),
            "Saturday, June 15, 2024"
        );
        // A day no month has is written without a weekday, not refused.
        assert_eq!(period_label(period(Some(2), Some(31))), "February 31, 2024");
    }

    #[test]
    fn a_running_time_is_minutes_and_seconds_and_hours_when_it_has_them() {
        assert_eq!(format_duration(7_900), "0:07");
        assert_eq!(format_duration(754_000), "12:34");
        assert_eq!(format_duration(3_723_000), "1:02:03");
        assert_eq!(format_duration(-5), "0:00");
    }
}
