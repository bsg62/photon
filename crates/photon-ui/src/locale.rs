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
    /// not one, or is one ICU4X has no words of its own for. ICU4X falls back from a
    /// region to its language, and from a language it has nothing of to the root - "2026
    /// M10 4", which is nobody's way of writing a date - where a browser falls back to its
    /// own default: none here, and the caller takes English.
    pub fn of(tag: &str) -> Option<Self> {
        let made = Self::made(tag)?;
        let root = Self::made("und")?;
        let probe = |locale: &Self| locale.month_year((2026, 1, 15));
        (made.tag == "und" || probe(&made) != probe(&root)).then_some(made)
    }

    fn made(tag: &str) -> Option<Self> {
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

    /// The month of a year a day stands in: "October 2026", "Oktober 2026". Of that day,
    /// and not of the month's first: a locale whose calendar is not the Gregorian one -
    /// Persian, where a month turns round the 21st - puts other days of one Gregorian
    /// month in another of its own.
    pub fn month_year(&self, (year, month, day): (i32, u8, u8)) -> Option<String> {
        let date = Date::try_new_iso(year, month, day).ok()?;
        Some(plain(self.month_year.format(&date)))
    }

    /// A day, with its weekday: "Sunday, October 4, 2026", "Sonntag, 4. Oktober 2026".
    pub fn day(&self, year: i32, month: u8, day: u8) -> Option<String> {
        let date = Date::try_new_iso(year, month, day).ok()?;
        Some(plain(self.day.format(&date)))
    }

    /// A day's day and month: "October 4", "4. Oktober". Of the real day, year and all:
    /// in another calendar the same Gregorian day and month fall on another day of its
    /// own from year to year.
    pub fn day_month(&self, (year, month, day): (i32, u8, u8)) -> Option<String> {
        let date = Date::try_new_iso(year, month, day).ok()?;
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

#[cfg(test)]
thread_local! {
    static THIS_THREAD: std::cell::RefCell<Option<Arc<Locale>>> =
        const { std::cell::RefCell::new(None) };
}

/// A test's own locale, on its own thread: the process's would be every test's, and under
/// `cargo test`, whose tests share a process, one test's German would be another's.
#[cfg(test)]
pub(crate) fn for_this_thread(locale: Option<Locale>) {
    THIS_THREAD.with(|this| *this.borrow_mut() = locale.map(Arc::new));
}

/// The locale photon writes in: English until one is set.
pub fn current() -> Arc<Locale> {
    #[cfg(test)]
    if let Some(locale) = THIS_THREAD.with(|this| this.borrow().clone()) {
        return locale;
    }
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
        assert_eq!(en.month_year((2026, 10, 4)).unwrap(), "October 2026");
        assert_eq!(en.day(2026, 10, 4).unwrap(), "Sunday, October 4, 2026");
        assert_eq!(en.day_month((2026, 10, 4)).unwrap(), "October 4");
        assert_eq!(
            en.date_time((2026, 10, 4), (12, 30)).unwrap(),
            "Oct 4, 2026, 12:30 PM"
        );
        let de = Locale::of("de-DE").unwrap();
        assert_eq!(de.month_year((2026, 10, 4)).unwrap(), "Oktober 2026");
        assert_eq!(de.day(2026, 10, 4).unwrap(), "Sonntag, 4. Oktober 2026");
        assert_eq!(de.day_month((2026, 10, 4)).unwrap(), "4. Oktober");
        assert_eq!(
            de.date_time((2026, 10, 4), (12, 30)).unwrap(),
            "04.10.2026, 12:30"
        );
        // A day no calendar holds is no date.
        assert_eq!(de.day(2026, 2, 30), None);
        assert_eq!(de.day_month((2024, 2, 29)).unwrap(), "29. Februar");
    }

    // A locale ICU4X has no words for is no locale here, and the caller writes English:
    // ICU4X's own fallback, the root's "2026 M10", is nobody's.
    #[test]
    fn a_locale_icu4x_has_nothing_of_is_none() {
        assert!(Locale::of("xx-YY").is_none());
        assert!(Locale::of("la").is_none());
        assert!(
            Locale::of("de-AT").is_some(),
            "a region of a language it has"
        );
    }

    // A calendar that is not the Gregorian one names a day's own month and day: in the
    // Persian, the 25th of October 2026 is in Aban and the 4th in Mehr, and a day and
    // month fall elsewhere from year to year (a review found both written from a month's
    // first day, or a year 2000).
    #[test]
    fn a_month_and_a_day_are_of_the_real_day_in_another_calendar() {
        let fa = Locale::of("fa-IR").unwrap();
        assert_ne!(fa.month_year((2026, 10, 4)), fa.month_year((2026, 10, 25)));
        assert_ne!(fa.day_month((2026, 3, 20)), fa.day_month((2000, 3, 20)));
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
