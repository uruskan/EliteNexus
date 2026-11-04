//! Journal timestamps.
//!
//! Every journal line starts with `"timestamp":"YYYY-MM-DDTHH:MM:SSZ"`. Parsing that with a
//! general-purpose date library is the single hottest thing during a bulk import, so this is a
//! hand-rolled, allocation-free parser that produces whole seconds since the Unix epoch (UTC).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Seconds since the Unix epoch (UTC). Journal timestamps have one-second resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub const ZERO: Timestamp = Timestamp(0);

    pub fn now() -> Self {
        Timestamp(chrono::Utc::now().timestamp())
    }

    /// Parses `YYYY-MM-DDTHH:MM:SS` with an optional fractional part and/or `Z` suffix.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() < 19 {
            return None;
        }
        if b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') || b[13] != b':' || b[16] != b':' {
            return None;
        }
        let num = |from: usize, to: usize| -> Option<i64> {
            let mut v = 0i64;
            for &c in &b[from..to] {
                if !c.is_ascii_digit() {
                    return None;
                }
                v = v * 10 + i64::from(c - b'0');
            }
            Some(v)
        };
        let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
        let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
        if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
            return None;
        }
        Some(Timestamp(days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + se))
    }

    pub fn to_chrono(self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(self.0, 0).unwrap_or_default()
    }

    /// Journal-style ISO 8601 (`2026-10-12T07:30:31Z`), which is also what EDDN/EDSM expect.
    pub fn to_iso(self) -> String {
        self.to_chrono().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    /// Local-time display string.
    pub fn to_local_string(self, fmt_str: &str) -> String {
        self.to_chrono().with_timezone(&chrono::Local).format(fmt_str).to_string()
    }

    /// A calendar day in local time from `YYYY-MM-DD` (a date as the commander reads it): its start and the next
    /// day's start.
    pub fn local_day(date: &str) -> Option<(Self, Self)> {
        let day = chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok()?;
        Some((Timestamp(day_start(day, &chrono::Local)?), Timestamp(day_start(day.succ_opt()?, &chrono::Local)?)))
    }

    pub fn secs_since(self, earlier: Timestamp) -> i64 {
        self.0 - earlier.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// When `date` starts in `tz`, in unix seconds. Midnight can fall in a daylight-saving gap; that day then starts an
/// hour later.
fn day_start<Tz: chrono::TimeZone>(date: chrono::NaiveDate, tz: &Tz) -> Option<i64> {
    let at = |h| date.and_hms_opt(h, 0, 0).and_then(|t| t.and_local_timezone(tz.clone()).earliest());
    at(0).or_else(|| at(1)).map(|t| t.timestamp())
}

/// Sorts instants into the commander's calendar days (local time), numbered as days since 1970-01-01: the slots of a
/// chart that counts per day, whose dates [`LocalDays::label`] writes. A chart's bars, axis and tooltips all go by it,
/// so each day is the date its events happened on the commander's clock. (Counting UTC days and naming each after the
/// local date of its UTC midnight put most of a day under the date before, west of UTC.) It works out a day's bounds
/// once and keeps them rather than asking for the time zone at every instant, which is a system call on Windows, and
/// the charts count again every frame. Instants in order are quickest.
#[derive(Debug, Clone, Default)]
pub struct LocalDays {
    /// The day met last and the instants it covers, `from..to` (none at first).
    day: i64,
    from: i64,
    to: i64,
}

impl LocalDays {
    /// The day `t` falls on.
    pub fn day(&mut self, t: Timestamp) -> i64 {
        self.day_in(t, &chrono::Local)
    }

    fn day_in<Tz: chrono::TimeZone>(&mut self, t: Timestamp, tz: &Tz) -> i64 {
        if !(self.from..self.to).contains(&t.0) {
            let date = t.to_chrono().with_timezone(tz).date_naive();
            self.day = i64::from(date.to_epoch_days());
            // A day lasts 23 or 25 hours where the clocks change; if its bounds don't come out, only `t` is kept.
            (self.from, self.to) = match (day_start(date, tz), date.succ_opt().and_then(|d| day_start(d, tz))) {
                (Some(from), Some(to)) if (from..to).contains(&t.0) => (from, to),
                _ => (t.0, t.0.saturating_add(1)),
            };
        }
        self.day
    }

    /// A day's date, `fmt` as for chrono (`%d %b` writes "03 Nov"); empty for a day outside chrono's calendar.
    pub fn label(day: i64, fmt: &str) -> String {
        i32::try_from(day).ok().and_then(chrono::NaiveDate::from_epoch_days).map(|d| d.format(fmt).to_string()).unwrap_or_default()
    }

    /// Where `t` sits on a chart whose x axis counts these days: its day plus the share of that day gone by, so whole
    /// numbers fall on the commander's midnights, under the dates [`LocalDays::label`] writes. A day that lasts 23 or 25
    /// hours, where the clocks change, is still one unit wide.
    pub fn at(&mut self, t: Timestamp) -> f64 {
        self.at_in(t, &chrono::Local)
    }

    fn at_in<Tz: chrono::TimeZone>(&mut self, t: Timestamp, tz: &Tz) -> f64 {
        let day = self.day_in(t, tz);
        day as f64 + (t.0 - self.from) as f64 / (self.to - self.from) as f64
    }

    /// The instant at `x` on such an axis (the inverse of [`LocalDays::at`]), for a chart's hover label.
    pub fn instant(x: f64) -> Timestamp {
        Self::instant_in(x, &chrono::Local)
    }

    fn instant_in<Tz: chrono::TimeZone>(x: f64, tz: &Tz) -> Timestamp {
        let day = x.floor();
        let date = i32::try_from(day as i64).ok().and_then(chrono::NaiveDate::from_epoch_days);
        match date.and_then(|d| Some((day_start(d, tz)?, day_start(d.succ_opt()?, tz)?))) {
            Some((from, to)) => Timestamp(from + ((x - day) * (to - from) as f64).round() as i64),
            None => Timestamp((x * 86_400.0) as i64),
        }
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_iso())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_iso())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Timestamp;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an ISO-8601 timestamp string or unix seconds")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Timestamp, E> {
                Ok(Timestamp::parse(v).unwrap_or_default())
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Timestamp, E> {
                Ok(Timestamp(v))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Timestamp, E> {
                Ok(Timestamp(v as i64))
            }
        }
        d.deserialize_any(V)
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_journal_timestamps() {
        let t = Timestamp::parse("2026-09-05T00:01:46Z").unwrap();
        assert_eq!(t.to_iso(), "2026-09-05T00:01:46Z");
        assert_eq!(Timestamp::parse("1970-01-01T00:00:00Z").unwrap().0, 0);
        assert_eq!(Timestamp::parse("2000-03-01T00:00:00Z").unwrap().0, 951_868_800);
        assert_eq!(Timestamp::parse("2024-02-29T12:34:56.789Z").unwrap().to_iso(), "2024-02-29T12:34:56Z");
        assert!(Timestamp::parse("garbage").is_none());
        assert!(Timestamp::parse("2026-13-05T00:01:46Z").is_none());
    }

    #[test]
    fn local_days() {
        let (start, end) = Timestamp::local_day("2026-10-22").unwrap();
        assert_eq!(start.to_local_string("%Y-%m-%d %H:%M"), "2026-10-22 00:00");
        assert_eq!(end.to_local_string("%Y-%m-%d %H:%M"), "2026-10-23 00:00");
        assert!((82_800..=90_000).contains(&(end.0 - start.0)), "a day, give or take a daylight-saving hour");
        assert!(Timestamp::local_day("22/10/2026").is_none());
    }

    #[test]
    fn days_as_the_commanders_clock_shows_them() {
        let at = |s: &str| Timestamp::parse(s).unwrap();
        // 02:00 UTC on 3 November is the evening of the 2nd in New York (UTC-5) and the morning of the 3rd in
        // Istanbul (UTC+3): each counts it on its own date, and the label writes that date. Noon UTC is the morning of
        // the 3rd in New York, which UTC days named after the local date of their midnight called the 2nd.
        let new_york = chrono::FixedOffset::west_opt(5 * 3_600).unwrap();
        let istanbul = chrono::FixedOffset::east_opt(3 * 3_600).unwrap();
        let t = at("2026-11-03T02:00:00Z");
        assert_eq!(LocalDays::label(LocalDays::default().day_in(t, &new_york), "%d %b %Y"), "02 Nov 2026");
        assert_eq!(LocalDays::label(LocalDays::default().day_in(t, &istanbul), "%d %b %Y"), "03 Nov 2026");
        assert_eq!(LocalDays::label(LocalDays::default().day_in(at("2026-11-03T12:00:00Z"), &new_york), "%d %b"), "03 Nov");
        // A day runs from local midnight to local midnight, whichever order the instants come in.
        let nov2 = at("2026-11-02T00:00:00Z").0 / 86_400;
        let mut days = LocalDays::default();
        let seen: Vec<i64> = ["2026-11-02T05:00:00Z", "2026-11-03T04:59:59Z", "2026-11-03T05:00:00Z", "2026-11-02T04:59:59Z", "2026-11-02T12:00:00Z"]
            .into_iter()
            .map(|s| days.day_in(at(s), &new_york))
            .collect();
        assert_eq!(seen, [nov2, nov2, nov2 + 1, nov2 - 1, nov2]);
        // On this computer's clock, a day's label is the date its instants have there.
        let mut days = LocalDays::default();
        for s in ["2026-01-15T00:30:00Z", "2026-07-01T23:30:00Z", "2026-11-04T12:00:00Z"] {
            let (from, to) = Timestamp::local_day(&LocalDays::label(days.day(at(s)), "%Y-%m-%d")).unwrap();
            assert!(from <= at(s) && at(s) < to, "{s}");
        }
        assert_eq!(LocalDays::label(i64::MAX, "%d %b"), "");
    }

    #[test]
    fn a_days_axis_puts_midnight_under_its_date() {
        let at = |s: &str| Timestamp::parse(s).unwrap();
        let new_york = chrono::FixedOffset::west_opt(5 * 3_600).unwrap();
        let nov3 = (at("2026-11-03T00:00:00Z").0 / 86_400) as f64;
        // Midnight in New York is 05:00 UTC: whole on the axis, under "03 Nov". Unix seconds over 86,400 put UTC
        // midnight there, the evening of the 2nd in New York, under the label "02 Nov": a day early.
        let mut days = LocalDays::default();
        assert_eq!(days.at_in(at("2026-11-03T05:00:00Z"), &new_york), nov3);
        assert_eq!(LocalDays::label(nov3 as i64, "%d %b"), "03 Nov");
        assert_eq!(days.at_in(at("2026-11-03T17:00:00Z"), &new_york), nov3 + 0.5, "noon in New York");
        assert_eq!(days.at_in(at("2026-11-03T04:00:00Z"), &new_york), nov3 - 1.0 + 23.0 / 24.0, "23:00 on the 2nd");
        // The hover label reads the instant back.
        for s in ["2026-11-03T05:00:00Z", "2026-11-03T17:00:00Z", "2026-11-04T04:59:59Z"] {
            assert_eq!(LocalDays::instant_in(days.at_in(at(s), &new_york), &new_york), at(s), "{s}");
        }
        // On this computer's clock too, whichever day the clocks change on.
        let mut days = LocalDays::default();
        for s in ["2026-03-29T00:30:00Z", "2026-10-25T01:30:00Z", "2026-11-04T12:00:00Z"] {
            assert_eq!(LocalDays::instant(days.at(at(s))), at(s), "{s}");
            assert_eq!(days.at(at(s)).floor() as i64, days.day(at(s)), "{s}");
        }
    }
}
