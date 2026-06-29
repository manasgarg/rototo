use std::net::IpAddr;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Rfc3339Timestamp {
    seconds: i128,
    nanos: u32,
}

pub(crate) fn parse_rfc3339_timestamp(value: &str) -> Option<Rfc3339Timestamp> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return None;
    }

    let year = parse_digits(bytes, 0, 4)? as i32;
    expect(bytes, 4, b'-')?;
    let month = parse_digits(bytes, 5, 2)?;
    expect(bytes, 7, b'-')?;
    let day = parse_digits(bytes, 8, 2)?;
    if !matches!(bytes.get(10), Some(b'T' | b't')) {
        return None;
    }
    let hour = parse_digits(bytes, 11, 2)?;
    expect(bytes, 13, b':')?;
    let minute = parse_digits(bytes, 14, 2)?;
    expect(bytes, 16, b':')?;
    let second = parse_digits(bytes, 17, 2)?;

    if !valid_date(year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let mut at = 19;
    let mut nanos = 0;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        let mut value = 0_u32;
        while let Some(byte) = bytes.get(at) {
            if !byte.is_ascii_digit() {
                break;
            }
            if at - start >= 9 {
                return None;
            }
            value = value * 10 + u32::from(byte - b'0');
            at += 1;
        }
        if at == start {
            return None;
        }
        for _ in 0..(9 - (at - start)) {
            value *= 10;
        }
        nanos = value;
    }

    let offset_seconds = match bytes.get(at) {
        Some(b'Z' | b'z') if at + 1 == bytes.len() => 0_i32,
        Some(b'+' | b'-') if at + 6 == bytes.len() => {
            let sign = if bytes[at] == b'+' { 1 } else { -1 };
            let offset_hour = parse_digits(bytes, at + 1, 2)? as i32;
            expect(bytes, at + 3, b':')?;
            let offset_minute = parse_digits(bytes, at + 4, 2)? as i32;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            sign * (offset_hour * 3600 + offset_minute * 60)
        }
        _ => return None,
    };

    let days = days_from_civil(year, month, day);
    let local_seconds = i128::from(days) * 86_400
        + i128::from(hour) * 3_600
        + i128::from(minute) * 60
        + i128::from(second);
    Some(Rfc3339Timestamp {
        seconds: local_seconds - i128::from(offset_seconds),
        nanos,
    })
}

fn parse_digits(bytes: &[u8], start: usize, len: usize) -> Option<u32> {
    let mut value = 0_u32;
    for byte in bytes.get(start..start + len)? {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}

fn expect(bytes: &[u8], index: usize, expected: u8) -> Option<()> {
    (bytes.get(index) == Some(&expected)).then_some(())
}

fn valid_date(year: i32, month: u32, day: u32) -> bool {
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=max_day).contains(&day)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Inverse of [`days_from_civil`] (Howard Hinnant's civil-from-days). Returns the
/// proleptic Gregorian `(year, month, day)` for a count of days since
/// 1970-01-01.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month, day)
}

/// The current time as an RFC3339 UTC string (`...Z`), the form rototo injects as
/// `env.now` and the time functions parse. Captured once per resolution so every
/// `env.now` reference in one resolution sees the same instant.
pub(crate) fn now_rfc3339() -> String {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format_rfc3339_utc(since_epoch.as_secs() as i64, since_epoch.subsec_nanos())
}

/// Format `seconds` since the Unix epoch (plus `nanos`) as an RFC3339 UTC string.
/// Fractional seconds are emitted only when non-zero, with trailing zeros trimmed.
fn format_rfc3339_utc(seconds: i64, nanos: u32) -> String {
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    let mut stamp = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    if nanos != 0 {
        let fraction = format!("{nanos:09}");
        stamp.push('.');
        stamp.push_str(fraction.trim_end_matches('0'));
    }
    stamp.push('Z');
    stamp
}

#[derive(Clone, Debug)]
pub(crate) enum CidrBlock {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

impl CidrBlock {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let (addr, prefix) = match value.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix.parse::<u8>().ok()?)),
            None => (value, None),
        };
        let addr = addr.parse::<IpAddr>().ok()?;
        match addr {
            IpAddr::V4(addr) => {
                let prefix = prefix.unwrap_or(32);
                if prefix > 32 {
                    return None;
                }
                let mask = prefix_mask_v4(prefix);
                Some(Self::V4 {
                    network: u32::from(addr) & mask,
                    prefix,
                })
            }
            IpAddr::V6(addr) => {
                let prefix = prefix.unwrap_or(128);
                if prefix > 128 {
                    return None;
                }
                let mask = prefix_mask_v6(prefix);
                Some(Self::V6 {
                    network: u128::from(addr) & mask,
                    prefix,
                })
            }
        }
    }

    pub(crate) fn contains(&self, value: IpAddr) -> bool {
        match (self, value) {
            (Self::V4 { network, prefix }, IpAddr::V4(value)) => {
                let mask = prefix_mask_v4(*prefix);
                (u32::from(value) & mask) == *network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(value)) => {
                let mask = prefix_mask_v6(*prefix);
                (u128::from(value) & mask) == *network
            }
            _ => false,
        }
    }
}

fn prefix_mask_v4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn prefix_mask_v6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_rfc3339_utc() {
        assert_eq!(format_rfc3339_utc(0, 0), "1970-01-01T00:00:00Z");
        // 2026-06-29T12:34:56Z.
        assert_eq!(format_rfc3339_utc(1_782_736_496, 0), "2026-06-29T12:34:56Z");
        // Fractional seconds keep only the significant digits.
        assert_eq!(
            format_rfc3339_utc(1_782_736_496, 500_000_000),
            "2026-06-29T12:34:56.5Z"
        );
    }

    #[test]
    fn format_round_trips_through_parser() {
        for seconds in [0_i64, 1_000_000, 1_782_736_496, 4_102_444_800] {
            let stamp = format_rfc3339_utc(seconds, 0);
            let parsed = parse_rfc3339_timestamp(&stamp).expect("formatted stamp parses");
            assert_eq!(parsed.seconds, i128::from(seconds), "round-trip {stamp}");
            assert_eq!(parsed.nanos, 0);
        }
    }

    #[test]
    fn now_rfc3339_is_parseable() {
        let now = now_rfc3339();
        assert!(
            parse_rfc3339_timestamp(&now).is_some(),
            "now_rfc3339 produced an unparseable stamp: {now}"
        );
    }
}
