use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Current UTC time as an ISO-8601 string with millisecond precision, the
/// same shape JavaScript's `Date.toISOString()` produces. ISO strings in UTC
/// compare correctly as plain strings, which is how the store orders and
/// expires rows.
pub fn now_iso() -> String {
    iso_from_system_time(SystemTime::now())
}

pub fn now_iso_minus(duration: Duration) -> String {
    iso_from_system_time(SystemTime::now() - duration)
}

pub fn now_iso_plus(duration: Duration) -> String {
    iso_from_system_time(SystemTime::now() + duration)
}

pub fn iso_from_system_time(time: SystemTime) -> String {
    let since_epoch = time
        .duration_since(UNIX_EPOCH)
        .expect("console timestamps are after the Unix epoch");
    let millis = since_epoch.subsec_millis();
    let seconds = since_epoch.as_secs();
    let days = (seconds / 86_400) as i64;
    let secs_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    )
}

/// Compact UTC stamp (`YYYYMMDDHHMMSS`) used in generated draft branch names.
pub fn now_compact_stamp() -> String {
    now_iso()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect()
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(iso_from_system_time(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // 2026-06-13T01:02:03.456Z
        let time = UNIX_EPOCH + Duration::from_millis(1_781_312_523_456);
        assert_eq!(iso_from_system_time(time), "2026-06-13T01:02:03.456Z");
        // Leap-year day: 2024-02-29T12:00:00.000Z
        let leap = UNIX_EPOCH + Duration::from_secs(1_709_208_000);
        assert_eq!(iso_from_system_time(leap), "2024-02-29T12:00:00.000Z");
    }

    #[test]
    fn iso_strings_order_chronologically() {
        let earlier = now_iso_minus(Duration::from_secs(60));
        let now = now_iso();
        let later = now_iso_plus(Duration::from_secs(60));
        assert!(earlier < now);
        assert!(now < later);
    }
}
