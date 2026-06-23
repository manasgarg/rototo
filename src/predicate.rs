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
