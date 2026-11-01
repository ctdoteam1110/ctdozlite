//! Timestamp presentation in the operating system timezone; stored epochs stay unchanged.
use chrono::{Local, TimeZone};

// Current wire timestamps may be expressed in seconds or milliseconds.
fn timestamp_millis(ts: i64) -> Option<i64> {
    match ts {
        1..10_000_000_000 => ts.checked_mul(1000),
        10_000_000_000.. => Some(ts),
        _ => None,
    }
}

/// Format a positive epoch in seconds or milliseconds as local HH:MM.
/// Invalid or out-of-range timestamps have no presentation value.
pub(crate) fn message_time(ts: i64) -> String {
    message_time_in(ts, &Local)
}

fn message_time_in<T: TimeZone>(ts: i64, timezone: &T) -> String
where
    T::Offset: std::fmt::Display,
{
    timestamp_millis(ts)
        .and_then(|ms| timezone.timestamp_millis_opt(ms).single())
        .map(|date| date.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Format the current local clock, including seconds for existing status labels.
pub(crate) fn now_clock() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn message_time_when_seconds_and_milliseconds_represent_same_instant() {
        let timezone = FixedOffset::east_opt(7 * 3600).unwrap();
        assert_eq!(message_time_in(1793467800, &timezone), "00:30");
        assert_eq!(message_time_in(1793467800000, &timezone), "00:30");
    }

    #[test]
    fn message_time_when_timezone_changes_across_midnight() {
        let east = FixedOffset::east_opt(7 * 3600).unwrap();
        let west = FixedOffset::west_opt(5 * 3600).unwrap();
        let utc = FixedOffset::east_opt(0).unwrap();
        assert_eq!(message_time_in(1793467800000, &east), "00:30");
        assert_eq!(message_time_in(1793467800000, &west), "12:30");
        assert_eq!(message_time_in(1793467800000, &utc), "17:30");
    }

    #[test]
    fn message_time_when_timestamp_is_invalid_or_out_of_range() {
        let timezone = FixedOffset::east_opt(7 * 3600).unwrap();
        for ts in [i64::MIN, -1, 0, i64::MAX] {
            assert_eq!(message_time_in(ts, &timezone), "", "timestamp {ts}");
        }
    }
}
