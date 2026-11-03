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
        let start = |d: chrono::NaiveDate| {
            let at = |h| d.and_hms_opt(h, 0, 0).and_then(|t| t.and_local_timezone(chrono::Local).earliest());
            // Midnight can fall in a daylight-saving gap; that day then starts an hour later.
            at(0).or_else(|| at(1)).map(|t| Timestamp(t.timestamp()))
        };
        Some((start(day)?, start(day.succ_opt()?)?))
    }

    pub fn secs_since(self, earlier: Timestamp) -> i64 {
        self.0 - earlier.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
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
}
