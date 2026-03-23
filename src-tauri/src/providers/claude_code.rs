use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

/// In-memory cache for parsed stats to avoid re-parsing all JSONL files on every request.
struct CachedStats {
    stats: AllStats,
    computed_at: Instant,
}

static STATS_CACHE: Mutex<Option<CachedStats>> = Mutex::new(None);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(300); // 5min fallback — primary invalidation is event-driven

/// Invalidate the stats cache so the next fetch re-parses JSONL files.
/// Called by the file watcher when JSONL/JSON changes are detected.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Per-million-token pricing (from LiteLLM / Anthropic pricing page)
struct ModelPricing {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

fn get_pricing(model: &str) -> ModelPricing {
    // Pricing per million tokens (https://docs.anthropic.com/en/docs/about-claude/pricing)
    // Cache read = 10% of input, Cache write = 125% of input
    if model.contains("opus") {
        ModelPricing { input: 5.0, output: 25.0, cache_read: 0.50, cache_write: 6.25 }
    } else if model.contains("sonnet") {
        ModelPricing { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }
    } else if model.contains("haiku") {
        ModelPricing { input: 1.0, output: 5.0, cache_read: 0.10, cache_write: 1.25 }
    } else {
        // Default to Sonnet pricing
        ModelPricing { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }
    }
}

fn calculate_cost(pricing: &ModelPricing, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
    (input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cache_read as f64 / 1_000_000.0) * pricing.cache_read
        + (cache_write as f64 / 1_000_000.0) * pricing.cache_write
}

// --- Persistent disk cache for historical month data ---

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskCache {
    months: HashMap<String, MonthData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonthData {
    daily: Vec<DailyUsage>,
    model_usage: HashMap<String, ModelUsage>,
    total_messages: u32,
}

fn disk_cache_path(claude_dir: &PathBuf) -> PathBuf {
    claude_dir.join("ai-token-monitor-cache.json")
}

fn load_disk_cache(claude_dir: &PathBuf) -> Option<DiskCache> {
    let path = disk_cache_path(claude_dir);
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_disk_cache(claude_dir: &PathBuf, cache: &DiskCache) {
    let path = disk_cache_path(claude_dir);
    if let Ok(content) = serde_json::to_string(cache) {
        let _ = fs::write(&path, content);
    }
}

fn current_month_str() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

fn date_to_month(date: &str) -> String {
    date.get(..7).unwrap_or(date).to_string()
}

// ---

pub struct ClaudeCodeProvider {
    claude_dir: PathBuf,
}

impl ClaudeCodeProvider {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            claude_dir: home.join(".claude"),
        }
    }

    /// Parse JSONL files, optionally filtering to only current month
    fn parse_session_files(&self, only_current_month: bool) -> Vec<SessionEntry> {
        let mut dedup: HashMap<String, SessionEntry> = HashMap::new();

        let projects_dir = self.claude_dir.join("projects");
        let pattern = projects_dir.join("**").join("*.jsonl").to_string_lossy().to_string();

        let files = glob::glob(&pattern).unwrap_or_else(|_| glob::glob("").unwrap());

        let current_month = if only_current_month {
            Some(current_month_str())
        } else {
            None
        };

        for path in files.flatten() {
            if let Some(ref month) = current_month {
                if let Ok(metadata) = fs::metadata(&path) {
                    if let Ok(modified) = metadata.modified() {
                        let modified_date: chrono::DateTime<chrono::Local> = modified.into();
                        let file_month = modified_date.format("%Y-%m").to_string();
                        if &file_month < month {
                            continue;
                        }
                    }
                }
            }

            if let Ok(file) = fs::File::open(&path) {
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(entry) = parse_session_line(&line) {
                        if let Some(ref month) = current_month {
                            if &date_to_month(&entry.date) < month {
                                continue;
                            }
                        }
                        let key = format!("{}:{}", entry.message_id, entry.request_id);
                        dedup.insert(key, entry);
                    }
                }
            }
        }

        dedup.into_values().collect()
    }
}

struct SessionEntry {
    date: String,
    model: String,
    session_id: String,
    message_id: String,
    request_id: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
}

fn parse_session_line(line: &str) -> Option<SessionEntry> {
    // Quick pre-filter to avoid parsing non-assistant lines
    if !line.contains("\"type\":\"assistant\"") {
        return None;
    }

    let value: serde_json::Value = serde_json::from_str(line).ok()?;

    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }

    let message = value.get("message")?;
    let usage = message.get("usage")?;

    // Must have at least input_tokens
    usage.get("input_tokens")?;

    let timestamp = value.get("timestamp")?.as_str()?;
    let date = timestamp.get(..10)?.to_string();

    let model = message.get("model")?.as_str()?.to_string();

    // Filter out synthetic/placeholder models
    if model.starts_with('<') || model == "synthetic" {
        return None;
    }

    let session_id = value.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let message_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let request_id = value.get("requestId").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read_input_tokens = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_creation_input_tokens = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    Some(SessionEntry {
        date,
        model,
        session_id,
        message_id,
        request_id,
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
    })
}

/// Aggregate session entries into daily and model maps
fn aggregate_entries(
    entries: &[SessionEntry],
) -> (HashMap<String, DailyUsage>, HashMap<String, ModelUsage>, u32, Option<String>) {
    let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
    let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
    let mut daily_session_ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut total_messages: u32 = 0;
    let mut first_date: Option<String> = None;

    for entry in entries {
        total_messages += 1;

        if first_date.as_ref().map_or(true, |d| entry.date < *d) {
            first_date = Some(entry.date.clone());
        }

        let pricing = get_pricing(&entry.model);
        let cost = calculate_cost(
            &pricing,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_read_input_tokens,
            entry.cache_creation_input_tokens,
        );

        let total_tokens = entry.input_tokens + entry.output_tokens;

        let daily = daily_map.entry(entry.date.clone()).or_insert_with(|| DailyUsage {
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
        *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;
        daily.cost_usd += cost;
        daily.messages += 1;
        daily.input_tokens += entry.input_tokens;
        daily.output_tokens += entry.output_tokens;
        daily.cache_read_tokens += entry.cache_read_input_tokens;
        daily.cache_write_tokens += entry.cache_creation_input_tokens;

        if !entry.session_id.is_empty() {
            daily_session_ids
                .entry(entry.date.clone())
                .or_default()
                .insert(entry.session_id.clone());
        }

        let mu = model_usage_map.entry(entry.model.clone()).or_insert_with(|| ModelUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            cache_write: 0,
            cost_usd: 0.0,
        });
        mu.input_tokens += entry.input_tokens;
        mu.output_tokens += entry.output_tokens;
        mu.cache_read += entry.cache_read_input_tokens;
        mu.cache_write += entry.cache_creation_input_tokens;
        mu.cost_usd += cost;
    }

    for (date, session_ids) in &daily_session_ids {
        if let Some(daily) = daily_map.get_mut(date) {
            daily.sessions = session_ids.len() as u32;
        }
    }

    (daily_map, model_usage_map, total_messages, first_date)
}

impl TokenProvider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        if CACHE_INVALIDATED.swap(false, Ordering::Relaxed) {
            if let Ok(mut cache) = STATS_CACHE.lock() {
                *cache = None;
            }
        }

        if let Ok(cache) = STATS_CACHE.lock() {
            if let Some(ref cached) = *cache {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        let current_month = current_month_str();

        // Load disk cache for completed months
        let mut disk_cache = load_disk_cache(&self.claude_dir).unwrap_or(DiskCache {
            months: HashMap::new(),
        });
        let has_historical = !disk_cache.months.is_empty();

        // Only parse current month files if we have historical cache
        let entries = if has_historical {
            self.parse_session_files(true)
        } else {
            self.parse_session_files(false)
        };

        // If no historical cache, split entries into current vs historical months
        if !has_historical {
            let mut current_entries: Vec<&SessionEntry> = Vec::new();
            let mut month_entries: HashMap<String, Vec<&SessionEntry>> = HashMap::new();

            for entry in &entries {
                let month = date_to_month(&entry.date);
                if month >= current_month {
                    current_entries.push(entry);
                } else {
                    month_entries.entry(month).or_default().push(entry);
                }
            }

            // Save completed months to disk cache
            let mut new_cache = DiskCache { months: HashMap::new() };
            for (month, month_data) in &month_entries {
                let owned: Vec<SessionEntry> = month_data.iter().map(|e| SessionEntry {
                    date: e.date.clone(), model: e.model.clone(), session_id: e.session_id.clone(),
                    message_id: e.message_id.clone(), request_id: e.request_id.clone(),
                    input_tokens: e.input_tokens, output_tokens: e.output_tokens,
                    cache_read_input_tokens: e.cache_read_input_tokens,
                    cache_creation_input_tokens: e.cache_creation_input_tokens,
                }).collect();
                let (daily_map, model_map, messages, _) = aggregate_entries(&owned);
                new_cache.months.insert(month.clone(), MonthData {
                    daily: daily_map.into_values().collect(),
                    model_usage: model_map,
                    total_messages: messages,
                });
            }
            if !new_cache.months.is_empty() {
                save_disk_cache(&self.claude_dir, &new_cache);
                disk_cache = new_cache;
            }

            // Aggregate only current month entries
            let current_owned: Vec<SessionEntry> = current_entries.iter().map(|e| SessionEntry {
                date: e.date.clone(), model: e.model.clone(), session_id: e.session_id.clone(),
                message_id: e.message_id.clone(), request_id: e.request_id.clone(),
                input_tokens: e.input_tokens, output_tokens: e.output_tokens,
                cache_read_input_tokens: e.cache_read_input_tokens,
                cache_creation_input_tokens: e.cache_creation_input_tokens,
            }).collect();
            let (current_daily_map, current_model_map, current_messages, current_first_date) =
                aggregate_entries(&current_owned);

            return self.merge_and_finalize(
                current_daily_map, current_model_map, current_messages, current_first_date, &disk_cache,
            );
        }

        // Has historical cache: only current month was parsed
        let (current_daily_map, current_model_map, current_messages, current_first_date) =
            aggregate_entries(&entries);

        self.merge_and_finalize(
            current_daily_map, current_model_map, current_messages, current_first_date, &disk_cache,
        )
    }

    fn is_available(&self) -> bool {
        self.claude_dir.join("projects").exists()
    }
}

impl ClaudeCodeProvider {
    fn merge_and_finalize(
        &self,
        mut daily_map: HashMap<String, DailyUsage>,
        mut model_usage_map: HashMap<String, ModelUsage>,
        mut total_messages: u32,
        mut first_date: Option<String>,
        disk_cache: &DiskCache,
    ) -> Result<AllStats, String> {
        for (_month, month_data) in &disk_cache.months {
            total_messages += month_data.total_messages;
            for d in &month_data.daily {
                if first_date.as_ref().map_or(true, |fd| d.date < *fd) {
                    first_date = Some(d.date.clone());
                }
                daily_map.insert(d.date.clone(), d.clone());
            }
            for (model, mu) in &month_data.model_usage {
                let existing = model_usage_map.entry(model.clone()).or_insert_with(|| ModelUsage {
                    input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.0,
                });
                existing.input_tokens += mu.input_tokens;
                existing.output_tokens += mu.output_tokens;
                existing.cache_read += mu.cache_read;
                existing.cache_write += mu.cache_write;
                existing.cost_usd += mu.cost_usd;
            }
        }

        let mut daily: Vec<DailyUsage> = daily_map.into_values().collect();
        daily.sort_by(|a, b| a.date.cmp(&b.date));

        let total_sessions = daily.iter().map(|d| d.sessions as u32).sum::<u32>();

        let stats = AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
        };

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(CachedStats {
                stats: stats.clone(),
                computed_at: Instant::now(),
            });
        }

        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_jsonl_line() -> &'static str {
        r#"{"sessionId":"abc-123","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"req-1","message":{"id":"msg-1","model":"claude-sonnet-4-6-20260320","usage":{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":50000,"cache_creation_input_tokens":2000}}}"#
    }

    #[test]
    fn parse_session_line_extracts_fields() {
        let entry = parse_session_line(sample_jsonl_line()).expect("should parse");
        assert_eq!(entry.date, "2026-03-23");
        assert!(entry.model.contains("sonnet"));
        assert_eq!(entry.session_id, "abc-123");
        assert_eq!(entry.message_id, "msg-1");
        assert_eq!(entry.request_id, "req-1");
        assert_eq!(entry.input_tokens, 1000);
        assert_eq!(entry.output_tokens, 500);
        assert_eq!(entry.cache_read_input_tokens, 50000);
        assert_eq!(entry.cache_creation_input_tokens, 2000);
    }

    #[test]
    fn parse_session_line_rejects_non_assistant() {
        let line = r#"{"type":"human","timestamp":"2026-03-23T10:00:00Z","message":{"content":"hello"}}"#;
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn parse_session_line_rejects_synthetic_model() {
        let line = r#"{"type":"assistant","timestamp":"2026-03-23T10:00:00Z","message":{"id":"m1","model":"<synthetic>","usage":{"input_tokens":1}},"requestId":"r1"}"#;
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn cost_calculation_sonnet() {
        let pricing = get_pricing("claude-sonnet-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        let expected = 3.0 + 15.0 + 0.30 + 3.75;
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_opus() {
        let pricing = get_pricing("claude-opus-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 0, 0, 0);
        assert!((cost - 5.0).abs() < 0.001);
    }

    #[test]
    fn cost_calculation_haiku() {
        let pricing = get_pricing("claude-haiku-4-5-20251001");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 6.0).abs() < 0.001);
    }

    #[test]
    fn unknown_model_defaults_to_sonnet_pricing() {
        let pricing = get_pricing("claude-unknown-model");
        assert!((pricing.input - 3.0).abs() < 0.001);
        assert!((pricing.output - 15.0).abs() < 0.001);
    }
}

