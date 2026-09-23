//! Calendar arithmetic for ZIP DOS timestamps and RFC 3339 strings.
//!
//! Two call sites do not justify a date crate: a UTC civil date from a Unix
//! time is the days-to-civil algorithm, a dozen lines.

use std::time::{SystemTime, UNIX_EPOCH};

/// UTC civil time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Civil {
    pub year: i64,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Civil UTC time of a Unix timestamp (seconds).
pub(crate) fn civil(unix: i64) -> Civil {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Days since 1970-01-01 to (y, m, d) in the proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Civil {
        year,
        month: month as u8,
        day: day as u8,
        hour: (secs / 3600) as u8,
        minute: (secs / 60 % 60) as u8,
        second: (secs % 60) as u8,
    }
}

fn unix_secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX / 2),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX / 2),
    }
}

/// A ZIP DOS timestamp in UTC, clamped to the representable 1980–2107.
pub(crate) fn dos_datetime(t: SystemTime) -> zip::DateTime {
    let c = civil(unix_secs(t));
    let clamped = if c.year < 1980 {
        return zip::DateTime::default();
    } else if c.year > 2107 {
        (2107, 12, 31, 23, 59, 58)
    } else {
        (c.year as u16, c.month, c.day, c.hour, c.minute, c.second)
    };
    zip::DateTime::from_date_and_time(
        clamped.0, clamped.1, clamped.2, clamped.3, clamped.4, clamped.5,
    )
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn known_dates() {
        let at = |s: u64| UNIX_EPOCH + Duration::from_secs(s);
        assert_eq!(dos_datetime(at(0)), zip::DateTime::default());
        let d = dos_datetime(at(1_789_837_331));
        assert_eq!(
            (d.year(), d.month(), d.day(), d.hour(), d.minute()),
            (2026, 9, 19, 17, 2)
        );
    }
}
