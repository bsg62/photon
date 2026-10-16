//! Dates and numbers as the system writes them: a period's header, a folder's month, a
//! photo's time, "On this day" and every count with thousands, in the system's language
//! and order - "Sonntag, 4. Oktober 2026" and "1.234" on a German system - as the Svelte
//! photon's `Intl` wrote them through the web view. Through ICU4X's own data, compiled in
//! (pure Rust, as the rule against C dependencies wants); the locale is read from the
//! system once (`sys-locale`). The words photon writes itself stay English, as they were
//! in the Svelte photon.
//!
//! One locale for the process, set at launch (`set`) and read wherever a date or a count
//! is written (`current`): the system's does not change under a running window, and a
//! formatter handed down every call chain that writes a number would be in every
//! signature. Until it is set it is English, which is what every test that does not set
//! one reads.

use icu_datetime::{
    DateTimeFormatter, fieldsets,
    input::{Date, DateTime, Time},
};
use icu_decimal::{DecimalFormatter, input::Decimal};
use icu_locale_core::Locale as Tag;
use std::sync::{Arc, RwLock};

/// The formatters of one locale.
pub struct Locale {
    tag: String,
    month_year: DateTimeFormatter<fieldsets::YM>,
    day: DateTimeFormatter<fieldsets::YMDE>,
    day_month: DateTimeFormatter<fieldsets::MD>,
    date_time: DateTimeFormatter<fieldsets::YMDT>,
    numbers: DecimalFormatter,
}

/// What a locale is called where nothing else is known, or the system's name for one is
/// no name ICU4X can read.
const ENGLISH: &str = "en-US";

impl Locale {
    /// The formatters of the locale `tag` - a BCP 47 name, "de-DE" - or none where it is
    /// not one. A locale ICU4X has no data of its own for falls back to its language's,
    /// then to the root's, as a browser's does.
    pub fn of(tag: &str) -> Option<Self> {
        let parsed: Tag = tag.parse().ok()?;
        let prefs = || (&parsed).into();
        Some(Self {
            tag: parsed.to_string(),
            month_year: DateTimeFormatter::try_new(prefs(), fieldsets::YM::long()).ok()?,
            day: DateTimeFormatter::try_new(prefs(), fieldsets::YMDE::long()).ok()?,
            day_month: DateTimeFormatter::try_new(prefs(), fieldsets::MD::long()).ok()?,
            date_time: DateTimeFormatter::try_new(prefs(), fieldsets::YMD::medium().with_time_hm())
                .ok()?,
            numbers: DecimalFormatter::try_new((&parsed).into(), Default::default()).ok()?,
        })
    }

    /// English, as the United States writes it: what photon writes where the system says
    /// nothing it can read.
    pub fn english() -> Self {
        // The one locale whose data is certain to be compiled in.
        Self::of(ENGLISH).unwrap_or_else(|| unreachable!("ICU4X has no English"))
    }

    /// The system's. A name the system gives in its own spelling - `de_DE.UTF-8` from a
    /// Linux environment - is read as the locale it means.
    pub fn system() -> Self {
        let named = sys_locale::get_locale().map(|name| bcp47(&name));
        named
            .and_then(|name| Self::of(&name))
            .unwrap_or_else(Self::english)
    }

    /// Its name, as ICU4X reads it.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// A month of a year: "October 2026", "Oktober 2026".
    pub fn month_year(&self, year: i32, month: u8) -> Option<String> {
        let date = Date::try_new_iso(year, month, 1).ok()?;
        Some(plain(self.month_year.format(&date)))
    }

    /// A day, with its weekday: "Sunday, October 4, 2026", "Sonntag, 4. Oktober 2026".
    pub fn day(&self, year: i32, month: u8, day: u8) -> Option<String> {
        let date = Date::try_new_iso(year, month, day).ok()?;
        Some(plain(self.day.format(&date)))
    }

    /// A day of the year in no year: "October 4", "4. Oktober". In a leap year, so that
    /// the 29th of February is one.
    pub fn day_month(&self, month: u8, day: u8) -> Option<String> {
        let date = Date::try_new_iso(2000, month, day).ok()?;
        Some(plain(self.day_month.format(&date)))
    }

    /// A day and its time to the minute, as a date and time are written in a sentence:
    /// "Oct 4, 2026, 12:30 PM", "04.10.2026, 12:30".
    pub fn date_time(
        &self,
        (year, month, day): (i32, u8, u8),
        (hour, minute): (u8, u8),
    ) -> Option<String> {
        let at = DateTime {
            date: Date::try_new_iso(year, month, day).ok()?,
            time: Time::try_new(hour, minute, 0, 0).ok()?,
        };
        Some(plain(self.date_time.format(&at)))
    }

    /// A count, with its thousands: "1,234", "1.234", "1 234".
    pub fn count(&self, count: i64) -> String {
        plain(self.numbers.format(&Decimal::from(count)))
    }
}

/// What ICU4X wrote, with its narrow no-break spaces - before "PM", between a French
/// count's thousands - as plain ones: the faces photon draws with are the system's, and
/// not every one has that glyph, where a browser finds one that does.
fn plain(written: impl std::fmt::Display) -> String {
    written.to_string().replace('\u{202f}', " ")
}

/// A system's name for a locale as BCP 47 names it: `de_DE.UTF-8` and `de_DE@euro` are
/// `de-DE`, `C` and `POSIX` - no locale at all - are English.
fn bcp47(name: &str) -> String {
    let name = name.split(['.', '@']).next().unwrap_or_default();
    match name {
        "" | "C" | "POSIX" => ENGLISH.to_owned(),
        name => name.replace('_', "-"),
    }
}

static CURRENT: RwLock<Option<Arc<Locale>>> = RwLock::new(None);

/// The locale photon writes in from now on.
pub fn set(locale: Locale) {
    let mut current = CURRENT
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *current = Some(Arc::new(locale));
}

/// The locale photon writes in: English until one is set.
pub fn current() -> Arc<Locale> {
    if let Some(locale) = (CURRENT.read())
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
    {
        return locale.clone();
    }
    let english = Arc::new(Locale::english());
    let mut current = CURRENT
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    current.get_or_insert(english).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The Svelte photon's `Intl`, as a browser writes these in English and in German.
    #[test]
    fn dates_are_written_as_the_locale_writes_them() {
        let en = Locale::of("en-US").unwrap();
        assert_eq!(en.month_year(2026, 10).unwrap(), "October 2026");
        assert_eq!(en.day(2026, 10, 4).unwrap(), "Sunday, October 4, 2026");
        assert_eq!(en.day_month(10, 4).unwrap(), "October 4");
        assert_eq!(
            en.date_time((2026, 10, 4), (12, 30)).unwrap(),
            "Oct 4, 2026, 12:30 PM"
        );
        let de = Locale::of("de-DE").unwrap();
        assert_eq!(de.month_year(2026, 10).unwrap(), "Oktober 2026");
        assert_eq!(de.day(2026, 10, 4).unwrap(), "Sonntag, 4. Oktober 2026");
        assert_eq!(de.day_month(10, 4).unwrap(), "4. Oktober");
        assert_eq!(
            de.date_time((2026, 10, 4), (12, 30)).unwrap(),
            "04.10.2026, 12:30"
        );
        // A day no calendar holds is no date.
        assert_eq!(de.day(2026, 2, 30), None);
        assert_eq!(de.day_month(2, 29).unwrap(), "29. Februar");
    }

    #[test]
    fn counts_are_grouped_as_the_locale_groups_them() {
        assert_eq!(Locale::of("en-US").unwrap().count(1_234_567), "1,234,567");
        assert_eq!(Locale::of("de-DE").unwrap().count(1_234_567), "1.234.567");
        assert_eq!(Locale::of("en-US").unwrap().count(999), "999");
        // A French count's narrow space, plainly.
        assert_eq!(Locale::of("fr-FR").unwrap().count(1_234_567), "1 234 567");
    }

    #[test]
    fn a_systems_name_for_a_locale_is_read_as_bcp_47() {
        assert_eq!(bcp47("de_DE.UTF-8"), "de-DE");
        assert_eq!(bcp47("de_DE@euro"), "de-DE");
        assert_eq!(bcp47("C"), "en-US");
        assert_eq!(bcp47("POSIX"), "en-US");
        assert_eq!(bcp47("en-GB"), "en-GB");
        assert!(Locale::of("not a locale at all").is_none());
    }
}
