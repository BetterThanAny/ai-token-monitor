use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::traits::TokenProvider;
use super::types::{
    AccountState, ActivityCategory, AllStats, AnalyticsData, BalanceInfo, ClientUsage, DailyUsage,
    LimitWindowStatus, McpServerUsage, ModelUsage, ProjectUsage, ToolCount,
};

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else {
        PathBuf::from(path)
    }
}

// --- Cache infrastructure (mirrors claude_code.rs patterns) ---

struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    /// Per-file parsed entries keyed by dedup key (session_id:line_index)
    entries: HashMap<String, CodexEntry>,
    /// Reverse index used to remove stale entries when a changed file shrinks or rewrites keys.
    entry_keys_by_file: HashMap<PathBuf, HashSet<String>>,
    /// File metadata for mtime-based change detection
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(120);

/// Invalidate cache — called by file watcher on .codex/ changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

use super::pricing;

const LONG_CONTEXT_THRESHOLD: u64 = 272_000;
const LONG_CONTEXT_INPUT_MULTIPLIER: f64 = 2.0;
const LONG_CONTEXT_OUTPUT_MULTIPLIER: f64 = 1.5;
const GPT55_FAST_CREDIT_MULTIPLIER: f64 = 2.5;
const GPT54_FAST_CREDIT_MULTIPLIER: f64 = 2.0;
const SERVICE_TIER_OVERRIDES_JSON: &str = include_str!("../../codex-service-tier-overrides.json");
const SERVICE_TIER_OVERRIDES_CONFIG: &str = "ai-token-monitor-service-tier-overrides.json";

fn long_context_model_family(model: &str) -> Option<&'static str> {
    if model.contains("gpt-5.5-pro") {
        Some("gpt-5.5-pro")
    } else if model.contains("gpt-5.5") {
        Some("gpt-5.5")
    } else if model.contains("gpt-5.4-pro") {
        Some("gpt-5.4-pro")
    } else if model.contains("gpt-5.4-mini") || model.contains("gpt-5.4-nano") {
        None
    } else if model.contains("gpt-5.4") {
        Some("gpt-5.4")
    } else {
        None
    }
}

fn codex_fast_credit_multiplier(model: &str, service_tier: ServiceTier) -> f64 {
    if service_tier != ServiceTier::Fast {
        return 1.0;
    }

    let model = model.to_ascii_lowercase();
    if model.contains("gpt-5.5") && !model.contains("gpt-5.5-pro") {
        GPT55_FAST_CREDIT_MULTIPLIER
    } else if model.contains("gpt-5.4")
        && !model.contains("gpt-5.4-pro")
        && !model.contains("gpt-5.4-mini")
        && !model.contains("gpt-5.4-nano")
    {
        GPT54_FAST_CREDIT_MULTIPLIER
    } else {
        1.0
    }
}

fn calculate_cost(
    pricing: &pricing::CodexPricing,
    input: u64,
    output: u64,
    cached: u64,
    input_multiplier: f64,
    output_multiplier: f64,
) -> f64 {
    // OpenAI's input_tokens includes cached_input_tokens as a subset.
    // Subtract cached to avoid double-counting: charge uncached at full rate, cached at discounted rate.
    let uncached_input = input.saturating_sub(cached);
    (uncached_input as f64 / 1_000_000.0) * pricing.input * input_multiplier
        + (output as f64 / 1_000_000.0) * pricing.output * output_multiplier
        + (cached as f64 / 1_000_000.0) * pricing.cached_input * input_multiplier
}

// --- Entry type ---

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenUsageSource {
    Last,
    TotalFallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceTier {
    Standard,
    Fast,
}

type CodexSnapshotDedupKey = (
    u64,
    u64,
    u64,
    u64,
    TokenUsageSource,
    Option<String>,
    Option<String>,
    Vec<String>,
    Vec<String>,
);

type CodexCumulativeDedupKey = (String, u64, u64, u64, u64);

#[derive(Clone, Debug)]
struct ServiceTierOverrideWindow {
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    tier: ServiceTier,
}

#[derive(Deserialize)]
struct RawServiceTierOverride {
    starts_at: String,
    #[serde(default)]
    ends_at: Option<String>,
    tier: String,
    #[serde(default)]
    provider: Option<String>,
}

fn service_tier_from_str(value: &str) -> Option<ServiceTier> {
    if value.eq_ignore_ascii_case("fast") {
        Some(ServiceTier::Fast)
    } else if value.eq_ignore_ascii_case("standard") {
        Some(ServiceTier::Standard)
    } else {
        None
    }
}

fn bool_from_config_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn quoted_config_value(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

fn parse_codex_config_service_tier(contents: &str) -> Option<ServiceTier> {
    let mut root_service_tier = None;
    let mut fast_mode_feature = false;
    let mut in_features = false;

    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            in_features = line == "[features]";
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        if !in_features && key == "service_tier" {
            root_service_tier = service_tier_from_str(quoted_config_value(value));
        } else if in_features && key == "fast_mode" {
            fast_mode_feature = bool_from_config_value(value).unwrap_or(false);
        }
    }

    if root_service_tier == Some(ServiceTier::Fast) || fast_mode_feature {
        Some(ServiceTier::Fast)
    } else {
        root_service_tier
    }
}

fn parse_override_datetime(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn parse_service_tier_overrides(contents: &str) -> Vec<ServiceTierOverrideWindow> {
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let Ok(raw_overrides) = serde_json::from_str::<Vec<RawServiceTierOverride>>(trimmed) else {
        eprintln!("[Codex] Failed to parse codex service tier overrides");
        return Vec::new();
    };

    let mut windows: Vec<ServiceTierOverrideWindow> = raw_overrides
        .into_iter()
        .filter(|raw| {
            raw.provider
                .as_deref()
                .map(|provider| provider.eq_ignore_ascii_case("codex"))
                .unwrap_or(true)
        })
        .filter_map(|raw| {
            let starts_at = parse_override_datetime(&raw.starts_at)?;
            let ends_at = raw.ends_at.as_deref().and_then(parse_override_datetime);
            let tier = service_tier_from_str(&raw.tier)?;
            Some(ServiceTierOverrideWindow {
                starts_at,
                ends_at,
                tier,
            })
        })
        .collect();
    windows.sort_by_key(|window| window.starts_at);
    windows
}

fn service_tier_overrides_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex").join(SERVICE_TIER_OVERRIDES_CONFIG))
}

fn configured_service_tier_overrides() -> Vec<ServiceTierOverrideWindow> {
    if let Some(path) = service_tier_overrides_path() {
        if let Ok(contents) = fs::read_to_string(&path) {
            return parse_service_tier_overrides(&contents);
        }
    }

    parse_service_tier_overrides(SERVICE_TIER_OVERRIDES_JSON)
}

fn codex_config_service_tier() -> Option<(ServiceTier, SystemTime)> {
    let path = dirs::home_dir()?.join(".codex").join("config.toml");
    let contents = fs::read_to_string(&path).ok()?;
    let tier = parse_codex_config_service_tier(&contents)?;
    let modified = fs::metadata(&path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Some((tier, modified))
}

fn default_service_tier_for_path(
    path: &Path,
    config_service_tier: Option<(ServiceTier, SystemTime)>,
) -> ServiceTier {
    default_service_tier_after_timestamp(None, path, config_service_tier)
}

fn event_datetime_from_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let timestamp = value.get("timestamp")?.as_str()?;
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn system_time_from_event_timestamp(value: &Value) -> Option<SystemTime> {
    let utc_dt = event_datetime_from_timestamp(value)?;
    let millis = utc_dt.timestamp_millis();
    if millis < 0 {
        return None;
    }

    Some(SystemTime::UNIX_EPOCH + Duration::from_millis(millis as u64))
}

fn default_service_tier_after_timestamp(
    event_time: Option<SystemTime>,
    path: &Path,
    config_service_tier: Option<(ServiceTier, SystemTime)>,
) -> ServiceTier {
    match config_service_tier {
        Some((ServiceTier::Fast, config_modified)) => {
            let is_after_config_change = event_time
                .or_else(|| fs::metadata(path).and_then(|m| m.modified()).ok())
                .map(|entry_time| entry_time >= config_modified)
                .unwrap_or(false);
            if is_after_config_change {
                ServiceTier::Fast
            } else {
                ServiceTier::Standard
            }
        }
        Some((ServiceTier::Standard, _)) | None => ServiceTier::Standard,
    }
}

fn service_tier_from_overrides(
    event_time: DateTime<Utc>,
    overrides: &[ServiceTierOverrideWindow],
) -> Option<ServiceTier> {
    overrides
        .iter()
        .rev()
        .find(|window| {
            event_time >= window.starts_at
                && window
                    .ends_at
                    .map(|ends_at| event_time < ends_at)
                    .unwrap_or(true)
        })
        .map(|window| window.tier)
}

fn default_service_tier_for_event(
    value: &Value,
    path: &Path,
    config_service_tier: Option<(ServiceTier, SystemTime)>,
    overrides: &[ServiceTierOverrideWindow],
) -> ServiceTier {
    if let Some(event_time) = event_datetime_from_timestamp(value) {
        if let Some(tier) = service_tier_from_overrides(event_time, overrides) {
            return tier;
        }
    }

    default_service_tier_after_timestamp(
        system_time_from_event_timestamp(value),
        path,
        config_service_tier,
    )
}

fn extract_service_tier(value: &Value) -> Option<ServiceTier> {
    [
        "/payload/service_tier",
        "/payload/serviceTier",
        "/payload/model_config/service_tier",
        "/payload/config/service_tier",
        "/payload/info/service_tier",
        "/payload/info/serviceTier",
        "/service_tier",
        "/serviceTier",
    ]
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(|v| v.as_str()))
    .and_then(service_tier_from_str)
    .or_else(|| {
        [
            "/payload/fast_mode",
            "/payload/fastMode",
            "/payload/features/fast_mode",
            "/payload/info/fast_mode",
            "/fast_mode",
        ]
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(|v| v.as_bool()))
        .and_then(|is_fast| is_fast.then_some(ServiceTier::Fast))
    })
}

#[derive(Clone, Copy)]
struct TokenTotals {
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    total_tokens: u64,
}

#[derive(Clone, Copy)]
struct TokenUsage {
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    total_tokens: u64,
    source: TokenUsageSource,
    cumulative: Option<TokenTotals>,
}

#[derive(Clone, Debug)]
struct CodexRateLimitWindow {
    name: String,
    used_percent: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    unit: String,
    window_minutes: Option<u32>,
    resets_at: Option<String>,
}

#[derive(Clone, Debug)]
struct CodexCredits {
    balance: Option<f64>,
    used: Option<f64>,
    total: Option<f64>,
    remaining: Option<f64>,
    unit: String,
    currency: Option<String>,
    expires_at: Option<String>,
    is_unlimited: bool,
}

#[derive(Clone, Debug)]
struct CodexRateLimitSnapshot {
    observed_at: Option<String>,
    windows: Vec<CodexRateLimitWindow>,
    credits: Option<CodexCredits>,
}

struct ProjectAcc {
    cost_usd: f64,
    tokens: u64,
    sessions: HashSet<String>,
    messages: u32,
}

struct ActivityAcc {
    cost_usd: f64,
    messages: u32,
}

#[derive(Clone)]
struct CodexEntry {
    date: String,
    model: String,
    session_id: String,
    cwd: Option<String>,
    client_name: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    total_tokens: u64,
    usage_source: TokenUsageSource,
    service_tier: ServiceTier,
    tool_names: Vec<String>,
    shell_commands: Vec<String>,
    rate_limits: Option<CodexRateLimitSnapshot>,
    counts_usage: bool,
}

// --- Provider ---

pub struct CodexProvider {
    #[allow(dead_code)]
    primary_dir: PathBuf,
    all_dirs: Vec<PathBuf>,
    config_service_tier: Option<(ServiceTier, SystemTime)>,
    service_tier_overrides: Vec<ServiceTierOverrideWindow>,
}

impl CodexProvider {
    pub fn new(codex_dirs: Vec<String>) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let primary = home.join(".codex");
        let mut all_dirs: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for d in &codex_dirs {
            let expanded = expand_tilde(d);
            let canonical = expanded.canonicalize().unwrap_or_else(|_| expanded.clone());
            if seen.insert(canonical) {
                all_dirs.push(expanded);
            }
        }

        let primary_canonical = primary.canonicalize().unwrap_or_else(|_| primary.clone());
        if !seen.contains(&primary_canonical) {
            all_dirs.insert(0, primary.clone());
        }

        Self {
            primary_dir: primary,
            all_dirs,
            config_service_tier: codex_config_service_tier(),
            service_tier_overrides: configured_service_tier_overrides(),
        }
    }

    fn session_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for dir in &self.all_dirs {
            roots.push(dir.join("sessions"));
            roots.push(dir.join("archived_sessions"));
        }
        roots
    }

    /// Collect mtime/size metadata for all JSONL files.
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        for root in self.session_roots() {
            if !root.exists() {
                continue;
            }
            let pattern = root
                .join("**")
                .join("*.jsonl")
                .to_string_lossy()
                .to_string();
            let Ok(files) = glob::glob(&pattern) else {
                continue;
            };
            for path in files.flatten() {
                if let Ok(m) = fs::metadata(&path) {
                    let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    meta.insert(path, (mtime, m.len()));
                }
            }
        }
        meta
    }

    /// Parse a single JSONL file and return entries keyed by dedup key.
    fn parse_single_file(
        path: &Path,
        config_service_tier: Option<(ServiceTier, SystemTime)>,
        service_tier_overrides: &[ServiceTierOverrideWindow],
    ) -> HashMap<String, CodexEntry> {
        let mut entries = HashMap::new();
        let Ok(file) = fs::File::open(path) else {
            return entries;
        };

        // Keep the path date as a fallback only. A single session file can span midnight,
        // so per-event timestamps are more accurate for "today" stats.
        let path_date = extract_date_from_path(path);

        let mut session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("codex-session")
            .to_string();
        let mut current_model = String::new();
        let mut current_cwd: Option<String> = None;
        let mut current_client = "Codex".to_string();
        let mut current_service_tier = default_service_tier_for_path(path, config_service_tier);
        let mut has_explicit_service_tier = false;
        let mut line_index: u32 = 0;
        let mut pending_tool_names: Vec<String> = Vec::new();
        let mut pending_shell_commands: Vec<String> = Vec::new();
        // Track previous snapshot for deduplication of identical consecutive token_count events
        let mut prev_snapshot: Option<CodexSnapshotDedupKey> = None;
        let mut seen_cumulative_snapshots: HashSet<CodexCumulativeDedupKey> = HashSet::new();

        let reader = BufReader::with_capacity(64 * 1024, file);
        for line in reader.lines().map_while(Result::ok) {
            line_index += 1;

            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            match value.get("type").and_then(|v| v.as_str()) {
                Some("session_meta") => {
                    if let Some(id) = value.pointer("/payload/id").and_then(|v| v.as_str()) {
                        session_id = id.to_string();
                    }
                    if let Some(cwd) = extract_cwd(&value) {
                        current_cwd = Some(cwd);
                    }
                    if let Some(client) = extract_client_name(&value) {
                        current_client = client;
                    }
                }
                Some("turn_context") => {
                    if let Some(model) = value.pointer("/payload/model").and_then(|v| v.as_str()) {
                        current_model = model.to_string();
                    }
                    if let Some(cwd) = extract_cwd(&value) {
                        current_cwd = Some(cwd);
                    }
                    if let Some(client) = extract_client_name(&value) {
                        current_client = client;
                    }
                    if let Some(service_tier) = extract_service_tier(&value) {
                        current_service_tier = service_tier;
                        has_explicit_service_tier = true;
                    } else if !has_explicit_service_tier {
                        current_service_tier = default_service_tier_for_event(
                            &value,
                            path,
                            config_service_tier,
                            service_tier_overrides,
                        );
                    }
                }
                Some("response_item") => {
                    if let Some((tool_name, shell_commands)) = extract_function_call(&value) {
                        pending_tool_names.push(tool_name);
                        pending_shell_commands.extend(shell_commands);
                    }
                }
                Some("event_msg") => {
                    if let Some(service_tier) = extract_service_tier(&value) {
                        current_service_tier = service_tier;
                        has_explicit_service_tier = true;
                    } else if !has_explicit_service_tier {
                        current_service_tier = default_service_tier_for_event(
                            &value,
                            path,
                            config_service_tier,
                            service_tier_overrides,
                        );
                    }

                    let payload_type = value.pointer("/payload/type").and_then(|v| v.as_str());
                    if let Some("token_count") = payload_type {
                        let Some(info) = value.pointer("/payload/info") else {
                            continue;
                        };
                        if info.is_null() {
                            continue;
                        }

                        let Some(usage) = extract_token_usage(info) else {
                            continue;
                        };
                        if let Some(service_tier) = extract_service_tier(info) {
                            current_service_tier = service_tier;
                        }

                        let rate_limits = extract_codex_rate_limits(&value);

                        let has_usage = usage.input_tokens != 0
                            || usage.output_tokens != 0
                            || usage.cached_tokens != 0
                            || usage.total_tokens != 0;

                        let counts_usage = if !has_usage {
                            false
                        } else if let Some(cumulative) = usage.cumulative {
                            let key = (
                                session_id.clone(),
                                cumulative.input_tokens,
                                cumulative.output_tokens,
                                cumulative.cached_tokens,
                                cumulative.total_tokens,
                            );
                            seen_cumulative_snapshots.insert(key)
                        } else {
                            // Older logs may not expose total_token_usage. Keep the old exact
                            // snapshot guard so identical duplicate rows are skipped, while two
                            // real adjacent requests with the same token totals are preserved.
                            let timestamp = value
                                .get("timestamp")
                                .and_then(|v| v.as_str())
                                .map(ToString::to_string);
                            let rate_limits_fingerprint = value
                                .pointer("/payload/rate_limits")
                                .or_else(|| value.pointer("/payload/info/rate_limits"))
                                .map(Value::to_string);
                            let snap = (
                                usage.input_tokens,
                                usage.output_tokens,
                                usage.cached_tokens,
                                usage.total_tokens,
                                usage.source,
                                timestamp,
                                rate_limits_fingerprint,
                                pending_tool_names.clone(),
                                pending_shell_commands.clone(),
                            );
                            if prev_snapshot.as_ref() == Some(&snap) {
                                continue;
                            }
                            prev_snapshot = Some(snap);
                            true
                        };

                        if !has_usage && rate_limits.is_none() {
                            continue;
                        }

                        let date = resolve_entry_date(path_date.as_deref(), &value);

                        let model = if current_model.is_empty() {
                            "codex".to_string()
                        } else {
                            current_model.clone()
                        };

                        let key = format!("{}:{}", session_id, line_index);
                        entries.insert(
                            key,
                            CodexEntry {
                                date,
                                model,
                                session_id: session_id.clone(),
                                cwd: current_cwd.clone(),
                                client_name: current_client.clone(),
                                input_tokens: usage.input_tokens,
                                output_tokens: usage.output_tokens,
                                cached_tokens: usage.cached_tokens,
                                total_tokens: usage.total_tokens,
                                usage_source: usage.source,
                                service_tier: current_service_tier,
                                tool_names: pending_tool_names.clone(),
                                shell_commands: pending_shell_commands.clone(),
                                rate_limits,
                                counts_usage,
                            },
                        );
                        if counts_usage {
                            pending_tool_names.clear();
                            pending_shell_commands.clear();
                        }
                    }
                }
                _ => {}
            }
        }

        entries
    }

    /// Incrementally parse only changed files.
    fn parse_incremental(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        cached_entries: &HashMap<String, CodexEntry>,
        cached_entry_keys_by_file: &HashMap<PathBuf, HashSet<String>>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        config_service_tier: Option<(ServiceTier, SystemTime)>,
        service_tier_overrides: &[ServiceTierOverrideWindow],
    ) -> (
        HashMap<String, CodexEntry>,
        HashMap<PathBuf, HashSet<String>>,
    ) {
        let mut entries = cached_entries.clone();
        let mut entry_keys_by_file = cached_entry_keys_by_file.clone();

        let mut changed_files: Vec<&PathBuf> = Vec::new();
        for (path, (mtime, size)) in current_meta {
            match cached_meta.get(path) {
                Some((cached_mtime, cached_size))
                    if cached_mtime == mtime && cached_size == size => {}
                _ => {
                    changed_files.push(path);
                }
            }
        }

        // If files were deleted, do a full re-parse
        let has_deleted = cached_meta.keys().any(|p| !current_meta.contains_key(p));
        if has_deleted {
            let mut fresh = HashMap::new();
            let mut fresh_keys_by_file = HashMap::new();
            for path in current_meta.keys() {
                let file_entries =
                    Self::parse_single_file(path, config_service_tier, service_tier_overrides);
                fresh_keys_by_file.insert(path.clone(), file_entries.keys().cloned().collect());
                fresh.extend(file_entries);
            }
            return (fresh, fresh_keys_by_file);
        }

        if !changed_files.is_empty() {
            let start = Instant::now();
            let count = changed_files.len();
            for path in &changed_files {
                if let Some(old_keys) = entry_keys_by_file.remove(*path) {
                    for key in old_keys {
                        entries.remove(&key);
                    }
                }

                let file_entries =
                    Self::parse_single_file(path, config_service_tier, service_tier_overrides);
                entry_keys_by_file.insert((*path).clone(), file_entries.keys().cloned().collect());
                entries.extend(file_entries);
            }
            eprintln!(
                "[PERF][Codex] Incremental parse: {} changed files in {:?} (total {} files)",
                count,
                start.elapsed(),
                current_meta.len()
            );
        }

        (entries, entry_keys_by_file)
    }

    /// Build AllStats from parsed entries.
    fn build_stats(entries: &HashMap<String, CodexEntry>) -> AllStats {
        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
        let mut total_messages: u32 = 0;
        let mut first_date: Option<String> = None;
        let mut all_session_ids: HashSet<String> = HashSet::new();
        let mut daily_session_ids: HashMap<String, HashSet<String>> = HashMap::new();
        let long_context_sessions = collect_long_context_sessions(entries);
        let mut project_map: HashMap<String, ProjectAcc> = HashMap::new();
        let mut tool_map: HashMap<String, u32> = HashMap::new();
        let mut shell_map: HashMap<String, u32> = HashMap::new();
        let mut mcp_map: HashMap<String, u32> = HashMap::new();
        let mut activity_map: HashMap<String, ActivityAcc> = HashMap::new();

        let ordered_entries = sorted_entries(entries);

        for (_, entry) in ordered_entries {
            if !entry.counts_usage {
                continue;
            }

            total_messages += 1;

            if first_date.as_ref().is_none_or(|d| entry.date < *d) {
                first_date = Some(entry.date.clone());
            }

            let cost = calculate_entry_cost(entry, &long_context_sessions);
            let total_tokens = entry.total_tokens;

            let daily = daily_map
                .entry(entry.date.clone())
                .or_insert_with(|| DailyUsage {
                    date: entry.date.clone(),
                    tokens: HashMap::new(),
                    cost_usd: 0.0,
                    messages: 0,
                    sessions: 0,
                    tool_calls: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                });
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += entry.total_tokens;
            daily.cost_usd += cost;
            daily.messages += 1;
            daily.tool_calls += entry.tool_names.len() as u32;
            // OpenAI's input_tokens includes cached as a subset.
            // Normalize to uncached-only so the frontend cache-hit formula
            // (cache_read / (input + cache_read)) stays consistent with Claude.
            daily.input_tokens += entry.input_tokens.saturating_sub(entry.cached_tokens);
            daily.output_tokens += entry.output_tokens;
            daily.cache_read_tokens += entry.cached_tokens;

            daily_session_ids
                .entry(entry.date.clone())
                .or_default()
                .insert(entry.session_id.clone());
            all_session_ids.insert(entry.session_id.clone());

            let mu = model_usage_map
                .entry(entry.model.clone())
                .or_insert_with(|| ModelUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read: 0,
                    cache_write: 0,
                    cost_usd: 0.0,
                });
            mu.input_tokens += entry.input_tokens.saturating_sub(entry.cached_tokens);
            mu.output_tokens += entry.output_tokens;
            mu.cache_read += entry.cached_tokens;
            mu.cost_usd += cost;

            if let Some(project_name) = project_name_from_cwd(entry.cwd.as_deref()) {
                let acc = project_map
                    .entry(project_name)
                    .or_insert_with(|| ProjectAcc {
                        cost_usd: 0.0,
                        tokens: 0,
                        sessions: HashSet::new(),
                        messages: 0,
                    });
                acc.cost_usd += cost;
                acc.tokens += total_tokens;
                acc.messages += 1;
                acc.sessions.insert(entry.session_id.clone());
            }

            for tool in &entry.tool_names {
                if let Some(server) = codex_mcp_server_from_tool_name(tool) {
                    *mcp_map.entry(server).or_insert(0) += 1;
                } else {
                    *tool_map.entry(tool.clone()).or_insert(0) += 1;
                }
            }

            for command in &entry.shell_commands {
                *shell_map.entry(command.clone()).or_insert(0) += 1;
            }

            let category = classify_codex_activity(&entry.tool_names, &entry.shell_commands);
            let acc = activity_map.entry(category).or_insert_with(|| ActivityAcc {
                cost_usd: 0.0,
                messages: 0,
            });
            acc.cost_usd += cost;
            acc.messages += 1;
        }

        // Set session counts from unique session IDs per day
        for (date, session_ids) in &daily_session_ids {
            if let Some(daily) = daily_map.get_mut(date) {
                daily.sessions = session_ids.len() as u32;
            }
        }

        let mut daily: Vec<DailyUsage> = daily_map.into_values().collect();
        daily.sort_by(|a, b| a.date.cmp(&b.date));

        let total_sessions = all_session_ids.len() as u32;
        let analytics =
            build_analytics_from_maps(project_map, tool_map, shell_map, mcp_map, activity_map);

        AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
            analytics: Some(analytics),
        }
    }

    fn build_account_state(entries: &HashMap<String, CodexEntry>) -> Option<AccountState> {
        if entries.is_empty() {
            return None;
        }

        struct ClientAcc {
            requests: u32,
            tokens: u64,
            cost_usd: f64,
        }

        let mut latest_snapshot: Option<CodexRateLimitSnapshot> = None;
        let mut client_map: HashMap<String, ClientAcc> = HashMap::new();
        let long_context_sessions = collect_long_context_sessions(entries);
        let mut total_requests = 0_u32;

        for (_, entry) in sorted_entries(entries) {
            if let Some(snapshot) = &entry.rate_limits {
                if latest_snapshot
                    .as_ref()
                    .map(|current| snapshot_is_newer(snapshot, current))
                    .unwrap_or(true)
                {
                    latest_snapshot = Some(snapshot.clone());
                }
            }

            if !entry.counts_usage {
                continue;
            }

            let cost = calculate_entry_cost(entry, &long_context_sessions);
            let client_name = if entry.client_name.trim().is_empty() {
                "Codex".to_string()
            } else {
                entry.client_name.clone()
            };
            let acc = client_map.entry(client_name).or_insert(ClientAcc {
                requests: 0,
                tokens: 0,
                cost_usd: 0.0,
            });
            acc.requests += 1;
            acc.tokens += entry.total_tokens;
            acc.cost_usd += cost;
            total_requests += 1;
        }

        let mut client_distribution: Vec<ClientUsage> = client_map
            .into_iter()
            .map(|(name, acc)| ClientUsage {
                name,
                requests: acc.requests,
                tokens: acc.tokens,
                cost_usd: acc.cost_usd,
                percent: if total_requests > 0 {
                    (acc.requests as f64 / total_requests as f64) * 100.0
                } else {
                    0.0
                },
            })
            .collect();
        client_distribution.sort_by_key(|b| std::cmp::Reverse(b.requests));

        let Some(snapshot) = latest_snapshot else {
            return Some(AccountState {
                provider: "codex".to_string(),
                fetched_at: None,
                is_stale: false,
                limit_windows: Vec::new(),
                rate_limits: Vec::new(),
                balance: None,
                client_distribution,
                diagnostics: Vec::new(),
            });
        };

        let limit_windows: Vec<LimitWindowStatus> = snapshot
            .windows
            .iter()
            .map(|window| LimitWindowStatus {
                name: window.name.clone(),
                used_percent: window.used_percent,
                used: None,
                total: window.limit,
                remaining: window.remaining,
                unit: window.unit.clone(),
                window_minutes: window.window_minutes,
                starts_at: None,
                ends_at: None,
                resets_at: window.resets_at.clone(),
                status: status_from_used_percent(window.used_percent),
                source: "codex_jsonl_rate_limits".to_string(),
            })
            .collect();

        let balance = snapshot.credits.as_ref().map(|credits| BalanceInfo {
            balance: credits.balance,
            used: credits.used,
            total: credits.total,
            remaining: credits.remaining,
            unit: credits.unit.clone(),
            currency: credits.currency.clone(),
            expires_at: credits.expires_at.clone(),
            is_unlimited: credits.is_unlimited,
            status: status_from_balance(credits),
        });

        Some(AccountState {
            provider: "codex".to_string(),
            fetched_at: snapshot.observed_at.clone(),
            is_stale: snapshot_is_stale(&snapshot),
            limit_windows,
            rate_limits: Vec::new(),
            balance,
            client_distribution,
            diagnostics: Vec::new(),
        })
    }

    pub fn fetch_account_state(&self) -> Result<Option<AccountState>, String> {
        let _ = self.fetch_stats()?;
        let cache = STATS_CACHE
            .lock()
            .map_err(|_| "Failed to acquire Codex cache lock".to_string())?;
        Ok(cache
            .as_ref()
            .and_then(|cached| Self::build_account_state(&cached.entries)))
    }

    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.collect_file_meta();

        let (entries, entry_keys_by_file) = if let Ok(mut cache) = STATS_CACHE.lock() {
            if let Some(ref mut cached) = *cache {
                if cached.file_meta == current_meta {
                    // No files changed — refresh timestamp and return cached
                    cached.computed_at = Instant::now();
                    let stats = cached.stats.clone();
                    eprintln!(
                        "[PERF][Codex] No files changed, reusing cache ({:?})",
                        start.elapsed()
                    );
                    return Ok(stats);
                }

                // Incremental parse
                Self::parse_incremental(
                    &current_meta,
                    &cached.entries,
                    &cached.entry_keys_by_file,
                    &cached.file_meta,
                    self.config_service_tier,
                    &self.service_tier_overrides,
                )
            } else {
                // First run — full parse
                drop(cache);
                eprintln!(
                    "[PERF][Codex] First run, full parse of {} files...",
                    current_meta.len()
                );
                let full_start = Instant::now();
                let mut entries = HashMap::new();
                let mut entry_keys_by_file = HashMap::new();
                for path in current_meta.keys() {
                    let file_entries = Self::parse_single_file(
                        path,
                        self.config_service_tier,
                        &self.service_tier_overrides,
                    );
                    entry_keys_by_file.insert(path.clone(), file_entries.keys().cloned().collect());
                    entries.extend(file_entries);
                }
                eprintln!(
                    "[PERF][Codex] Full parse completed in {:?}",
                    full_start.elapsed()
                );
                (entries, entry_keys_by_file)
            }
        } else {
            return Err("Failed to acquire cache lock".to_string());
        };

        let stats = Self::build_stats(&entries);

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(IncrementalCache {
                stats: stats.clone(),
                computed_at: Instant::now(),
                entries,
                entry_keys_by_file,
                file_meta: current_meta,
            });
        }

        eprintln!("[PERF][Codex] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }
}

impl TokenProvider for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        // Return cached if still fresh and not invalidated
        if !was_invalidated {
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    if cached.computed_at.elapsed() < CACHE_TTL {
                        return Ok(cached.stats.clone());
                    }
                }
            }
        }

        // Thundering herd prevention
        if PARSING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    return Ok(cached.stats.clone());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    return Ok(cached.stats.clone());
                }
            }
            return Err("Codex stats computation in progress".to_string());
        }

        let result = self.do_fetch_stats();
        PARSING.store(false, Ordering::SeqCst);
        result
    }

    fn is_available(&self) -> bool {
        self.session_roots().iter().any(|root| root.exists())
    }
}

// --- Helper functions ---

/// Extract date from directory path: .../sessions/YYYY/MM/DD/rollout-*.jsonl → "YYYY-MM-DD"
fn extract_date_from_path(path: &Path) -> Option<String> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Look for sessions/YYYY/MM/DD or archived_sessions/YYYY/MM/DD pattern
    for window in components.windows(4) {
        if (window[0] == "sessions" || window[0] == "archived_sessions")
            && window[1].len() == 4
            && window[2].len() == 2
            && window[3].len() == 2
        {
            if let (Ok(_y), Ok(_m), Ok(_d)) = (
                window[1].parse::<u32>(),
                window[2].parse::<u32>(),
                window[3].parse::<u32>(),
            ) {
                return Some(format!("{}-{}-{}", window[1], window[2], window[3]));
            }
        }
    }
    None
}

/// Fallback: extract date from timestamp field, converting UTC → local timezone.
fn extract_date_from_timestamp(value: &Value) -> Option<String> {
    let timestamp = value.get("timestamp")?.as_str()?;
    if let Ok(utc_dt) = timestamp.parse::<chrono::DateTime<chrono::Utc>>() {
        Some(
            utc_dt
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string(),
        )
    } else {
        // Fallback: substring (less accurate but safe)
        timestamp.get(..10).map(ToString::to_string)
    }
}

/// Extract token usage from a token_count event's info field.
///
/// `last_token_usage` is the per-response/request usage used for token and cost
/// accounting. `total_token_usage` is cumulative: it is used to recognize repeated
/// snapshots in modern logs, and only as a usage fallback for older logs that lack
/// `last_token_usage`.
fn parse_token_totals(usage: &Value) -> TokenTotals {
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input + output);

    TokenTotals {
        input_tokens: input,
        output_tokens: output,
        cached_tokens: cached,
        total_tokens: total,
    }
}

fn extract_token_usage(info: &Value) -> Option<TokenUsage> {
    let (usage, source) = if let Some(last) = info.get("last_token_usage") {
        (last, TokenUsageSource::Last)
    } else {
        (
            info.get("total_token_usage")?,
            TokenUsageSource::TotalFallback,
        )
    };

    let totals = parse_token_totals(usage);
    let cumulative = info.get("total_token_usage").map(parse_token_totals);

    Some(TokenUsage {
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cached_tokens: totals.cached_tokens,
        total_tokens: totals.total_tokens,
        source,
        cumulative,
    })
}

fn sorted_entries(entries: &HashMap<String, CodexEntry>) -> Vec<(&String, &CodexEntry)> {
    let mut ordered_entries: Vec<(&String, &CodexEntry)> = entries.iter().collect();
    ordered_entries.sort_by(|(key_a, entry_a), (key_b, entry_b)| {
        let line_a = dedup_key_line_index(key_a);
        let line_b = dedup_key_line_index(key_b);
        entry_a
            .session_id
            .cmp(&entry_b.session_id)
            .then_with(|| line_a.cmp(&line_b))
            .then_with(|| key_a.cmp(key_b))
    });
    ordered_entries
}

fn long_context_session_key(entry: &CodexEntry) -> Option<(String, &'static str)> {
    long_context_model_family(&entry.model).map(|family| (entry.session_id.clone(), family))
}

fn collect_long_context_sessions(
    entries: &HashMap<String, CodexEntry>,
) -> HashSet<(String, &'static str)> {
    entries
        .values()
        .filter(|entry| entry.counts_usage)
        .filter(|entry| entry.usage_source == TokenUsageSource::Last)
        .filter(|entry| entry.input_tokens > LONG_CONTEXT_THRESHOLD)
        .filter_map(long_context_session_key)
        .collect()
}

fn calculate_entry_cost(
    entry: &CodexEntry,
    long_context_sessions: &HashSet<(String, &'static str)>,
) -> f64 {
    let pricing = pricing::get_codex_pricing(&entry.model);
    let is_long_context = long_context_session_key(entry)
        .map(|key| long_context_sessions.contains(&key))
        .unwrap_or(false);
    let (input_multiplier, output_multiplier) = if is_long_context {
        (
            LONG_CONTEXT_INPUT_MULTIPLIER,
            LONG_CONTEXT_OUTPUT_MULTIPLIER,
        )
    } else {
        (1.0, 1.0)
    };
    let fast_multiplier = codex_fast_credit_multiplier(&entry.model, entry.service_tier);
    calculate_cost(
        &pricing,
        entry.input_tokens,
        entry.output_tokens,
        entry.cached_tokens,
        input_multiplier * fast_multiplier,
        output_multiplier * fast_multiplier,
    )
}

fn extract_cwd(value: &Value) -> Option<String> {
    [
        "/payload/cwd",
        "/payload/current_dir",
        "/payload/workdir",
        "/cwd",
    ]
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(|v| v.as_str()))
    .filter(|cwd| !cwd.trim().is_empty())
    .map(ToString::to_string)
}

fn extract_client_name(value: &Value) -> Option<String> {
    let payload = value.get("payload").unwrap_or(value);
    let originator = payload
        .get("originator")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let source = payload.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let lower = format!("{} {}", originator, source).to_ascii_lowercase();

    if lower.contains("vscode")
        || lower.contains("cursor")
        || lower.contains("editor")
        || lower.contains("extension")
    {
        Some("Codex Editor".to_string())
    } else if lower.contains("desktop") {
        Some("Codex Desktop".to_string())
    } else if payload.get("cli_version").is_some()
        || lower.contains("cli")
        || lower.contains("codex")
    {
        Some("Codex CLI".to_string())
    } else {
        None
    }
}

fn extract_function_call(value: &Value) -> Option<(String, Vec<String>)> {
    let payload = value.get("payload")?;
    if payload.get("type").and_then(|v| v.as_str()) != Some("function_call") {
        return None;
    }
    let raw_name = payload.get("name")?.as_str()?;
    let namespace = payload
        .get("namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tool_name = normalize_codex_tool_name(raw_name, namespace);
    let shell_commands = if raw_name == "exec_command" {
        payload
            .get("arguments")
            .map(extract_exec_command_names)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Some((tool_name, shell_commands))
}

fn normalize_codex_tool_name(name: &str, namespace: &str) -> String {
    if name.starts_with("mcp__") {
        return name.to_string();
    }
    if let Some(server) = codex_mcp_server_from_namespace(namespace) {
        return format!("mcp__{}__{}", server, name);
    }

    match name {
        "exec_command" => "Bash".to_string(),
        "apply_patch" => "Edit".to_string(),
        "update_plan" => "Plan".to_string(),
        "view_image" => "Read".to_string(),
        "web_search" | "web_search_exa" => "WebSearch".to_string(),
        "web_fetch" | "web_fetch_exa" => "WebFetch".to_string(),
        name if name.starts_with("browser_") => "Browser".to_string(),
        name if name.starts_with("mcp__") => name.to_string(),
        _ => name.to_string(),
    }
}

fn codex_mcp_server_from_namespace(namespace: &str) -> Option<String> {
    let rest = namespace.strip_prefix("mcp__")?;
    let server = rest
        .trim_end_matches("__")
        .split("__")
        .next()
        .unwrap_or("")
        .trim();
    if server.is_empty() {
        None
    } else {
        Some(server.to_string())
    }
}

fn codex_mcp_server_from_tool_name(tool_name: &str) -> Option<String> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let server = rest.split("__").next().unwrap_or("").trim();
    if server.is_empty() {
        None
    } else {
        Some(server.to_string())
    }
}

fn extract_exec_command_names(arguments: &Value) -> Vec<String> {
    let parsed;
    let args = if let Some(s) = arguments.as_str() {
        let Some(value) = serde_json::from_str::<Value>(s).ok() else {
            return Vec::new();
        };
        parsed = value;
        &parsed
    } else {
        arguments
    };
    args.get("cmd")
        .and_then(|v| v.as_str())
        .map(extract_shell_commands)
        .unwrap_or_default()
}

fn extract_shell_commands(command: &str) -> Vec<String> {
    let mut commands = Vec::new();
    for semi_part in command.split(';') {
        for and_part in semi_part.split("&&") {
            for or_part in and_part.split("||") {
                for segment in or_part.split('|') {
                    let trimmed = segment.trim().trim_start_matches('&').trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let first_token = trimmed
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_matches(|c: char| c == '\'' || c == '"');
                    if first_token.is_empty() {
                        continue;
                    }
                    let basename = first_token.rsplit('/').next().unwrap_or(first_token);
                    if !basename.is_empty() && basename != "cd" {
                        commands.push(basename.to_string());
                    }
                }
            }
        }
    }
    commands
}

fn extract_codex_rate_limits(value: &Value) -> Option<CodexRateLimitSnapshot> {
    let rate_limits = value
        .pointer("/payload/rate_limits")
        .or_else(|| value.pointer("/payload/info/rate_limits"))?;
    if rate_limits.is_null() {
        return None;
    }

    let observed_at = value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let limit_id = rate_limits
        .get("limit_id")
        .and_then(|v| v.as_str())
        .unwrap_or("codex");
    if !limit_id.eq_ignore_ascii_case("codex") {
        return None;
    }

    let mut windows = Vec::new();
    for (key, label) in [("primary", "Primary"), ("secondary", "Secondary")] {
        if let Some(window) = rate_limits.get(key) {
            let name = format!("{} Usage", label);
            if let Some(parsed) = parse_codex_rate_limit_window(name, window) {
                windows.push(parsed);
            }
        }
    }

    if windows.is_empty() {
        if let Some(array) = rate_limits.as_array() {
            for (idx, window) in array.iter().enumerate() {
                if let Some(parsed) =
                    parse_codex_rate_limit_window(format!("Window {}", idx + 1), window)
                {
                    windows.push(parsed);
                }
            }
        }
    }

    let credits = rate_limits.get("credits").and_then(parse_codex_credits);
    if windows.is_empty() && credits.is_none() {
        return None;
    }

    Some(CodexRateLimitSnapshot {
        observed_at,
        windows,
        credits,
    })
}

fn parse_codex_rate_limit_window(name: String, value: &Value) -> Option<CodexRateLimitWindow> {
    if !value.is_object() {
        return None;
    }
    let used_percent = number_at(value, &["used_percent", "usage_percent", "utilization"])
        .or_else(|| number_at(value, &["remaining_percent"]).map(|remaining| 100.0 - remaining))
        .map(clamp_percent);
    let unit = normalize_limit_unit(
        value
            .get("unit")
            .and_then(|v| v.as_str())
            .unwrap_or("percent"),
    );
    let mut limit = number_at(value, &["limit", "total", "quota", "max"]);
    let mut remaining = number_at(value, &["remaining", "available"])
        .or_else(|| number_at(value, &["remaining_percent"]));
    if is_percent_unit(&unit) {
        if let Some(used) = used_percent {
            limit = limit.or(Some(100.0));
            remaining = remaining.or(Some((100.0 - used).max(0.0)));
        }
    }
    let window_minutes = number_at(value, &["window_minutes", "window"])
        .or_else(|| {
            number_at(value, &["limit_window_seconds"]).map(|seconds| (seconds / 60.0).ceil())
        })
        .and_then(|n| u32::try_from(n as u64).ok());
    let resets_at = value
        .get("resets_at")
        .or_else(|| value.get("reset_at"))
        .or_else(|| value.get("reset_time"))
        .and_then(reset_value_to_rfc3339)
        .or_else(|| {
            number_at(value, &["reset_after_seconds"]).map(|seconds| {
                let reset_at = chrono::Utc::now() + chrono::Duration::seconds(seconds as i64);
                reset_at.to_rfc3339()
            })
        });

    if used_percent.is_none() && limit.is_none() && remaining.is_none() && resets_at.is_none() {
        return None;
    }

    Some(CodexRateLimitWindow {
        name: append_window_label(name, window_minutes),
        used_percent,
        limit,
        remaining,
        unit,
        window_minutes,
        resets_at,
    })
}

fn normalize_limit_unit(unit: &str) -> String {
    if is_percent_unit(unit) {
        "percent".to_string()
    } else {
        unit.to_string()
    }
}

fn append_window_label(name: String, window_minutes: Option<u32>) -> String {
    let Some(minutes) = window_minutes else {
        return name;
    };
    if name.contains('(') {
        return name;
    }
    let label = format_window_minutes(minutes);
    if label.is_empty() {
        name
    } else {
        format!("{} ({})", name, label)
    }
}

fn format_window_minutes(minutes: u32) -> String {
    match minutes {
        0 => String::new(),
        300 => "5h".to_string(),
        10080 => "7d".to_string(),
        _ if minutes.is_multiple_of(1440) => format!("{}d", minutes / 1440),
        _ if minutes.is_multiple_of(60) => format!("{}h", minutes / 60),
        _ => format!("{}m", minutes),
    }
}

fn reset_value_to_rfc3339(value: &Value) -> Option<String> {
    if let Some(seconds) = value.as_i64().or_else(|| value.as_u64().map(|n| n as i64)) {
        return unix_seconds_to_rfc3339(seconds);
    }

    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(seconds) = raw.parse::<i64>() {
        return unix_seconds_to_rfc3339(seconds);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&chrono::Utc).to_rfc3339());
    }
    Some(raw.to_string())
}

fn unix_seconds_to_rfc3339(seconds: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0).map(|dt| dt.to_rfc3339())
}

fn clamp_percent(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn currency_number_at(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let v = value.get(*key)?;
        v.as_f64()
            .or_else(|| v.as_u64().map(|n| n as f64))
            .or_else(|| v.as_i64().map(|n| n as f64))
            .or_else(|| v.as_str().and_then(parse_currency_number))
    })
}

fn parse_currency_number(raw: &str) -> Option<f64> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    if cleaned.trim().is_empty() {
        None
    } else {
        cleaned.parse::<f64>().ok()
    }
}

fn bool_at(value: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        let Some(v) = value.get(*key) else {
            return false;
        };
        v.as_bool().unwrap_or_else(|| {
            v.as_str()
                .map(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "yes" | "1"))
                .unwrap_or(false)
        })
    })
}

fn infer_credit_unit(value: &Value) -> String {
    if let Some(unit) = value.get("unit").and_then(|v| v.as_str()) {
        return unit.to_ascii_lowercase();
    }
    let currency = value
        .get("currency")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_uppercase());
    if currency.as_deref() == Some("USD")
        || value
            .get("balance")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.trim().starts_with('$'))
    {
        "usd".to_string()
    } else {
        "credits".to_string()
    }
}

fn infer_credit_currency(value: &Value) -> Option<String> {
    value
        .get("unit")
        .and_then(|v| v.as_str())
        .filter(|unit| unit.eq_ignore_ascii_case("usd"))
        .map(|_| "USD".to_string())
        .or_else(|| {
            value
                .get("currency")
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_uppercase())
        })
        .or_else(|| {
            value
                .get("balance")
                .and_then(|v| v.as_str())
                .filter(|s| s.trim().starts_with('$'))
                .map(|_| "USD".to_string())
        })
}

fn parse_codex_credits(value: &Value) -> Option<CodexCredits> {
    if value.is_null() {
        return None;
    }
    if let Some(balance) = value.as_f64() {
        return Some(CodexCredits {
            balance: Some(balance),
            used: None,
            total: None,
            remaining: Some(balance),
            unit: "credits".to_string(),
            currency: None,
            expires_at: None,
            is_unlimited: false,
        });
    }
    if !value.is_object() {
        return None;
    }

    let is_unlimited = bool_at(value, &["unlimited", "is_unlimited"]);
    let has_credits = bool_at(value, &["has_credits", "hasCredits"]);
    let balance = currency_number_at(value, &["balance", "remaining", "available"]);
    let used = currency_number_at(value, &["used", "used_credits"]);
    let total = currency_number_at(value, &["total", "limit", "monthly_limit"]);
    let remaining =
        currency_number_at(value, &["remaining", "available"]).or_else(|| match (total, used) {
            (Some(total), Some(used)) => Some((total - used).max(0.0)),
            _ => None,
        });
    let unit = infer_credit_unit(value);
    let currency = infer_credit_currency(value);
    let expires_at = value
        .get("expires_at")
        .or_else(|| value.get("expiresAt"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    if !is_unlimited
        && !has_credits
        && balance.is_none()
        && used.is_none()
        && total.is_none()
        && remaining.is_none()
    {
        return None;
    }

    Some(CodexCredits {
        balance,
        used,
        total,
        remaining,
        unit,
        currency,
        expires_at,
        is_unlimited,
    })
}

fn number_at(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let v = value.get(*key)?;
        v.as_f64()
            .or_else(|| v.as_u64().map(|n| n as f64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
    })
}

fn resolve_entry_date(path_date: Option<&str>, value: &Value) -> String {
    extract_date_from_timestamp(value)
        .or_else(|| path_date.map(ToString::to_string))
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn dedup_key_line_index(key: &str) -> u32 {
    key.rsplit_once(':')
        .and_then(|(_, line)| line.parse::<u32>().ok())
        .unwrap_or(0)
}

fn project_name_from_cwd(cwd: Option<&str>) -> Option<String> {
    let cwd = cwd?.trim();
    if cwd.is_empty() {
        return None;
    }
    Path::new(cwd)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .or_else(|| Some(cwd.to_string()))
}

fn classify_codex_activity(tool_names: &[String], shell_commands: &[String]) -> String {
    if tool_names.is_empty() {
        return "Conversation".to_string();
    }

    let has_edit = tool_names.iter().any(|t| t == "Edit" || t == "Write");
    let has_bash = tool_names.iter().any(|t| t == "Bash");
    let has_read = tool_names.iter().any(|t| t == "Read");
    let has_search = tool_names
        .iter()
        .any(|t| t == "WebSearch" || t == "WebFetch");
    let has_plan = tool_names.iter().any(|t| t == "Plan");
    let has_browser = tool_names.iter().any(|t| t == "Browser");
    let has_agent = tool_names
        .iter()
        .any(|t| t == "Task" || t == "Agent" || t.starts_with("mcp__"));

    if has_plan {
        return "Planning".to_string();
    }
    if has_agent {
        return "Delegation".to_string();
    }

    if has_bash && !has_edit {
        if shell_commands
            .iter()
            .any(|cmd| matches!(cmd.as_str(), "pytest" | "vitest" | "jest" | "mocha" | "npx"))
        {
            return "Testing".to_string();
        }
        if shell_commands.iter().any(|cmd| cmd == "git") {
            return "Git Ops".to_string();
        }
        if shell_commands.iter().any(|cmd| {
            matches!(
                cmd.as_str(),
                "docker" | "make" | "cargo" | "npm" | "yarn" | "pnpm" | "bun" | "pip" | "brew"
            )
        }) {
            return "Build/Deploy".to_string();
        }
    }

    if has_edit {
        return "Coding".to_string();
    }
    if has_bash || has_read || has_search || has_browser {
        return "Exploration".to_string();
    }
    "Conversation".to_string()
}

fn build_analytics_from_maps(
    project_map: HashMap<String, ProjectAcc>,
    tool_map: HashMap<String, u32>,
    shell_map: HashMap<String, u32>,
    mcp_map: HashMap<String, u32>,
    activity_map: HashMap<String, ActivityAcc>,
) -> AnalyticsData {
    let mut project_usage: Vec<ProjectUsage> = project_map
        .into_iter()
        .map(|(name, acc)| ProjectUsage {
            name,
            cost_usd: acc.cost_usd,
            tokens: acc.tokens,
            sessions: acc.sessions.len() as u32,
            messages: acc.messages,
        })
        .collect();
    project_usage.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut tool_usage: Vec<ToolCount> = tool_map
        .into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    tool_usage.sort_by_key(|b| std::cmp::Reverse(b.count));

    let mut shell_commands: Vec<ToolCount> = shell_map
        .into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    shell_commands.sort_by_key(|b| std::cmp::Reverse(b.count));

    let mut mcp_usage: Vec<McpServerUsage> = mcp_map
        .into_iter()
        .map(|(server, calls)| McpServerUsage { server, calls })
        .collect();
    mcp_usage.sort_by_key(|b| std::cmp::Reverse(b.calls));

    let mut activity_breakdown: Vec<ActivityCategory> = activity_map
        .into_iter()
        .map(|(category, acc)| ActivityCategory {
            category,
            cost_usd: acc.cost_usd,
            messages: acc.messages,
        })
        .collect();
    activity_breakdown.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    AnalyticsData {
        project_usage,
        tool_usage,
        shell_commands,
        mcp_usage,
        activity_breakdown,
    }
}

fn status_from_used_percent(used_percent: Option<f64>) -> String {
    match used_percent {
        Some(p) if p >= 100.0 => "exhausted".to_string(),
        Some(p) if p >= 90.0 => "critical".to_string(),
        Some(p) if p >= 70.0 => "warning".to_string(),
        Some(_) => "ok".to_string(),
        None => "unknown".to_string(),
    }
}

fn status_from_balance(credits: &CodexCredits) -> String {
    if credits.is_unlimited {
        return "ok".to_string();
    }
    if let Some(remaining) = credits.remaining.or(credits.balance) {
        if remaining <= 0.0 {
            return "exhausted".to_string();
        }
    }
    match (credits.used, credits.total) {
        (Some(used), Some(total)) if total > 0.0 => {
            status_from_used_percent(Some((used / total) * 100.0))
        }
        _ => "unknown".to_string(),
    }
}

fn is_percent_unit(unit: &str) -> bool {
    let normalized = unit.trim().to_ascii_lowercase();
    normalized == "percent" || normalized == "%"
}

fn snapshot_is_newer(candidate: &CodexRateLimitSnapshot, current: &CodexRateLimitSnapshot) -> bool {
    match (&candidate.observed_at, &current.observed_at) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => false,
    }
}

fn snapshot_is_stale(snapshot: &CodexRateLimitSnapshot) -> bool {
    snapshot
        .observed_at
        .as_deref()
        .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|dt| {
            chrono::Utc::now()
                .signed_duration_since(dt.with_timezone(&chrono::Utc))
                .num_seconds()
                > 600
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entry() -> CodexEntry {
        CodexEntry {
            date: "2026-01-01".to_string(),
            model: "codex".to_string(),
            session_id: "test-session".to_string(),
            cwd: None,
            client_name: "Codex CLI".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            total_tokens: 0,
            usage_source: TokenUsageSource::Last,
            service_tier: ServiceTier::Standard,
            tool_names: Vec::new(),
            shell_commands: Vec::new(),
            rate_limits: None,
            counts_usage: true,
        }
    }

    fn temp_jsonl_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ai-token-monitor-codex-{name}-{}-{nanos}.jsonl",
            std::process::id()
        ))
    }

    fn codex_jsonl_with_token_events(events: &[(&str, u64, u64, u64)]) -> String {
        let mut lines = vec![
            r#"{"type":"session_meta","payload":{"id":"same-session","cwd":"/tmp/project","cli_version":"1.0.0"}}"#.to_string(),
            r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#.to_string(),
        ];
        for (timestamp, input, output, total) in events {
            lines.push(format!(
                r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"output_tokens":{output},"cached_input_tokens":0,"total_tokens":{total}}}}}}}}}"#
            ));
        }
        lines.join("\n")
    }

    type CumulativeTokenEvent<'a> = (&'a str, u64, u64, u64, u64, u64, u64, &'a str, f64);

    fn codex_jsonl_with_cumulative_token_events(events: &[CumulativeTokenEvent<'_>]) -> String {
        let mut lines = vec![
            r#"{"type":"session_meta","payload":{"id":"same-session","cwd":"/tmp/project","cli_version":"1.0.0"}}"#.to_string(),
            r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#.to_string(),
        ];
        for (
            timestamp,
            input,
            output,
            total,
            cumulative_input,
            cumulative_output,
            cumulative_total,
            limit_id,
            used_percent,
        ) in events
        {
            lines.push(
                serde_json::json!({
                    "timestamp": timestamp,
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": input,
                                "output_tokens": output,
                                "cached_input_tokens": 0,
                                "total_tokens": total
                            },
                            "total_token_usage": {
                                "input_tokens": cumulative_input,
                                "output_tokens": cumulative_output,
                                "cached_input_tokens": 0,
                                "total_tokens": cumulative_total
                            }
                        },
                        "rate_limits": {
                            "limit_id": limit_id,
                            "primary": {
                                "used_percent": used_percent,
                                "window_minutes": 300,
                                "resets_at": 1777734000
                            },
                            "secondary": {
                                "used_percent": 50.0,
                                "window_minutes": 10080,
                                "resets_at": 1778338800
                            }
                        }
                    }
                })
                .to_string(),
            );
        }
        lines.join("\n")
    }

    #[test]
    fn test_extract_date_from_path() {
        let path = PathBuf::from("/home/user/.codex/sessions/2026/03/24/rollout-abc123.jsonl");
        assert_eq!(extract_date_from_path(&path).as_deref(), Some("2026-03-24"));

        let path2 =
            PathBuf::from("/home/user/.codex/archived_sessions/2026/01/15/rollout-xyz.jsonl");
        assert_eq!(
            extract_date_from_path(&path2).as_deref(),
            Some("2026-01-15")
        );

        let path3 = PathBuf::from("/some/random/path/file.jsonl");
        assert_eq!(extract_date_from_path(&path3), None);
    }

    #[test]
    fn test_extract_date_from_timestamp() {
        let value: Value = serde_json::json!({
            "timestamp": "2026-03-23T23:50:00.000Z"
        });
        let date = extract_date_from_timestamp(&value);
        assert!(date.is_some());
        // Exact value depends on local timezone, but format should be YYYY-MM-DD
        let d = date.unwrap();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("2026-03-2"));
    }

    #[test]
    fn test_extract_token_usage_last_usage() {
        let info: Value = serde_json::json!({
            "total_token_usage": {
                "total_tokens": 300,
                "input_tokens": 200,
                "output_tokens": 100,
                "cached_input_tokens": 0
            },
            "last_token_usage": {
                "total_tokens": 25,
                "input_tokens": 20,
                "output_tokens": 5,
                "cached_input_tokens": 2
            }
        });
        let usage = extract_token_usage(&info).unwrap();
        assert_eq!(usage.input_tokens, 20);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cached_tokens, 2);
        assert_eq!(usage.total_tokens, 25);
        assert_eq!(usage.source, TokenUsageSource::Last);
    }

    #[test]
    fn test_resolve_entry_date_prefers_event_timestamp() {
        // Use midday UTC so the local date is 2026-03-27 in any timezone (UTC-12 to UTC+12).
        let value: Value = serde_json::json!({
            "timestamp": "2026-03-27T12:00:00.000Z"
        });
        let resolved = resolve_entry_date(Some("2026-03-20"), &value);
        assert_eq!(resolved, "2026-03-27");
    }

    #[test]
    fn test_resolve_entry_date_falls_back_to_path_date() {
        let value: Value = serde_json::json!({
            "type": "event_msg"
        });
        let resolved = resolve_entry_date(Some("2026-03-27"), &value);
        assert_eq!(resolved, "2026-03-27");
    }

    #[test]
    fn test_extract_token_usage_total_fallback() {
        let info: Value = serde_json::json!({
            "total_token_usage": {
                "total_tokens": 300,
                "input_tokens": 200,
                "output_tokens": 100,
                "cached_input_tokens": 10
            }
        });
        let usage = extract_token_usage(&info).unwrap();
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.cached_tokens, 10);
        assert_eq!(usage.total_tokens, 300);
        assert_eq!(usage.source, TokenUsageSource::TotalFallback);
    }

    #[test]
    fn test_extract_token_usage_zero() {
        let info: Value = serde_json::json!({
            "last_token_usage": {
                "total_tokens": 0,
                "input_tokens": 0,
                "output_tokens": 0,
                "cached_input_tokens": 0
            }
        });
        let result = extract_token_usage(&info);
        assert!(result.is_some());
        let usage = result.unwrap();
        assert_eq!(
            (
                usage.input_tokens,
                usage.output_tokens,
                usage.cached_tokens,
                usage.total_tokens
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(usage.source, TokenUsageSource::Last);
    }

    #[test]
    fn test_parse_single_file_keeps_same_token_usage_at_different_timestamps() {
        let path = temp_jsonl_path("same-token-usage");
        std::fs::write(
            &path,
            codex_jsonl_with_token_events(&[
                ("2026-05-02T10:00:00Z", 100, 50, 150),
                ("2026-05-02T10:00:01Z", 100, 50, 150),
            ]),
        )
        .unwrap();

        let entries = CodexProvider::parse_single_file(&path, None, &[]);
        let _ = std::fs::remove_file(&path);

        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_single_file_dedupes_repeated_cumulative_token_count_snapshots() {
        let path = temp_jsonl_path("dedupe-cumulative");
        std::fs::write(
            &path,
            codex_jsonl_with_cumulative_token_events(&[
                (
                    "2026-05-02T10:00:00Z",
                    100,
                    50,
                    150,
                    100,
                    50,
                    150,
                    "codex",
                    10.0,
                ),
                (
                    "2026-05-02T10:00:01Z",
                    100,
                    50,
                    150,
                    100,
                    50,
                    150,
                    "codex_bengalfox",
                    0.0,
                ),
                (
                    "2026-05-02T10:00:02Z",
                    25,
                    5,
                    30,
                    125,
                    55,
                    180,
                    "codex_bengalfox",
                    0.0,
                ),
                (
                    "2026-05-02T10:00:03Z",
                    25,
                    5,
                    30,
                    125,
                    55,
                    180,
                    "codex",
                    11.0,
                ),
            ]),
        )
        .unwrap();

        let entries = CodexProvider::parse_single_file(&path, None, &[]);
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            entries.values().filter(|entry| entry.counts_usage).count(),
            2
        );
        let stats = CodexProvider::build_stats(&entries);
        assert_eq!(stats.total_messages, 2);
        assert_eq!(stats.daily[0].tokens["gpt-5.5"], 180);
    }

    #[test]
    fn test_duplicate_token_count_can_still_update_rate_limit_snapshot() {
        let path = temp_jsonl_path("dedupe-keeps-quota");
        std::fs::write(
            &path,
            codex_jsonl_with_cumulative_token_events(&[
                (
                    "2026-05-02T10:00:00Z",
                    100,
                    50,
                    150,
                    100,
                    50,
                    150,
                    "codex",
                    10.0,
                ),
                (
                    "2026-05-02T10:00:10Z",
                    100,
                    50,
                    150,
                    100,
                    50,
                    150,
                    "codex",
                    42.0,
                ),
            ]),
        )
        .unwrap();

        let entries = CodexProvider::parse_single_file(&path, None, &[]);
        let _ = std::fs::remove_file(&path);

        let stats = CodexProvider::build_stats(&entries);
        assert_eq!(stats.total_messages, 1);
        assert_eq!(stats.daily[0].tokens["gpt-5.5"], 150);

        let state = CodexProvider::build_account_state(&entries).unwrap();
        assert_eq!(state.limit_windows[0].used_percent, Some(42.0));
        assert_eq!(state.client_distribution[0].requests, 1);
        assert_eq!(state.client_distribution[0].tokens, 150);
    }

    #[test]
    fn test_parse_incremental_removes_stale_entries_for_changed_file() {
        let path = temp_jsonl_path("changed-file");
        std::fs::write(
            &path,
            codex_jsonl_with_token_events(&[("2026-05-02T10:00:00Z", 100, 50, 150)]),
        )
        .unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        let mut current_meta = HashMap::new();
        current_meta.insert(
            path.clone(),
            (
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                metadata.len(),
            ),
        );
        let mut cached_meta = HashMap::new();
        cached_meta.insert(path.clone(), (SystemTime::UNIX_EPOCH, 0));
        let mut cached_entries = HashMap::new();
        cached_entries.insert("stale:1".to_string(), test_entry());
        cached_entries.insert("stale:2".to_string(), test_entry());
        let mut cached_keys_by_file = HashMap::new();
        cached_keys_by_file.insert(
            path.clone(),
            HashSet::from(["stale:1".to_string(), "stale:2".to_string()]),
        );

        let (entries, keys_by_file) = CodexProvider::parse_incremental(
            &current_meta,
            &cached_entries,
            &cached_keys_by_file,
            &cached_meta,
            None,
            &[],
        );
        let _ = std::fs::remove_file(&path);

        assert_eq!(entries.len(), 1);
        assert!(!entries.contains_key("stale:1"));
        assert!(!entries.contains_key("stale:2"));
        assert_eq!(keys_by_file.get(&path).map(HashSet::len), Some(1));
    }

    #[test]
    fn test_extract_service_tier_from_turn_context() {
        let value: Value = serde_json::json!({
            "type": "turn_context",
            "payload": {
                "service_tier": "fast"
            }
        });
        assert_eq!(extract_service_tier(&value), Some(ServiceTier::Fast));
    }

    #[test]
    fn test_extract_function_call_maps_exec_command() {
        let value: Value = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"cargo test --lib\",\"workdir\":\"/tmp\"}"
            }
        });

        let (tool, commands) = extract_function_call(&value).unwrap();
        assert_eq!(tool, "Bash");
        assert_eq!(commands, vec!["cargo"]);
    }

    #[test]
    fn test_extract_function_call_splits_shell_commands() {
        let value: Value = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"cd /tmp && /usr/bin/python3 script.py | npm test; git status\",\"workdir\":\"/tmp\"}"
            }
        });

        let (_, commands) = extract_function_call(&value).unwrap();
        assert_eq!(commands, vec!["python3", "npm", "git"]);
    }

    #[test]
    fn test_extract_function_call_uses_mcp_namespace() {
        let value: Value = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "gigabrain_recall",
                "namespace": "mcp__gigabrain__"
            }
        });

        let (tool, commands) = extract_function_call(&value).unwrap();
        assert_eq!(tool, "mcp__gigabrain__gigabrain_recall");
        assert!(commands.is_empty());
        assert_eq!(
            codex_mcp_server_from_tool_name(&tool).as_deref(),
            Some("gigabrain")
        );
    }

    #[test]
    fn test_extract_codex_rate_limits_snapshot() {
        let value: Value = serde_json::json!({
            "timestamp": "2026-05-02T10:00:00Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "rate_limits": {
                    "limit_id": "codex",
                    "primary": {
                        "used_percent": 71.5,
                        "window_minutes": 300,
                        "resets_at": 1777734000
                    },
                    "secondary": {
                        "remaining_percent": 40.0,
                        "limit_window_seconds": 604800,
                        "reset_after_seconds": 3600
                    },
                    "credits": {
                        "has_credits": true,
                        "balance": "$9.99"
                    }
                }
            }
        });

        let snapshot = extract_codex_rate_limits(&value).unwrap();
        assert_eq!(
            snapshot.observed_at.as_deref(),
            Some("2026-05-02T10:00:00Z")
        );
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].name, "Primary Usage (5h)");
        assert_eq!(snapshot.windows[0].used_percent, Some(71.5));
        assert_eq!(snapshot.windows[0].remaining, Some(28.5));
        assert_eq!(snapshot.windows[0].window_minutes, Some(300));
        assert_eq!(
            snapshot.windows[0].resets_at,
            unix_seconds_to_rfc3339(1777734000)
        );
        assert_eq!(snapshot.windows[1].name, "Secondary Usage (7d)");
        assert_eq!(snapshot.windows[1].used_percent, Some(60.0));
        assert_eq!(snapshot.windows[1].remaining, Some(40.0));
        assert_eq!(snapshot.windows[1].window_minutes, Some(10080));
        assert_eq!(
            snapshot.credits.as_ref().and_then(|c| c.balance),
            Some(9.99)
        );
        assert_eq!(
            snapshot.credits.as_ref().map(|c| c.unit.as_str()),
            Some("usd")
        );
    }

    #[test]
    fn test_extract_codex_rate_limits_ignores_non_primary_codex_limit() {
        let value: Value = serde_json::json!({
            "timestamp": "2026-05-02T10:00:00Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "rate_limits": {
                    "limit_id": "codex_bengalfox",
                    "limit_name": "GPT-5.3-Codex-Spark",
                    "primary": {
                        "used_percent": 0.0,
                        "window_minutes": 300,
                        "resets_at": 1777734000
                    },
                    "secondary": {
                        "used_percent": 0.0,
                        "window_minutes": 10080,
                        "resets_at": 1778338800
                    }
                }
            }
        });

        assert!(extract_codex_rate_limits(&value).is_none());
    }

    #[test]
    fn test_parse_codex_config_service_tier() {
        let config = r#"
model = "gpt-5.5"
service_tier = "fast"

[features]
fast_mode = true
"#;
        assert_eq!(
            parse_codex_config_service_tier(config),
            Some(ServiceTier::Fast)
        );

        let standard = r#"
service_tier = "standard"

[features]
memories = true
"#;
        assert_eq!(
            parse_codex_config_service_tier(standard),
            Some(ServiceTier::Standard)
        );
    }

    #[test]
    fn test_config_fast_fallback_uses_event_timestamp() {
        let path = PathBuf::from("/tmp/nonexistent-codex-session.jsonl");
        let config_modified = SystemTime::UNIX_EPOCH + Duration::from_secs(60);
        let config_tier = Some((ServiceTier::Fast, config_modified));
        let before_config: Value = serde_json::json!({
            "timestamp": "1970-01-01T00:00:30.000Z"
        });
        let after_config: Value = serde_json::json!({
            "timestamp": "1970-01-01T00:01:30.000Z"
        });

        assert_eq!(
            default_service_tier_for_event(&before_config, &path, config_tier, &[]),
            ServiceTier::Standard
        );
        assert_eq!(
            default_service_tier_for_event(&after_config, &path, config_tier, &[]),
            ServiceTier::Fast
        );
    }

    #[test]
    fn test_service_tier_override_applies_to_unmarked_event() {
        let path = PathBuf::from("/tmp/nonexistent-codex-session.jsonl");
        let overrides = parse_service_tier_overrides(
            r#"[
              {
                "provider": "codex",
                "starts_at": "2026-05-02T13:00:00+08:00",
                "ends_at": "2026-05-02T20:00:00+08:00",
                "tier": "fast"
              }
            ]"#,
        );
        let inside: Value = serde_json::json!({
            "timestamp": "2026-05-02T06:00:00.000Z"
        });
        let outside: Value = serde_json::json!({
            "timestamp": "2026-05-02T12:30:00.000Z"
        });

        assert_eq!(
            default_service_tier_for_event(&inside, &path, None, &overrides),
            ServiceTier::Fast
        );
        assert_eq!(
            default_service_tier_for_event(&outside, &path, None, &overrides),
            ServiceTier::Standard
        );
    }

    #[test]
    fn test_pricing_models() {
        let o3 = pricing::get_codex_pricing("o3-2025-04-16");
        assert!((o3.input - 2.00).abs() < 0.001);
        assert!((o3.cached_input - 0.50).abs() < 0.001);
        assert!((o3.output - 8.00).abs() < 0.001);

        let o4mini = pricing::get_codex_pricing("o4-mini-2025-04-16");
        assert!((o4mini.input - 1.10).abs() < 0.001);
        assert!((o4mini.cached_input - 0.275).abs() < 0.001);

        let gpt41 = pricing::get_codex_pricing("gpt-4.1-2025-04-14");
        assert!((gpt41.input - 2.00).abs() < 0.001);

        let gpt41mini = pricing::get_codex_pricing("gpt-4.1-mini-2025-04-14");
        assert!((gpt41mini.input - 0.40).abs() < 0.001);

        let codex_mini = pricing::get_codex_pricing("codex-mini-latest");
        assert!((codex_mini.input - 1.50).abs() < 0.001);
        assert!((codex_mini.cached_input - 0.375).abs() < 0.001);

        let gpt52codex = pricing::get_codex_pricing("gpt-5.2-codex");
        assert!((gpt52codex.input - 1.75).abs() < 0.001);
        assert!((gpt52codex.output - 14.00).abs() < 0.001);

        let gpt52 = pricing::get_codex_pricing("gpt-5.2");
        assert!((gpt52.input - 1.75).abs() < 0.001);
        assert!((gpt52.output - 14.00).abs() < 0.001);

        let gpt53spark = pricing::get_codex_pricing("gpt-5.3-codex-spark");
        assert!((gpt53spark.input - 1.75).abs() < 0.001);
        assert!((gpt53spark.output - 14.00).abs() < 0.001);

        let unknown = pricing::get_codex_pricing("some-future-model");
        assert!((unknown.input - 2.50).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost() {
        let pricing = pricing::CodexPricing {
            input: 1.0,
            output: 5.0,
            cached_input: 0.5,
        };
        // input=1M (includes 200K cached), output=500K, cached=200K
        // uncached_input = 1M - 200K = 800K
        // cost = (800K/1M)*1.0 + (500K/1M)*5.0 + (200K/1M)*0.5 = 0.8 + 2.5 + 0.1 = 3.4
        let cost = calculate_cost(&pricing, 1_000_000, 500_000, 200_000, 1.0, 1.0);
        let expected = 0.8 + 2.5 + 0.1;
        assert!((cost - expected).abs() < 0.0001);
    }

    #[test]
    fn test_build_stats_tracks_daily_messages() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-03-24".to_string(),
                model: "o4-mini".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cached_tokens: 25,
                total_tokens: 150,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-03-24".to_string(),
                model: "o4-mini".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 200,
                output_tokens: 25,
                cached_tokens: 10,
                total_tokens: 225,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        assert_eq!(stats.total_messages, 2);
        assert_eq!(stats.daily.len(), 1);
        assert_eq!(stats.daily[0].messages, 2);
        assert_eq!(stats.daily[0].sessions, 1);
    }

    #[test]
    fn test_build_stats_counts_total_sessions_uniquely_across_days() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-03-24".to_string(),
                session_id: "session-a".to_string(),
                total_tokens: 10,
                ..test_entry()
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-03-25".to_string(),
                session_id: "session-a".to_string(),
                total_tokens: 20,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        assert_eq!(stats.daily.len(), 2);
        assert_eq!(stats.daily.iter().map(|d| d.sessions).sum::<u32>(), 2);
        assert_eq!(stats.total_sessions, 1);
    }

    #[test]
    fn test_build_stats_includes_codex_analytics() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                cwd: Some("/Users/example/work/project-a".to_string()),
                input_tokens: 1_000,
                output_tokens: 500,
                total_tokens: 1_500,
                tool_names: vec!["Bash".to_string(), "Edit".to_string()],
                shell_commands: vec!["cargo".to_string()],
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let analytics = stats.analytics.unwrap();
        assert_eq!(stats.daily[0].tool_calls, 2);
        assert_eq!(analytics.project_usage[0].name, "project-a");
        assert_eq!(analytics.project_usage[0].sessions, 1);
        assert_eq!(analytics.tool_usage[0].count, 1);
        assert_eq!(analytics.shell_commands[0].name, "cargo");
        assert_eq!(analytics.activity_breakdown[0].category, "Coding");
    }

    #[test]
    fn test_build_stats_counts_mcp_tools_by_server() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 1_000,
                output_tokens: 500,
                total_tokens: 1_500,
                tool_names: vec!["mcp__gigabrain__gigabrain_recall".to_string()],
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let analytics = stats.analytics.unwrap();
        assert_eq!(analytics.mcp_usage[0].server, "gigabrain");
        assert_eq!(analytics.mcp_usage[0].calls, 1);
        assert_eq!(analytics.tool_usage.len(), 0);
        assert_eq!(analytics.activity_breakdown[0].category, "Delegation");
    }

    #[test]
    fn test_build_account_state_uses_latest_rate_limits_and_clients() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                client_name: "Codex CLI".to_string(),
                input_tokens: 1_000,
                output_tokens: 500,
                total_tokens: 1_500,
                rate_limits: Some(CodexRateLimitSnapshot {
                    observed_at: Some(chrono::Utc::now().to_rfc3339()),
                    windows: vec![CodexRateLimitWindow {
                        name: "Codex Primary".to_string(),
                        used_percent: Some(72.0),
                        limit: None,
                        remaining: None,
                        unit: "percent".to_string(),
                        window_minutes: Some(300),
                        resets_at: Some("2026-05-02T15:00:00Z".to_string()),
                    }],
                    credits: None,
                }),
                ..test_entry()
            },
        );

        let state = CodexProvider::build_account_state(&entries).unwrap();
        assert_eq!(state.provider, "codex");
        assert_eq!(state.limit_windows.len(), 1);
        assert_eq!(state.rate_limits.len(), 0);
        assert_eq!(state.limit_windows[0].status, "warning");
        assert_eq!(state.client_distribution[0].name, "Codex CLI");
        assert_eq!(state.client_distribution[0].requests, 1);
    }

    #[test]
    fn test_build_account_state_uses_full_session_long_context_cost() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                client_name: "Codex CLI".to_string(),
                input_tokens: 1_000,
                output_tokens: 1_000,
                cached_tokens: 0,
                total_tokens: 2_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-05-03".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                client_name: "Codex CLI".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let state = CodexProvider::build_account_state(&entries).unwrap();
        let client = &state.client_distribution[0];
        assert_eq!(client.name, "Codex CLI");
        assert_eq!(client.requests, 2);
        assert!((client.cost_usd - 2.785).abs() < 0.0001);
    }

    #[test]
    fn test_build_stats_applies_fast_multiplier_to_gpt55_session() {
        let mut entries = HashMap::new();
        entries.insert(
            "fast-session:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "fast-session".to_string(),
                input_tokens: 100_000,
                output_tokens: 10_000,
                cached_tokens: 10_000,
                total_tokens: 110_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Fast,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let daily = &stats.daily[0];
        let standard_cost = 0.45 + 0.005 + 0.30;
        assert!((daily.cost_usd - (standard_cost * 2.5)).abs() < 0.0001);
    }

    #[test]
    fn test_fast_multiplier_is_limited_to_supported_codex_models() {
        assert_eq!(
            codex_fast_credit_multiplier("gpt-5.5", ServiceTier::Fast),
            GPT55_FAST_CREDIT_MULTIPLIER
        );
        assert_eq!(
            codex_fast_credit_multiplier("gpt-5.4", ServiceTier::Fast),
            GPT54_FAST_CREDIT_MULTIPLIER
        );
        assert_eq!(
            codex_fast_credit_multiplier("gpt-5.4-mini", ServiceTier::Fast),
            1.0
        );
        assert_eq!(
            codex_fast_credit_multiplier("gpt-5.5-pro", ServiceTier::Fast),
            1.0
        );
        assert_eq!(
            codex_fast_credit_multiplier("gpt-5.5", ServiceTier::Standard),
            1.0
        );
    }

    #[test]
    fn test_build_stats_applies_long_context_multiplier_to_gpt55_session() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 1_000_000,
                output_tokens: 500_000,
                cached_tokens: 200_000,
                total_tokens: 1_500_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let daily = &stats.daily[0];
        // GPT-5.5 long-context standard pricing:
        // 800K uncached input * $10/MTok + 200K cached input * $1/MTok
        // + 500K output * $45/MTok = $30.70.
        assert!((daily.cost_usd - 30.70).abs() < 0.0001);
        assert_eq!(daily.input_tokens, 800_000);
        assert_eq!(daily.cache_read_tokens, 200_000);
        assert_eq!(stats.model_usage["gpt-5.5"].input_tokens, 800_000);
        assert_eq!(stats.model_usage["gpt-5.5"].cache_read, 200_000);
    }

    #[test]
    fn test_long_context_threshold_is_strictly_greater_than_272k() {
        let mut entries = HashMap::new();
        entries.insert(
            "exact-threshold:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "exact-threshold".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );
        entries.insert(
            "long-threshold:1".to_string(),
            CodexEntry {
                date: "2026-05-03".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "long-threshold".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD + 1,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD + 1,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let exact = stats.daily.iter().find(|d| d.date == "2026-05-02").unwrap();
        let long = stats.daily.iter().find(|d| d.date == "2026-05-03").unwrap();

        assert!((exact.cost_usd - 1.36).abs() < 0.0001);
        assert!((long.cost_usd - 2.72001).abs() < 0.0001);
    }

    #[test]
    fn test_total_usage_fallback_does_not_trigger_long_context_multiplier() {
        let mut entries = HashMap::new();
        entries.insert(
            "fallback-total:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "fallback-total".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                output_tokens: 10_000,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD + 11_000,
                usage_source: TokenUsageSource::TotalFallback,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let daily = &stats.daily[0];
        // total_token_usage is cumulative fallback data. It can keep ordinary
        // cost accounting alive for old logs, but it cannot prove a single
        // prompt/context crossed 272K, so GPT-5.5 stays at short-context rates:
        // 273K input * $5/MTok + 10K output * $30/MTok = $1.665.
        assert!((daily.cost_usd - 1.665).abs() < 0.0001);
    }

    #[test]
    fn test_long_context_reprices_entries_before_trigger_for_full_session() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 1_000,
                output_tokens: 1_000,
                cached_tokens: 0,
                total_tokens: 2_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-05-03".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let before = stats.daily.iter().find(|d| d.date == "2026-05-02").unwrap();
        let trigger = stats.daily.iter().find(|d| d.date == "2026-05-03").unwrap();

        assert!((before.cost_usd - 0.055).abs() < 0.0001);
        assert!((trigger.cost_usd - 2.73).abs() < 0.0001);
    }

    #[test]
    fn test_long_context_applies_after_trigger_in_same_session_and_model_family() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 1_000,
                output_tokens: 1_000,
                cached_tokens: 0,
                total_tokens: 2_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let daily = &stats.daily[0];
        let expected = 2.73 + 0.01 + 0.045;
        assert!((daily.cost_usd - expected).abs() < 0.0001);
    }

    #[test]
    fn test_non_billable_entry_does_not_trigger_long_context_session() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                counts_usage: false,
                ..test_entry()
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 1_000,
                output_tokens: 1_000,
                cached_tokens: 0,
                total_tokens: 2_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let daily = &stats.daily[0];
        assert!((daily.cost_usd - 0.035).abs() < 0.0001);
    }

    #[test]
    fn test_long_context_does_not_bleed_across_model_families() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.5".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: LONG_CONTEXT_THRESHOLD + 1_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-05-02".to_string(),
                model: "gpt-5.4".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 1_000,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: 1_000,
                usage_source: TokenUsageSource::Last,
                service_tier: ServiceTier::Standard,
                ..test_entry()
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        let daily = &stats.daily[0];
        let expected = 2.73 + 0.0025;
        assert!((daily.cost_usd - expected).abs() < 0.0001);
    }

    #[test]
    fn test_long_context_model_matching_excludes_gpt54_mini_and_nano() {
        assert_eq!(long_context_model_family("gpt-5.5"), Some("gpt-5.5"));
        assert_eq!(
            long_context_model_family("gpt-5.5-pro"),
            Some("gpt-5.5-pro")
        );
        assert_eq!(long_context_model_family("gpt-5.4"), Some("gpt-5.4"));
        assert_eq!(
            long_context_model_family("gpt-5.4-pro"),
            Some("gpt-5.4-pro")
        );
        assert_eq!(long_context_model_family("gpt-5.4-mini"), None);
        assert_eq!(long_context_model_family("gpt-5.4-nano"), None);
    }
}
