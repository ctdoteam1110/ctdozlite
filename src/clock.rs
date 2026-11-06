//! Shared timestamp presentation in the OS timezone; wire epochs stay unchanged.
use chrono::{DateTime, Datelike, Local, TimeZone};

pub fn normalize_ts_ms(ts: i64) -> i64 {
    if ts <= 0 {
        0
    } else if ts < 10_000_000_000 {
        ts.saturating_mul(1000)
    } else {
        ts
    }
}

pub fn local_datetime(ts: i64) -> Option<DateTime<Local>> {
    let ms = normalize_ts_ms(ts);
    (ms > 0)
        .then(|| Local.timestamp_millis_opt(ms).single())
        .flatten()
}

pub fn message_time(ts: i64) -> String {
    message_time_in(ts, &Local)
}

fn message_time_in<T: TimeZone>(ts: i64, timezone: &T) -> String
where
    T::Offset: std::fmt::Display,
{
    let ms = normalize_ts_ms(ts);
    (ms > 0)
        .then(|| timezone.timestamp_millis_opt(ms).single())
        .flatten()
        .map(|date| date.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Sidebar timestamps switch at an elapsed 24 hours, not at local midnight.
pub fn sidebar_time_at(ts: i64, now: i64) -> String {
    sidebar_time_in(ts, now, &Local)
}

pub fn sidebar_time_in<T: TimeZone>(ts: i64, now: i64, timezone: &T) -> String
where
    T::Offset: std::fmt::Display,
{
    let ts = normalize_ts_ms(ts);
    let now = normalize_ts_ms(now);
    let Some(date) = timezone
        .timestamp_millis_opt(ts)
        .single()
        .filter(|_| ts > 0)
    else {
        return String::new();
    };
    if now.saturating_sub(ts) < 86_400_000 {
        return date.format("%H:%M").to_string();
    }
    let same_year = timezone
        .timestamp_millis_opt(now)
        .single()
        .is_some_and(|current| current.year() == date.year());
    date.format(if same_year { "%d/%m" } else { "%d/%m/%Y" })
        .to_string()
}

/// Zalo PC chat-list label: "Vừa xong", "N phút", "N giờ", "N ngày" (under a week), then
/// `dd/mm` (`dd/mm/yyyy` for another year). One format for every row, whatever text the
/// thread snapshot stored.
pub fn sidebar_relative_at(ts: i64, now: i64) -> String {
    sidebar_relative_in(ts, now, &Local)
}

pub fn sidebar_relative_in<T: TimeZone>(ts: i64, now: i64, timezone: &T) -> String
where
    T::Offset: std::fmt::Display,
{
    let ts = normalize_ts_ms(ts);
    let now = normalize_ts_ms(now);
    if ts <= 0 {
        return String::new();
    }
    let mins = now.saturating_sub(ts).max(0) / 60_000;
    match mins {
        0 => "Vừa xong".into(),
        1..=59 => format!("{mins} phút"),
        60..=1439 => format!("{} giờ", mins / 60),
        1440..=10079 => format!("{} ngày", mins / 1440),
        _ => {
            let Some(date) = timezone.timestamp_millis_opt(ts).single() else {
                return String::new();
            };
            let same_year = timezone
                .timestamp_millis_opt(now)
                .single()
                .is_some_and(|current| current.year() == date.year());
            date.format(if same_year { "%d/%m" } else { "%d/%m/%Y" })
                .to_string()
        }
    }
}

pub fn local_day(ts: i64) -> Option<i64> {
    local_datetime(ts).map(|dt| dt.date_naive().num_days_from_ce() as i64)
}

pub fn now_clock() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    #[test]
    fn seconds_and_millis_use_the_same_instant_and_roll_dates() {
        let tz = FixedOffset::east_opt(7 * 3600).unwrap();
        let ts = 1793425680000;
        assert_eq!(
            tz.timestamp_millis_opt(normalize_ts_ms(ts))
                .single()
                .unwrap()
                .format("%H:%M")
                .to_string(),
            "12:48"
        );
        assert_eq!(normalize_ts_ms(ts / 1000), ts);
        assert_eq!(normalize_ts_ms(0), 0);
        assert_eq!(message_time(i64::MAX), "");
        let dt = tz.timestamp_millis_opt(1793467800000).single().unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M").to_string(), "2026-11-01 00:30");
    }

    #[test]
    fn sidebar_relative_uses_zalo_buckets() {
        let tz = FixedOffset::east_opt(7 * 3600).unwrap();
        let now = 1793467800000; // 2026-11-01 00:30 +07
        let ago = |ms: i64| sidebar_relative_in(now - ms, now, &tz);
        assert_eq!(ago(20_000), "Vừa xong");
        assert_eq!(ago(5 * 60_000), "5 phút");
        assert_eq!(ago(4 * 3_600_000 + 60_000), "4 giờ");
        assert_eq!(ago(3 * 86_400_000), "3 ngày");
        assert_eq!(ago(10 * 86_400_000), "22/10");
        assert_eq!(sidebar_relative_in(now / 1000 - 300, now, &tz), "5 phút");
        assert_eq!(sidebar_relative_in(0, now, &tz), "");
    }

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
