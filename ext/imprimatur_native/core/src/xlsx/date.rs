//! Dates as Excel stores them: a number of days since 1900-01-00, with the
//! time of day as the fraction. No time zone; the wall-clock fields are what
//! the cell shows.

#[cfg(test)]
mod tests;

use std::fmt;

use crate::{Error, Result};

const SECONDS_PER_DAY: f64 = 86_400.0;

/// A date, optionally with a time of day, between 1900-01-01 and 9999-12-31.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateTime {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    nanosecond: u32,
}

impl DateTime {
    /// Midnight on a calendar date.
    ///
    /// # Errors
    /// [`Error::DateOutOfRange`] for an invalid date or one Excel cannot hold.
    pub fn date(year: u16, month: u8, day: u8) -> Result<Self> {
        let date = Self { year, month, day, hour: 0, minute: 0, second: 0, nanosecond: 0 };
        let valid = (1900..=9999).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month);
        valid.ok_or_else(|| Error::DateOutOfRange(date.to_string()))?;
        Ok(date)
    }

    /// The same date at a time of day. Excel shows milliseconds at most.
    ///
    /// # Errors
    /// [`Error::DateOutOfRange`] for an hour, minute, second or nanosecond
    /// out of range (leap seconds included).
    pub fn at(self, hour: u8, minute: u8, second: u8, nanosecond: u32) -> Result<Self> {
        let time = Self { hour, minute, second, nanosecond, ..self };
        (hour < 24 && minute < 60 && second < 60 && nanosecond < 1_000_000_000)
            .ok_or_else(|| Error::DateOutOfRange(time.to_string()))?;
        Ok(time)
    }

    /// The date a serial number stands for, to the second: the inverse of
    /// [`serial`](Self::serial). `date1904` is the Mac epoch some workbooks
    /// use, where 0 is 1904-01-01. `None` outside 1900-01-01 to 9999-12-31,
    /// and for 60, the 1900-02-29 that never existed.
    #[must_use]
    pub fn from_serial(serial: f64, date1904: bool) -> Option<Self> {
        if !serial.is_finite() || serial < 0.0 {
            return None;
        }
        #[expect(clippy::cast_possible_truncation, reason = "checked against 9999-12-31 below")]
        let seconds = (serial * SECONDS_PER_DAY).round() as i64;
        let (mut days, seconds) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
        days += if date1904 {
            days_from_civil(1904, 1, 1)
        } else if days == 60 {
            return None;
        } else if days > 60 {
            days_from_civil(1899, 12, 30)
        } else {
            days_from_civil(1899, 12, 31)
        };
        let (year, month, day) = civil_from_days(days);
        let (Ok(year), Ok(month), Ok(day)) =
            (u16::try_from(year), u8::try_from(month), u8::try_from(day))
        else {
            return None;
        };
        let (hour, minute, second) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
        #[expect(clippy::cast_possible_truncation, reason = "a time of day: 0 to 59, or 23")]
        #[expect(clippy::cast_sign_loss, reason = "rem_euclid is never negative")]
        let time = (hour as u8, minute as u8, second as u8);
        Self::date(year, month, day).ok()?.at(time.0, time.1, time.2, 0).ok()
    }

    /// The same day at midnight.
    #[must_use]
    pub const fn date_only(self) -> Self {
        Self { hour: 0, minute: 0, second: 0, nanosecond: 0, ..self }
    }

    /// Hours, minutes and seconds.
    #[must_use]
    pub const fn time_of_day(&self) -> (u8, u8, u8) {
        (self.hour, self.minute, self.second)
    }

    /// Whether the time of day is past midnight.
    #[must_use]
    pub const fn has_time(&self) -> bool {
        self.hour != 0 || self.minute != 0 || self.second != 0 || self.nanosecond != 0
    }

    /// The Excel serial number: 1 is 1900-01-01, 45000 is 2023-03-15.
    #[must_use]
    pub fn serial(&self) -> f64 {
        let (year, month, day) = (i64::from(self.year), i64::from(self.month), i64::from(self.day));
        let mut days = days_from_civil(year, month, day) - days_from_civil(1899, 12, 31);
        // Excel keeps Lotus 1-2-3's 1900-02-29, which never existed: every
        // later date is one day further along.
        if (self.year, self.month) >= (1900, 3) {
            days += 1;
        }
        let seconds =
            u32::from(self.hour) * 3600 + u32::from(self.minute) * 60 + u32::from(self.second);
        let fraction = (f64::from(seconds) + f64::from(self.nanosecond) / 1e9) / SECONDS_PER_DAY;
        #[expect(clippy::cast_precision_loss, reason = "at most 2958465 days, far below 2**53")]
        let days = days as f64;
        days + fraction
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)?;
        if self.has_time() {
            write!(f, " {:02}:{:02}:{:02}", self.hour, self.minute, self.second)?;
            if self.nanosecond != 0 {
                write!(f, ".{:09}", self.nanosecond)?;
            }
        }
        Ok(())
    }
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The calendar date `days` after 1970-01-01: the inverse of
/// [`days_from_civil`] (Howard Hinnant's `civil_from_days`).
const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// Days from 1970-01-01 in the proleptic Gregorian calendar (Howard
/// Hinnant's `days_from_civil`).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
