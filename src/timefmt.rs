//! Local-time formatting.
//!
//! Earlier builds formatted timestamps as `(secs / 3600) % 24`, which is UTC — Vietnamese users
//! saw message times 7 hours behind (upstream issue #4), while a few GUI paths hard-coded UTC+7.
//! Every timestamp shown to the user now goes through this module, which uses the operating
//! system's time zone (including DST) via `chrono::Local`.

use chrono::{Local, LocalResult, Offset, TimeZone};

/// Current Unix time in milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Offset of the local time zone from UTC, in seconds, at the given Unix second.
pub fn local_offset_secs_at(unix_secs: i64) -> i64 {
    match Local.timestamp_opt(unix_secs, 0) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => {
            i64::from(dt.offset().fix().local_minus_utc())
        }
        LocalResult::None => 0,
    }
}

/// `HH:MM` for a timestamp in milliseconds with an explicit offset (seconds east of UTC).
pub fn hm_with_offset(ts_ms: i64, offset_secs: i64) -> String {
    let local = ts_ms.div_euclid(1000) + offset_secs;
    let sod = local.rem_euclid(86_400);
    format!("{:02}:{:02}", sod / 3600, (sod / 60) % 60)
}

/// `HH:MM:SS` for a Unix second with an explicit offset (seconds east of UTC).
pub fn hms_with_offset(unix_secs: i64, offset_secs: i64) -> String {
    let sod = (unix_secs + offset_secs).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", sod / 3600, (sod / 60) % 60, sod % 60)
}

/// `HH:MM` in local time for a timestamp in milliseconds.
pub fn hm_from_ms(ts_ms: i64) -> String {
    hm_with_offset(ts_ms, local_offset_secs_at(ts_ms.div_euclid(1000)))
}

/// `HH:MM` in local time for "now".
pub fn hm_now() -> String {
    hm_from_ms(now_ms())
}

/// `HH:MM:SS` in local time for "now".
pub fn hms_now() -> String {
    let secs = now_ms().div_euclid(1000);
    hms_with_offset(secs, local_offset_secs_at(secs))
}

/// Days since the Unix epoch in local time — equal values mean "same calendar day".
pub fn local_day_index(ts_ms: i64) -> i64 {
    let secs = ts_ms.div_euclid(1000);
    (secs + local_offset_secs_at(secs)).div_euclid(86_400)
}

/// Local calendar date `(year, month, day)` for a timestamp in milliseconds.
pub fn local_ymd(ts_ms: i64) -> (i32, u32, u32) {
    use chrono::Datelike;
    match Local.timestamp_millis_opt(ts_ms) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => (dt.year(), dt.month(), dt.day()),
        LocalResult::None => (1970, 1, 1),
    }
}

/// Local weekday, Monday = 1 … Sunday = 7.
pub fn local_weekday(ts_ms: i64) -> u32 {
    use chrono::Datelike;
    match Local.timestamp_millis_opt(ts_ms) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.weekday().number_from_monday(),
        LocalResult::None => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_plus_seven_shows_vietnam_time() {
        // 2026-11-01T03:30:00Z = 10:30 in Vietnam (UTC+7).
        let ts_ms = 1_793_503_800_000;
        assert_eq!(hm_with_offset(ts_ms, 0), "03:30");
        assert_eq!(hm_with_offset(ts_ms, 7 * 3600), "10:30");
    }

    #[test]
    fn offset_wraps_past_midnight() {
        // 20:00 UTC + 7h = 03:00 next day.
        assert_eq!(hm_with_offset(20 * 3600 * 1000, 7 * 3600), "03:00");
        assert_eq!(hms_with_offset(20 * 3600 + 5, 7 * 3600), "03:00:05");
        // Negative offsets wrap backwards.
        assert_eq!(hm_with_offset(3600 * 1000, -5 * 3600), "20:00");
    }

    #[test]
    fn local_helpers_have_expected_shape() {
        let s = hm_from_ms(now_ms());
        assert_eq!(s.len(), 5);
        assert_eq!(&s[2..3], ":");
        assert_eq!(hms_now().len(), 8);
        let today = local_day_index(now_ms());
        assert!(today > 19_000);
        assert!((1..=7).contains(&local_weekday(now_ms())));
    }
}
