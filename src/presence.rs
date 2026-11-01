//! Validate last-online responses without changing the existing five-minute UI policy.

use anyhow::{bail, ensure, Result};
use serde_json::Value;

const ACTIVE_SECONDS: i64 = 300;
const MAX_ENVELOPE_DEPTH: usize = 4;

/// Validated last-online information, independent of the friendship response.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LastOnline {
    last_second: Option<i64>,
    hidden: bool,
}

impl LastOnline {
    /// Decode nested or JSON-string envelopes and reject failed responses.
    ///
    /// # Errors
    /// Returns an error for a failed/malformed status, invalid JSON or excessive nesting.
    pub fn parse(response: &Value, now_second: i64) -> Result<Self> {
        Self::parse_node(response, now_second, 0)
    }

    fn parse_node(value: &Value, now: i64, depth: usize) -> Result<Self> {
        ensure!(
            depth <= MAX_ENVELOPE_DEPTH,
            "Last-online response nesting exceeded"
        );
        if let Some(text) = value.as_str() {
            let decoded: Value = serde_json::from_str(text)?;
            return Self::parse_node(&decoded, now, depth + 1);
        }
        if let Some(code) = value.get("error_code") {
            let parsed = code.as_i64().or_else(|| code.as_str()?.trim().parse().ok());
            ensure!(parsed == Some(0), "Last-online request failed");
        }
        let hidden = value
            .pointer("/settings/show_online_status")
            .is_some_and(|v| v.as_bool() == Some(false) || v.as_i64() == Some(0));
        if let Some(data) = value.get("data") {
            if data.is_object() || data.is_string() {
                let mut parsed = Self::parse_node(data, now, depth + 1)?;
                // Validate nested errors even when outer privacy hides presence.
                parsed.hidden |= hidden;
                if parsed.hidden {
                    parsed.last_second = None;
                }
                return Ok(parsed);
            }
        }
        if !value.is_object() && !value.is_null() {
            bail!("Invalid last-online response");
        }
        let raw = value.get("lastOnline").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str()?.trim().parse::<i64>().ok())
        });
        // Keep the existing seconds/milliseconds interpretation, but future clocks
        // are unknown rather than being clamped into a false online indicator.
        let last_second = raw.and_then(|ts| {
            let second = if ts > 10_000_000_000 { ts / 1000 } else { ts };
            (!hidden && second > 0 && second <= now).then_some(second)
        });
        Ok(Self {
            last_second,
            hidden,
        })
    }

    /// Return the existing online flag and Vietnamese label for validated data.
    pub fn status(&self, now_second: i64) -> (bool, String) {
        let Some(last) = self.last_second.filter(|_| !self.hidden) else {
            return (false, String::new());
        };
        if now_second < last {
            return (false, String::new());
        }
        let age = now_second.saturating_sub(last);
        if age <= ACTIVE_SECONDS {
            return (true, "Vừa mới truy cập".into());
        }
        let minutes = age / 60;
        if minutes < 60 {
            return (false, format!("Truy cập {minutes} phút trước"));
        }
        let hours = age / 3600;
        if hours < 24 {
            return (false, format!("Truy cập {hours} giờ trước"));
        }
        (
            false,
            format!("Truy cập {} ngày trước", (age / 86400).min(7)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const NOW: i64 = 1_800_000_000;

    #[test]
    fn nested_or_string_envelopes_preserve_existing_five_minute_policy() {
        for response in [
            json!({"data":{"data":{"lastOnline": NOW-300}}}),
            json!({"error_code":"0","data":json!({"error_code":0,"lastOnline":(NOW-300)*1000}).to_string()}),
        ] {
            assert_eq!(
                LastOnline::parse(&response, NOW).unwrap().status(NOW),
                (true, "Vừa mới truy cập".into())
            );
        }
        let info = LastOnline::parse(&json!({"lastOnline":NOW-301}), NOW).unwrap();
        assert_eq!(info.status(NOW), (false, "Truy cập 5 phút trước".into()));
    }

    #[test]
    fn failed_outer_or_inner_envelope_never_reports_online() {
        for value in [
            json!({"error_code":600,"data":{"lastOnline":NOW}}),
            json!({"data":{"error_code":114,"lastOnline":NOW}}),
            json!({"data":{"data":{"error_code":"600","lastOnline":NOW}}}),
            json!({"error_code":"bad","lastOnline":NOW}),
            json!({"error_code":0.5,"lastOnline":NOW}),
        ] {
            assert!(LastOnline::parse(&value, NOW).is_err());
        }
    }

    #[test]
    fn privacy_in_any_envelope_wins_over_online_timestamp() {
        for value in [
            json!({"settings":{"show_online_status":false},"data":{"lastOnline":NOW}}),
            json!({"data":{"settings":{"show_online_status":0},"lastOnline":NOW}}),
        ] {
            assert_eq!(
                LastOnline::parse(&value, NOW).unwrap().status(NOW),
                (false, String::new())
            );
        }
    }

    #[test]
    fn invalid_missing_or_future_timestamps_are_unknown() {
        for raw in [
            json!(null),
            json!(0),
            json!(-1),
            json!(NOW + 1),
            json!((NOW + 1) * 1000),
            json!("bad"),
            json!(true),
            json!(1.5),
        ] {
            assert_eq!(
                LastOnline::parse(&json!({"lastOnline":raw}), NOW)
                    .unwrap()
                    .status(NOW),
                (false, String::new())
            );
        }
        assert_eq!(
            LastOnline::parse(&json!({}), NOW).unwrap().status(NOW),
            (false, String::new())
        );
    }

    #[test]
    fn malformed_or_deep_envelopes_return_errors() {
        assert!(LastOnline::parse(&json!({"data":"not json"}), NOW).is_err());
        let mut value = json!({"lastOnline":NOW});
        for _ in 0..10 {
            value = json!({"data":value});
        }
        assert!(LastOnline::parse(&value, NOW).is_err());
    }
}
