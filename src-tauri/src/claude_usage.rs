use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STATUSLINE_REFRESH_INTERVAL_SECS: u64 = 15 * 60;
const STATUSLINE_RATE_LIMITS_STALE_AFTER_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageWindow {
    pub utilization: f64,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub monthly_limit: f64,
    pub used_credits: f64,
    pub utilization: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeQuotaUsage {
    pub five_hour: Option<UsageWindow>,
    pub seven_day: Option<UsageWindow>,
    pub seven_day_sonnet: Option<UsageWindow>,
    pub seven_day_opus: Option<UsageWindow>,
    pub extra_usage: Option<ExtraUsage>,
    pub fetched_at: String,
    pub is_stale: bool,
}

fn statusline_rate_limits_path() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".claude")
            .join("ai-token-monitor-rate-limits.json"),
    )
}

/// Read Claude Code's native statusLine rate limit snapshot.
///
/// This intentionally reads only a small local file owned by ai-token-monitor.
/// A statusLine wrapper can write either the raw Claude Code stdin payload or a
/// compact object with `captured_at` and `rate_limits`. We do not read Claude
/// OAuth credentials or call Anthropic usage APIs.
pub fn get_statusline_rate_limits_usage() -> Result<Option<ClaudeQuotaUsage>, String> {
    let Some(path) = statusline_rate_limits_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Claude statusLine quota snapshot could not be read: {}", e))?;
    let fallback_fetched_at = fs::metadata(&path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_to_rfc3339);

    parse_statusline_rate_limits_snapshot(&content, fallback_fetched_at).map(Some)
}

fn parse_statusline_rate_limits_snapshot(
    raw: &str,
    fallback_fetched_at: Option<String>,
) -> Result<ClaudeQuotaUsage, String> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|e| format!("Claude statusLine quota snapshot JSON parse failed: {}", e))?;
    let rate_limits = value.get("rate_limits").unwrap_or(&value);
    if !rate_limits.is_object() {
        return Err("Claude statusLine quota snapshot does not contain rate_limits.".to_string());
    }

    let five_hour = parse_statusline_usage_window(rate_limits.get("five_hour"), "five_hour")?;
    let seven_day = parse_statusline_usage_window(rate_limits.get("seven_day"), "seven_day")?;
    let seven_day_sonnet =
        parse_statusline_usage_window(rate_limits.get("seven_day_sonnet"), "seven_day_sonnet")?;
    let seven_day_opus =
        parse_statusline_usage_window(rate_limits.get("seven_day_opus"), "seven_day_opus")?;
    if five_hour.is_none()
        && seven_day.is_none()
        && seven_day_sonnet.is_none()
        && seven_day_opus.is_none()
    {
        return Err(
            "Claude statusLine quota snapshot does not contain usable rate_limits.".to_string(),
        );
    }

    let fallback_dt = fallback_fetched_at
        .as_deref()
        .and_then(parse_datetime_string);
    let captured_at = value
        .get("captured_at")
        .or_else(|| value.get("timestamp"))
        .and_then(parse_datetime_value)
        .or(fallback_dt);
    let fetched_at = captured_at
        .as_ref()
        .map(|dt| dt.to_rfc3339())
        .or(fallback_fetched_at)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let is_stale = captured_at
        .as_ref()
        .map(|dt| {
            chrono::Utc::now().signed_duration_since(*dt).num_seconds()
                > STATUSLINE_RATE_LIMITS_STALE_AFTER_SECS
        })
        .unwrap_or(false);

    Ok(ClaudeQuotaUsage {
        five_hour,
        seven_day,
        seven_day_sonnet,
        seven_day_opus,
        extra_usage: None,
        fetched_at,
        is_stale,
    })
}

fn parse_statusline_usage_window(
    value: Option<&Value>,
    label: &str,
) -> Result<Option<UsageWindow>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let used_percentage = value
        .get("used_percentage")
        .or_else(|| value.get("used_percent"))
        .or_else(|| value.get("utilization"))
        .and_then(value_as_f64)
        .map(clamp_percentage)
        .ok_or_else(|| {
            format!(
                "Claude statusLine quota snapshot window `{}` is missing used_percentage.",
                label
            )
        })?;

    Ok(Some(UsageWindow {
        utilization: used_percentage,
        resets_at: parse_reset_value(value.get("resets_at")),
    }))
}

fn parse_reset_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    if let Some(datetime) = parse_datetime_value(value) {
        return Some(datetime.to_rfc3339());
    }
    value.as_str().map(ToString::to_string)
}

fn parse_datetime_value(value: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Some(value) = value.as_i64() {
        return chrono::DateTime::<chrono::Utc>::from_timestamp(value, 0);
    }
    if let Some(value) = value.as_f64() {
        if value.is_finite() {
            return chrono::DateTime::<chrono::Utc>::from_timestamp(value as i64, 0);
        }
    }
    value.as_str().and_then(parse_datetime_string)
}

fn parse_datetime_string(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let trimmed = value.trim();
    if let Ok(timestamp) = trimmed.parse::<i64>() {
        return chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0);
    }
    chrono::DateTime::parse_from_rfc3339(trimmed)
        .ok()
        .map(|datetime| datetime.with_timezone(&chrono::Utc))
}

fn value_as_f64(value: &Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))?;
    if number.is_finite() {
        Some(number)
    } else {
        None
    }
}

fn clamp_percentage(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn system_time_to_rfc3339(value: SystemTime) -> Option<String> {
    let datetime: chrono::DateTime<chrono::Utc> = value.into();
    Some(datetime.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_statusline_rate_limits_snapshot() {
        let usage = parse_statusline_rate_limits_snapshot(
            r#"{
                "captured_at": "2026-05-02T12:00:00Z",
                "rate_limits": {
                    "five_hour": {"used_percentage": 23.5, "resets_at": 1777713600},
                    "seven_day": {"used_percentage": "41.2", "resets_at": "2026-05-08T12:00:00Z"}
                }
            }"#,
            None,
        )
        .unwrap();

        assert_eq!(usage.fetched_at, "2026-05-02T12:00:00+00:00");
        assert_eq!(usage.five_hour.unwrap().utilization, 23.5);
        assert_eq!(
            usage.seven_day.unwrap().resets_at.as_deref(),
            Some("2026-05-08T12:00:00+00:00")
        );
    }

    #[test]
    fn parses_raw_statusline_payload_shape() {
        let usage = parse_statusline_rate_limits_snapshot(
            r#"{
                "session_id": "abc123",
                "rate_limits": {
                    "five_hour": {"used_percentage": 10, "resets_at": null}
                }
            }"#,
            Some("2026-05-02T12:30:00Z".to_string()),
        )
        .unwrap();

        assert_eq!(usage.fetched_at, "2026-05-02T12:30:00+00:00");
        assert_eq!(usage.five_hour.unwrap().resets_at, None);
        assert!(usage.seven_day.is_none());
    }

    #[test]
    fn parses_model_specific_statusline_windows() {
        let usage = parse_statusline_rate_limits_snapshot(
            r#"{
                "captured_at": "2026-05-02T12:00:00Z",
                "rate_limits": {
                    "seven_day_sonnet": {"used_percent": 33},
                    "seven_day_opus": {"utilization": 44}
                }
            }"#,
            None,
        )
        .unwrap();

        assert_eq!(usage.seven_day_sonnet.unwrap().utilization, 33.0);
        assert_eq!(usage.seven_day_opus.unwrap().utilization, 44.0);
    }

    #[test]
    fn rejects_statusline_snapshot_without_usable_rate_limits() {
        let err = parse_statusline_rate_limits_snapshot(
            r#"{"rate_limits":{"five_hour":{"resets_at":1777713600}}}"#,
            None,
        )
        .unwrap_err();

        assert!(err.contains("used_percentage"));
    }
}
