use crate::{KronroeError, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

const MICROS_PER_MILLI: i64 = 1_000;
const MICROS_PER_SECOND: i64 = 1_000_000;
const MICROS_PER_MINUTE: i64 = 60 * MICROS_PER_SECOND;
const MICROS_PER_HOUR: i64 = 60 * MICROS_PER_MINUTE;
const MICROS_PER_DAY: i64 = 24 * MICROS_PER_HOUR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KronroeTimestamp {
    unix_micros: i64,
}

impl KronroeTimestamp {
    pub const MIN: Self = Self {
        unix_micros: i64::MIN,
    };
    pub const MAX: Self = Self {
        unix_micros: i64::MAX,
    };

    pub fn now(clock: &impl KronroeClock) -> Self {
        clock.now()
    }

    pub fn now_utc() -> Self {
        default_clock().now()
    }

    pub fn from_unix_micros(unix_micros: i64) -> Result<Self> {
        Ok(Self { unix_micros })
    }

    pub fn from_unix_millis(unix_millis: i64) -> Result<Self> {
        let unix_micros = unix_millis.checked_mul(MICROS_PER_MILLI).ok_or_else(|| {
            KronroeError::Internal("timestamp overflows microsecond range".into())
        })?;
        Self::from_unix_micros(unix_micros)
    }

    pub fn to_unix_micros(self) -> i64 {
        self.unix_micros
    }

    pub fn to_unix_millis(self) -> i64 {
        self.unix_micros / MICROS_PER_MILLI
    }

    pub fn parse_rfc3339(input: &str) -> Result<Self> {
        parse_rfc3339_timestamp(input)
    }

    pub fn to_rfc3339_z(self) -> String {
        format_rfc3339_z(self.unix_micros)
    }

    pub fn to_rfc3339(self) -> String {
        self.to_rfc3339_z()
    }

    pub fn date_ymd(self) -> String {
        let days = self.unix_micros.div_euclid(MICROS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        format!("{year:04}-{month:02}-{day:02}")
    }

    pub fn from_utc_components(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        micros: u32,
    ) -> Result<Self> {
        validate_date(year, month, day)?;
        validate_time(hour, minute, second, micros)?;
        let days = days_from_civil(year, month, day);
        let day_micros = (hour as i64)
            .checked_mul(MICROS_PER_HOUR)
            .and_then(|v| v.checked_add((minute as i64) * MICROS_PER_MINUTE))
            .and_then(|v| v.checked_add((second as i64) * MICROS_PER_SECOND))
            .and_then(|v| v.checked_add(micros as i64))
            .ok_or_else(|| {
                KronroeError::Internal("timestamp overflows microsecond range".into())
            })?;
        let unix_micros = days
            .checked_mul(MICROS_PER_DAY)
            .and_then(|v| v.checked_add(day_micros))
            .ok_or_else(|| {
                KronroeError::Internal("timestamp overflows microsecond range".into())
            })?;
        Ok(Self { unix_micros })
    }
}

impl fmt::Display for KronroeTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339_z())
    }
}

impl std::str::FromStr for KronroeTimestamp {
    type Err = KronroeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse_rfc3339(s)
    }
}

impl Serialize for KronroeTimestamp {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_rfc3339_z())
    }
}

impl<'de> Deserialize<'de> for KronroeTimestamp {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        KronroeTimestamp::parse_rfc3339(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KronroeSpan {
    micros: i64,
}

impl KronroeSpan {
    pub fn microseconds(micros: i64) -> Self {
        Self { micros }
    }

    pub fn milliseconds(millis: i64) -> Self {
        Self {
            micros: millis.saturating_mul(MICROS_PER_MILLI),
        }
    }

    pub fn seconds(seconds: i64) -> Self {
        Self {
            micros: seconds.saturating_mul(MICROS_PER_SECOND),
        }
    }

    pub fn minutes(minutes: i64) -> Self {
        Self {
            micros: minutes.saturating_mul(MICROS_PER_MINUTE),
        }
    }

    pub fn hours(hours: i64) -> Self {
        Self {
            micros: hours.saturating_mul(MICROS_PER_HOUR),
        }
    }

    pub fn days(days: i64) -> Self {
        Self {
            micros: days.saturating_mul(MICROS_PER_DAY),
        }
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        self.micros
            .checked_add(rhs.micros)
            .map(Self::microseconds)
            .ok_or_else(|| KronroeError::Internal("timespan overflow".into()))
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        self.micros
            .checked_sub(rhs.micros)
            .map(Self::microseconds)
            .ok_or_else(|| KronroeError::Internal("timespan overflow".into()))
    }

    pub fn as_micros(self) -> i64 {
        self.micros
    }

    pub fn as_seconds_f64(self) -> f64 {
        self.micros as f64 / MICROS_PER_SECOND as f64
    }

    pub fn num_seconds(self) -> i64 {
        self.micros / MICROS_PER_SECOND
    }

    pub fn num_days(self) -> i64 {
        self.micros / MICROS_PER_DAY
    }
}

impl Add<KronroeSpan> for KronroeTimestamp {
    type Output = KronroeTimestamp;

    fn add(self, rhs: KronroeSpan) -> Self::Output {
        KronroeTimestamp {
            unix_micros: self.unix_micros.saturating_add(rhs.micros),
        }
    }
}

impl AddAssign<KronroeSpan> for KronroeTimestamp {
    fn add_assign(&mut self, rhs: KronroeSpan) {
        self.unix_micros = self.unix_micros.saturating_add(rhs.micros);
    }
}

impl Sub<KronroeSpan> for KronroeTimestamp {
    type Output = KronroeTimestamp;

    fn sub(self, rhs: KronroeSpan) -> Self::Output {
        KronroeTimestamp {
            unix_micros: self.unix_micros.saturating_sub(rhs.micros),
        }
    }
}

impl SubAssign<KronroeSpan> for KronroeTimestamp {
    fn sub_assign(&mut self, rhs: KronroeSpan) {
        self.unix_micros = self.unix_micros.saturating_sub(rhs.micros);
    }
}

impl Sub<KronroeTimestamp> for KronroeTimestamp {
    type Output = KronroeSpan;

    fn sub(self, rhs: KronroeTimestamp) -> Self::Output {
        KronroeSpan::microseconds(self.unix_micros.saturating_sub(rhs.unix_micros))
    }
}

pub trait KronroeClock {
    fn now(&self) -> KronroeTimestamp;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl KronroeClock for SystemClock {
    fn now(&self) -> KronroeTimestamp {
        #[cfg(target_arch = "wasm32")]
        {
            let millis = js_sys::Date::new_0().get_time();
            let micros = (millis * MICROS_PER_MILLI as f64).round() as i64;
            KronroeTimestamp {
                unix_micros: micros,
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::time::{SystemTime, UNIX_EPOCH};

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            let micros =
                now.as_secs().saturating_mul(1_000_000) as i64 + i64::from(now.subsec_micros());
            KronroeTimestamp {
                unix_micros: micros,
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    now: KronroeTimestamp,
}

impl FixedClock {
    pub fn new(now: KronroeTimestamp) -> Self {
        Self { now }
    }
}

impl KronroeClock for FixedClock {
    fn now(&self) -> KronroeTimestamp {
        self.now
    }
}

pub fn default_clock() -> SystemClock {
    SystemClock
}

fn parse_rfc3339_timestamp(input: &str) -> Result<KronroeTimestamp> {
    let date_time_sep = input
        .find('T')
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 timestamp: {input}")))?;
    let (date, rest) = input.split_at(date_time_sep);
    let rest = &rest[1..];

    let (time_part, offset_sign, offset_part) = if let Some(stripped) = rest.strip_suffix('Z') {
        (stripped, None, None)
    } else if let Some(index) = rest.rfind(['+', '-']) {
        let (time, offset) = rest.split_at(index);
        let sign = offset
            .chars()
            .next()
            .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 timestamp: {input}")))?;
        (time, Some(sign), Some(&offset[1..]))
    } else {
        return Err(KronroeError::Internal(format!(
            "invalid RFC3339 timestamp: {input}"
        )));
    };

    let (year, month, day) = parse_date(date)?;
    let (hour, minute, second, micros) = parse_time(time_part)?;
    let offset_micros = match (offset_sign, offset_part) {
        (None, None) => 0,
        (Some(sign), Some(offset)) => parse_offset(sign, offset)?,
        _ => {
            return Err(KronroeError::Internal(format!(
                "invalid RFC3339 timestamp: {input}"
            )))
        }
    };

    let local =
        KronroeTimestamp::from_utc_components(year, month, day, hour, minute, second, micros)?;
    Ok(KronroeTimestamp {
        unix_micros: local.unix_micros.saturating_sub(offset_micros),
    })
}

fn parse_date(date: &str) -> Result<(i32, u32, u32)> {
    let mut parts = date.split('-');
    let year = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 date: {date}")))?
        .parse::<i32>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 date: {date}")))?;
    let month = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 date: {date}")))?
        .parse::<u32>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 date: {date}")))?;
    let day = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 date: {date}")))?
        .parse::<u32>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 date: {date}")))?;
    if parts.next().is_some() {
        return Err(KronroeError::Internal(format!(
            "invalid RFC3339 date: {date}"
        )));
    }
    validate_date(year, month, day)?;
    Ok((year, month, day))
}

fn parse_time(time: &str) -> Result<(u32, u32, u32, u32)> {
    let (base, fraction) = match time.split_once('.') {
        Some((base, fraction)) => (base, Some(fraction)),
        None => (time, None),
    };
    let mut parts = base.split(':');
    let hour = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 time: {time}")))?
        .parse::<u32>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 time: {time}")))?;
    let minute = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 time: {time}")))?
        .parse::<u32>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 time: {time}")))?;
    let second = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 time: {time}")))?
        .parse::<u32>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 time: {time}")))?;
    if parts.next().is_some() {
        return Err(KronroeError::Internal(format!(
            "invalid RFC3339 time: {time}"
        )));
    }

    let micros = match fraction {
        None => 0,
        Some(raw) => {
            if raw.is_empty() || raw.len() > 6 || !raw.chars().all(|c| c.is_ascii_digit()) {
                return Err(KronroeError::Internal(format!(
                    "invalid RFC3339 fractional seconds: {time}"
                )));
            }
            let padded = format!("{raw:0<6}");
            padded
                .parse::<u32>()
                .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 time: {time}")))?
        }
    };

    validate_time(hour, minute, second, micros)?;
    Ok((hour, minute, second, micros))
}

fn parse_offset(sign: char, offset: &str) -> Result<i64> {
    let mut parts = offset.split(':');
    let hours = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 offset: {offset}")))?
        .parse::<i64>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 offset: {offset}")))?;
    let minutes = parts
        .next()
        .ok_or_else(|| KronroeError::Internal(format!("invalid RFC3339 offset: {offset}")))?
        .parse::<i64>()
        .map_err(|_| KronroeError::Internal(format!("invalid RFC3339 offset: {offset}")))?;
    if parts.next().is_some() || hours > 23 || minutes > 59 {
        return Err(KronroeError::Internal(format!(
            "invalid RFC3339 offset: {offset}"
        )));
    }
    let sign = match sign {
        '+' => 1_i64,
        '-' => -1_i64,
        _ => {
            return Err(KronroeError::Internal(format!(
                "invalid RFC3339 offset sign: {sign}"
            )))
        }
    };
    Ok(sign * (hours * MICROS_PER_HOUR + minutes * MICROS_PER_MINUTE))
}

fn validate_date(year: i32, month: u32, day: u32) -> Result<()> {
    if !(1..=12).contains(&month) {
        return Err(KronroeError::Internal(format!(
            "invalid RFC3339 month: {month}"
        )));
    }
    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        return Err(KronroeError::Internal(format!(
            "invalid RFC3339 day: {day}"
        )));
    }
    Ok(())
}

fn validate_time(hour: u32, minute: u32, second: u32, micros: u32) -> Result<()> {
    if hour > 23 || minute > 59 || second > 59 || micros > 999_999 {
        return Err(KronroeError::Internal("invalid RFC3339 time".into()));
    }
    Ok(())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn format_rfc3339_z(unix_micros: i64) -> String {
    let days = unix_micros.div_euclid(MICROS_PER_DAY);
    let micros_of_day = unix_micros.rem_euclid(MICROS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = micros_of_day / MICROS_PER_HOUR;
    let minute = (micros_of_day % MICROS_PER_HOUR) / MICROS_PER_MINUTE;
    let second = (micros_of_day % MICROS_PER_MINUTE) / MICROS_PER_SECOND;
    let micros = micros_of_day % MICROS_PER_SECOND;
    if micros == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        let mut frac = format!("{micros:06}");
        while frac.ends_with('0') {
            frac.pop();
        }
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{frac}Z")
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as i64
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i32::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes_offset_timestamp() {
        let ts = KronroeTimestamp::parse_rfc3339("2026-03-22T09:00:00+01:00").unwrap();
        assert_eq!(ts.to_rfc3339_z(), "2026-03-22T08:00:00Z");
    }

    #[test]
    fn trims_fractional_zeros() {
        let ts = KronroeTimestamp::parse_rfc3339("2026-03-22T08:00:00.123400Z").unwrap();
        assert_eq!(ts.to_rfc3339_z(), "2026-03-22T08:00:00.1234Z");
    }

    #[test]
    fn round_trips_microseconds() {
        let ts = KronroeTimestamp::parse_rfc3339("2026-03-22T08:00:00.123456Z").unwrap();
        assert_eq!(ts.to_rfc3339_z(), "2026-03-22T08:00:00.123456Z");
    }

    #[test]
    fn rejects_invalid_timestamp() {
        assert!(KronroeTimestamp::parse_rfc3339("2026-03-22").is_err());
        assert!(KronroeTimestamp::parse_rfc3339("2026-03-22T08:00:60Z").is_err());
        assert!(KronroeTimestamp::parse_rfc3339("2026-03-22T08:00:00").is_err());
    }
}
