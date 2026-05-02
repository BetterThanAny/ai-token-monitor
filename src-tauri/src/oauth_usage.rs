use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Do not call Anthropic's OAuth usage endpoint more often than this.
///
/// Multiple UI paths can ask for account state updates, so this guard lives in
/// the backend fetcher rather than only in the frontend polling hook.
pub const USAGE_REFRESH_INTERVAL_SECS: u64 = 15 * 60;
const USAGE_STALE_AFTER_SECS: u64 = 30 * 60;
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
pub struct OAuthUsage {
    pub five_hour: Option<UsageWindow>,
    pub seven_day: Option<UsageWindow>,
    pub seven_day_sonnet: Option<UsageWindow>,
    pub seven_day_opus: Option<UsageWindow>,
    pub extra_usage: Option<ExtraUsage>,
    pub fetched_at: String,
    pub is_stale: bool,
}

struct CacheEntry {
    usage: OAuthUsage,
    fetched_at: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CooldownState {
    rate_limit_until_unix: i64,
}

static OAUTH_CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);
static LAST_FETCH_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Tracks when we can retry after a 429 response.
/// Stores the Instant after which we're allowed to call the API again.
static RATE_LIMIT_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Flag to prevent concurrent fetch_and_cache_usage calls.
/// This avoids duplicate keychain prompts when concurrent opt-in fetches race.
static FETCH_IN_PROGRESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// RAII guard that resets FETCH_IN_PROGRESS to false when dropped.
/// Ensures the flag is cleared even if the inner fetch panics.
struct FetchGuard;

impl Drop for FetchGuard {
    fn drop(&mut self) {
        FETCH_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Return cached OAuth usage data without fetching.
pub fn get_cached_usage() -> Option<OAuthUsage> {
    let cache = OAUTH_CACHE.lock().ok()?;
    cache.as_ref().map(|entry| {
        let mut usage = entry.usage.clone();
        if entry.fetched_at.elapsed().as_secs() > USAGE_STALE_AFTER_SECS {
            usage.is_stale = true;
        }
        usage
    })
}

pub fn get_last_error() -> Option<String> {
    LAST_FETCH_ERROR.lock().ok()?.clone()
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
/// compact object with `captured_at` and `rate_limits`. We do not inspect
/// claude-hud internals or Claude transcript files here.
pub fn get_statusline_rate_limits_usage() -> Result<Option<OAuthUsage>, String> {
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

fn set_last_error(message: impl Into<String>) {
    if let Ok(mut guard) = LAST_FETCH_ERROR.lock() {
        *guard = Some(message.into());
    }
}

fn clear_last_error() {
    if let Ok(mut guard) = LAST_FETCH_ERROR.lock() {
        *guard = None;
    }
}

/// Check if cache was fetched within the given number of seconds.
pub fn is_cache_fresh(max_age_secs: u64) -> bool {
    if let Ok(cache) = OAUTH_CACHE.lock() {
        if let Some(ref entry) = *cache {
            return entry.fetched_at.elapsed().as_secs() < max_age_secs;
        }
    }
    false
}

fn cooldown_path() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".claude")
            .join("ai-token-monitor-oauth-cooldown.json"),
    )
}

fn load_persisted_cooldown() -> Option<Instant> {
    let path = cooldown_path()?;
    let raw = fs::read_to_string(path).ok()?;
    let state: CooldownState = serde_json::from_str(&raw).ok()?;
    let now = chrono::Utc::now().timestamp();
    if state.rate_limit_until_unix <= now {
        return None;
    }
    let remaining = u64::try_from(state.rate_limit_until_unix - now).ok()?;
    Some(Instant::now() + Duration::from_secs(remaining))
}

fn persist_cooldown(cooldown_secs: u64) {
    let Some(path) = cooldown_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let state = CooldownState {
        rate_limit_until_unix: chrono::Utc::now().timestamp() + cooldown_secs as i64,
    };
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = fs::write(path, json);
    }
}

fn clear_persisted_cooldown() {
    if let Some(path) = cooldown_path() {
        let _ = fs::remove_file(path);
    }
}

fn active_rate_limit_until() -> Option<Instant> {
    let memory_until = RATE_LIMIT_UNTIL.lock().ok().and_then(|guard| *guard);
    let persisted_until = load_persisted_cooldown();

    match (memory_until, persisted_until) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => {
            if let Ok(mut guard) = RATE_LIMIT_UNTIL.lock() {
                *guard = Some(b);
            }
            Some(b)
        }
        (None, None) => None,
    }
}

fn set_rate_limit_cooldown(cooldown_secs: u64) {
    if let Ok(mut guard) = RATE_LIMIT_UNTIL.lock() {
        *guard = Some(Instant::now() + Duration::from_secs(cooldown_secs));
    }
    persist_cooldown(cooldown_secs);
}

/// Fetch usage from OAuth API and update cache. Returns the usage data.
/// Uses an atomic flag to prevent concurrent fetches (avoids duplicate keychain prompts).
pub async fn fetch_and_cache_usage() -> Option<OAuthUsage> {
    use std::sync::atomic::Ordering;

    if is_cache_fresh(USAGE_REFRESH_INTERVAL_SECS) {
        return get_cached_usage();
    }

    // If another fetch is in progress, return cached data instead of
    // triggering a second keychain access
    if FETCH_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return get_cached_usage();
    }

    let _guard = FetchGuard;
    fetch_and_cache_usage_inner().await
}

async fn fetch_and_cache_usage_inner() -> Option<OAuthUsage> {
    // Skip API call if we're still within a Retry-After window
    if let Some(until) = active_rate_limit_until() {
        if Instant::now() < until {
            return get_cached_usage().map(|mut u| {
                u.is_stale = true;
                u
            });
        }
    }

    let Some(token) = read_oauth_token() else {
        set_last_error("Claude OAuth token was not found in Keychain or credentials file.");
        return get_cached_usage().map(|mut u| {
            u.is_stale = true;
            u
        });
    };

    match fetch_usage_from_api(&token).await {
        Ok(mut usage) => {
            usage.is_stale = false;
            usage.fetched_at = chrono::Local::now().to_rfc3339();
            clear_last_error();
            // Clear rate limit timer on success
            if let Ok(mut guard) = RATE_LIMIT_UNTIL.lock() {
                *guard = None;
            }
            clear_persisted_cooldown();
            if let Ok(mut cache) = OAUTH_CACHE.lock() {
                *cache = Some(CacheEntry {
                    usage: usage.clone(),
                    fetched_at: Instant::now(),
                });
            }
            Some(usage)
        }
        Err(e) => {
            eprintln!("[OAUTH] fetch failed: {}", e);
            set_last_error(e);
            // Return stale cache on error
            get_cached_usage().map(|mut u| {
                u.is_stale = true;
                u
            })
        }
    }
}

/// Read OAuth access token from macOS Keychain.
fn read_oauth_token() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        read_oauth_token_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        read_oauth_token_file()
    }
}

#[cfg(target_os = "macos")]
fn read_oauth_token_macos() -> Option<String> {
    // Try Keychain first, then fall back to .credentials.json file
    read_oauth_token_keychain().or_else(read_oauth_token_file)
}

#[cfg(target_os = "macos")]
fn read_oauth_token_keychain() -> Option<String> {
    let account = whoami::username();

    // Try legacy name first (avoids `security dump-keychain` discovery)
    let legacy = "Claude Code-credentials";
    if let Some(token) = read_keychain_password(legacy, &account) {
        return Some(token);
    }

    // Claude Code v2.1.52+ uses "Claude Code-credentials-{hash}" service name.
    // Only run discovery if legacy name didn't work.
    let service_names = find_keychain_service_names();
    for service in &service_names {
        if service == legacy {
            continue; // Already tried
        }
        if let Some(token) = read_keychain_password(service, &account) {
            return Some(token);
        }
    }
    None
}

/// Read a password from macOS Keychain via `/usr/bin/security` CLI.
/// Claude Code stores credentials using the same `security` binary,
/// so it's always in the keychain item's ACL — no permission prompts.
#[cfg(target_os = "macos")]
fn read_keychain_password(service: &str, account: &str) -> Option<String> {
    use std::process::Command;

    let output = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    extract_token_from_keychain_data(&output.stdout)
}

/// Cached keychain service names to avoid repeated `security dump-keychain` calls
/// which trigger additional macOS Keychain permission prompts.
#[cfg(target_os = "macos")]
static SERVICE_NAMES_CACHE: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Find Keychain service names matching "Claude Code-credentials*"
#[cfg(target_os = "macos")]
fn find_keychain_service_names() -> Vec<String> {
    use std::process::Command;

    // Return cached names if available
    if let Ok(cache) = SERVICE_NAMES_CACHE.lock() {
        if let Some(ref names) = *cache {
            return names.clone();
        }
    }

    let mut names = Vec::new();

    // Use `security find-generic-password` to discover entries.
    // First try prefix-based discovery via `security dump-keychain` grep.
    if let Ok(output) = Command::new("/usr/bin/security")
        .args(["dump-keychain"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            // Look for: "svce"<blob>="Claude Code-credentials..."
            if let Some(start) = line.find("\"Claude Code-credentials") {
                let rest = &line[start + 1..]; // skip opening quote
                if let Some(end) = rest.find('"') {
                    let service = &rest[..end];
                    if !names.contains(&service.to_string()) {
                        names.push(service.to_string());
                    }
                }
            }
        }
    }

    // Always include the legacy name as fallback
    let legacy = "Claude Code-credentials".to_string();
    if !names.contains(&legacy) {
        names.push(legacy);
    }

    // Cache the result
    if let Ok(mut cache) = SERVICE_NAMES_CACHE.lock() {
        *cache = Some(names.clone());
    }

    names
}

#[cfg(target_os = "macos")]
fn extract_token_from_keychain_data(data: &[u8]) -> Option<String> {
    let json_str = String::from_utf8_lossy(data);
    // Claude Code may prepend a non-JSON byte
    let json_str = json_str.trim_start_matches(|c: char| !c.is_ascii() || c == '\x07');
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    value
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(|s| s.to_string())
}

fn read_oauth_token_file() -> Option<String> {
    // Read from ~/.claude/.credentials.json (Windows, Linux, and macOS fallback)
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))?;
    let path = config_dir.join(".credentials.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    value
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(|s| s.to_string())
}

fn parse_statusline_rate_limits_snapshot(
    raw: &str,
    fallback_fetched_at: Option<String>,
) -> Result<OAuthUsage, String> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|e| format!("Claude statusLine quota snapshot JSON parse failed: {}", e))?;
    let rate_limits = value.get("rate_limits").unwrap_or(&value);
    if !rate_limits.is_object() {
        return Err("Claude statusLine quota snapshot does not contain rate_limits.".to_string());
    }

    let five_hour = parse_statusline_usage_window(rate_limits.get("five_hour"), "five_hour")?;
    let seven_day = parse_statusline_usage_window(rate_limits.get("seven_day"), "seven_day")?;
    if five_hour.is_none() && seven_day.is_none() {
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
            chrono::Utc::now()
                .signed_duration_since(dt.clone())
                .num_seconds()
                > STATUSLINE_RATE_LIMITS_STALE_AFTER_SECS
        })
        .unwrap_or(false);

    Ok(OAuthUsage {
        five_hour,
        seven_day,
        seven_day_sonnet: None,
        seven_day_opus: None,
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

/// Raw API response structure
#[derive(Debug, Deserialize)]
struct ApiResponse {
    five_hour: Option<ApiUsageWindow>,
    seven_day: Option<ApiUsageWindow>,
    seven_day_sonnet: Option<ApiUsageWindow>,
    seven_day_opus: Option<ApiUsageWindow>,
    extra_usage: Option<ApiExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct ApiUsageWindow {
    utilization: f64,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiExtraUsage {
    is_enabled: bool,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
}

async fn fetch_usage_from_api(token: &str) -> Result<OAuthUsage, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let response = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            format!("claude-code/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300); // default 5 min if header missing
        let cooldown_secs = retry_after_secs.max(USAGE_REFRESH_INTERVAL_SECS);
        set_rate_limit_cooldown(cooldown_secs);
        return Err(format!(
            "Rate limited (429), retry after {}s; local cooldown {}s",
            retry_after_secs, cooldown_secs
        ));
    }
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|e| format!("failed to read error body: {}", e));
        return Err(format!("HTTP {}: {}", status, truncate_for_error(&body)));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("reading response failed: {}", e))?;
    let api: ApiResponse = serde_json::from_str(&body).map_err(|e| {
        format!(
            "JSON parse failed: {}; body: {}",
            e,
            truncate_for_error(&body)
        )
    })?;

    Ok(OAuthUsage {
        five_hour: api.five_hour.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        seven_day: api.seven_day.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        seven_day_sonnet: api.seven_day_sonnet.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        seven_day_opus: api.seven_day_opus.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        extra_usage: api.extra_usage.and_then(|e| {
            let monthly_limit = e.monthly_limit?;
            let used_credits = e.used_credits?;
            let utilization = e.utilization?;
            Some(ExtraUsage {
                is_enabled: e.is_enabled,
                monthly_limit: monthly_limit / 100.0,
                used_credits: used_credits / 100.0,
                utilization,
            })
        }),
        fetched_at: String::new(),
        is_stale: false,
    })
}

fn truncate_for_error(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() > 240 {
        format!("{}...", trimmed.chars().take(240).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_window_with_null_reset() {
        let api: ApiResponse = serde_json::from_str(
            r#"{
                "five_hour": {"utilization": 0.0, "resets_at": null},
                "seven_day": {"utilization": 42.5, "resets_at": "2026-05-02T15:00:00Z"}
            }"#,
        )
        .unwrap();

        assert_eq!(api.five_hour.unwrap().resets_at, None);
        assert_eq!(
            api.seven_day.unwrap().resets_at.as_deref(),
            Some("2026-05-02T15:00:00Z")
        );
    }

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
    fn rejects_statusline_snapshot_without_usable_rate_limits() {
        let err = parse_statusline_rate_limits_snapshot(
            r#"{"rate_limits":{"five_hour":{"resets_at":1777713600}}}"#,
            None,
        )
        .unwrap_err();

        assert!(err.contains("used_percentage"));
    }
}
