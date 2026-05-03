use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Embedded pricing JSON (compile-time fallback)
const EMBEDDED_PRICING: &str = include_str!("../../pricing.json");

static PRICING: OnceLock<PricingConfig> = OnceLock::new();

// --- JSON schema types ---

#[derive(Deserialize)]
struct PricingConfig {
    #[serde(default = "unknown_version")]
    version: String,
    #[serde(default = "unknown_last_updated")]
    last_updated: String,
    claude: ProviderConfig,
    codex: ProviderConfig,
}

fn unknown_version() -> String {
    "unknown".to_string()
}

fn unknown_last_updated() -> String {
    "unknown".to_string()
}

#[derive(Deserialize)]
struct ProviderConfig {
    default: String,
    models: Vec<PricingEntry>,
}

#[derive(Deserialize)]
struct PricingEntry {
    #[serde(rename = "match")]
    match_pattern: String,
    #[serde(default)]
    label: String,
    input: f64,
    output: f64,
    #[serde(default)]
    cache_read: f64,
    #[serde(default)]
    cache_write: f64,
    #[serde(default)]
    cache_write_1h: f64,
    #[serde(default)]
    cached_input: f64,
}

// --- Public pricing types (used by providers) ---

pub struct ClaudePricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
}

pub struct CodexPricing {
    pub input: f64,
    pub output: f64,
    pub cached_input: f64,
}

// --- Loading ---

fn config() -> &'static PricingConfig {
    PRICING.get_or_init(|| {
        let embedded_cfg: PricingConfig =
            serde_json::from_str(EMBEDDED_PRICING).expect("embedded pricing.json must be valid");

        // Try loading from user's ~/.claude/pricing.json first
        if let Some(home) = dirs::home_dir() {
            let user_path = home.join(".claude").join("pricing.json");
            if let Ok(contents) = std::fs::read_to_string(&user_path) {
                if !is_user_pricing_current(&contents, EMBEDDED_PRICING) {
                    eprintln!(
                        "[PRICING] Ignoring stale user pricing file {}; using embedded pricing data",
                        user_path.display()
                    );
                } else if let Ok(cfg) = serde_json::from_str(&contents) {
                    eprintln!("[PRICING] Loaded from {}", user_path.display());
                    return cfg;
                }
            }
        }

        // Fallback to embedded
        eprintln!("[PRICING] Using embedded pricing data");
        embedded_cfg
    })
}

fn is_user_pricing_current(user_contents: &str, embedded_contents: &str) -> bool {
    let user_version = pricing_version(user_contents);
    let embedded_version = pricing_version(embedded_contents);
    match (user_version, embedded_version) {
        (Some(user), Some(embedded)) => user >= embedded,
        _ => false,
    }
}

fn pricing_version(contents: &str) -> Option<Vec<u32>> {
    let raw: serde_json::Value = serde_json::from_str(contents).ok()?;
    let version = raw.get("version")?.as_str()?;
    let parsed: Vec<u32> = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<_, _>>()
        .ok()?;
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

fn find_pricing<'a>(provider: &'a ProviderConfig, model: &str) -> &'a PricingEntry {
    // First match wins (order in JSON matters)
    provider
        .models
        .iter()
        .find(|e| model.contains(&e.match_pattern))
        .unwrap_or_else(|| {
            // Fallback to default model
            provider
                .models
                .iter()
                .find(|e| e.match_pattern == provider.default)
                .unwrap_or(&provider.models[0])
        })
}

// --- Public API ---

pub fn get_claude_pricing(model: &str) -> ClaudePricing {
    let entry = find_pricing(&config().claude, model);
    ClaudePricing {
        input: entry.input,
        output: entry.output,
        cache_read: entry.cache_read,
        cache_write_5m: entry.cache_write,
        cache_write_1h: if entry.cache_write_1h > 0.0 {
            entry.cache_write_1h
        } else {
            entry.cache_write
        },
    }
}

pub fn get_claude_pricing_for_speed(
    model: &str,
    speed: Option<&str>,
    service_tier: Option<&str>,
) -> ClaudePricing {
    let mut pricing = get_claude_pricing(model);
    if is_claude_fast_mode(model, speed, service_tier) {
        pricing.input = 30.0;
        pricing.output = 150.0;
        pricing.cache_read = 3.0;
        pricing.cache_write_5m = 37.5;
        pricing.cache_write_1h = 60.0;
    }
    pricing
}

pub fn is_claude_fast_mode(model: &str, speed: Option<&str>, service_tier: Option<&str>) -> bool {
    let model = model.to_ascii_lowercase();
    if !model.contains("opus-4-6") {
        return false;
    }

    speed.is_some_and(|s| s.eq_ignore_ascii_case("fast"))
        || service_tier.is_some_and(|s| s.eq_ignore_ascii_case("fast"))
}

pub fn get_codex_pricing(model: &str) -> CodexPricing {
    let entry = find_pricing(&config().codex, model);
    CodexPricing {
        input: entry.input,
        output: entry.output,
        cached_input: entry.cached_input,
    }
}

// --- Frontend API (pricing table for tooltip display) ---

#[derive(Serialize, Clone)]
pub struct PricingRow {
    pub model: String,
    pub input: String,
    pub output: String,
    pub cache_read: String,
    pub cache_write: String,
}

#[derive(Serialize, Clone)]
pub struct PricingTable {
    pub version: String,
    pub last_updated: String,
    pub claude: Vec<PricingRow>,
    pub codex: Vec<PricingRow>,
}

fn format_price(val: f64) -> String {
    if val == 0.0 {
        "—".to_string()
    } else if val < 0.01 {
        format!("${:.3}", val)
    } else if val == val.floor() {
        format!("${:.0}", val)
    } else {
        format!("${:.2}", val)
    }
}

fn deduplicated_rows(provider: &ProviderConfig, use_cached_input: bool) -> Vec<PricingRow> {
    let mut rows = Vec::new();
    let mut seen_labels = std::collections::HashSet::new();
    for entry in &provider.models {
        let label = if entry.label.is_empty() {
            &entry.match_pattern
        } else {
            &entry.label
        };
        if seen_labels.insert(label.to_string()) {
            rows.push(PricingRow {
                model: label.to_string(),
                input: format_price(entry.input),
                output: format_price(entry.output),
                cache_read: format_price(if use_cached_input {
                    entry.cached_input
                } else {
                    entry.cache_read
                }),
                cache_write: if use_cached_input {
                    "—".to_string()
                } else {
                    format_price(entry.cache_write)
                },
            });
        }
    }
    rows
}

pub fn get_pricing_table() -> PricingTable {
    let cfg = config();
    PricingTable {
        version: cfg.version.clone(),
        last_updated: cfg.last_updated.clone(),
        claude: deduplicated_rows(&cfg.claude, false),
        codex: deduplicated_rows(&cfg.codex, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_json_parses() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        assert!(!cfg.claude.models.is_empty());
        assert!(!cfg.codex.models.is_empty());
    }

    #[test]
    fn stale_user_pricing_does_not_override_embedded() {
        let user = r#"{"version":"1.2.0"}"#;
        let embedded = r#"{"version":"1.3.1"}"#;
        assert!(!is_user_pricing_current(user, embedded));
    }

    #[test]
    fn newer_user_pricing_can_override_embedded() {
        let user = r#"{"version":"1.4.0"}"#;
        let embedded = r#"{"version":"1.3.1"}"#;
        assert!(is_user_pricing_current(user, embedded));
    }

    #[test]
    fn unversioned_user_pricing_does_not_override_embedded() {
        let user = r#"{"codex":{"models":[]}}"#;
        let embedded = r#"{"version":"1.3.1"}"#;
        assert!(!is_user_pricing_current(user, embedded));
    }

    #[test]
    fn claude_opus_pricing() {
        let p = get_claude_pricing("claude-opus-4-6-20260320");
        assert!((p.input - 5.0).abs() < 0.001);
        assert!((p.output - 25.0).abs() < 0.001);
        assert!((p.cache_write_5m - 6.25).abs() < 0.001);
        assert!((p.cache_write_1h - 10.0).abs() < 0.001);
    }

    #[test]
    fn claude_opus_46_fast_pricing() {
        let p =
            get_claude_pricing_for_speed("claude-opus-4-6-20260320", Some("fast"), Some("fast"));
        assert!((p.input - 30.0).abs() < 0.001);
        assert!((p.output - 150.0).abs() < 0.001);
        assert!((p.cache_read - 3.0).abs() < 0.001);
        assert!((p.cache_write_5m - 37.5).abs() < 0.001);
        assert!((p.cache_write_1h - 60.0).abs() < 0.001);
    }

    #[test]
    fn claude_fast_flag_does_not_apply_to_unsupported_models() {
        let p = get_claude_pricing_for_speed("claude-opus-4-7", Some("fast"), Some("fast"));
        assert!((p.input - 5.0).abs() < 0.001);
        assert!((p.output - 25.0).abs() < 0.001);
    }

    // Regression guard: "opus-4-7" must match its own entry, not fall through
    // to the "opus-4" substring and get billed at Opus 4.1 rates ($15/$75).
    #[test]
    fn claude_opus_47_not_billed_as_41() {
        let p = get_claude_pricing("claude-opus-4-7-20260416");
        assert!(
            (p.input - 5.0).abs() < 0.001,
            "Opus 4.7 input must be $5/MTok, got ${}",
            p.input
        );
        assert!(
            (p.output - 25.0).abs() < 0.001,
            "Opus 4.7 output must be $25/MTok, got ${}",
            p.output
        );
        assert!((p.cache_read - 0.50).abs() < 0.001);
        assert!((p.cache_write_5m - 6.25).abs() < 0.001);
        assert!((p.cache_write_1h - 10.0).abs() < 0.001);
    }

    #[test]
    fn claude_sonnet_pricing() {
        let p = get_claude_pricing("claude-sonnet-4-6-20260320");
        assert!((p.input - 3.0).abs() < 0.001);
        assert!((p.output - 15.0).abs() < 0.001);
        assert!((p.cache_write_5m - 3.75).abs() < 0.001);
        assert!((p.cache_write_1h - 6.0).abs() < 0.001);
    }

    #[test]
    fn claude_haiku_pricing() {
        let p = get_claude_pricing("claude-haiku-4-5-20251001");
        assert!((p.input - 1.0).abs() < 0.001);
        assert!((p.output - 5.0).abs() < 0.001);
        assert!((p.cache_write_5m - 1.25).abs() < 0.001);
        assert!((p.cache_write_1h - 2.0).abs() < 0.001);
    }

    #[test]
    fn claude_unknown_defaults_to_sonnet() {
        let p = get_claude_pricing("claude-unknown-model");
        assert!((p.input - 3.0).abs() < 0.001);
    }

    #[test]
    fn codex_o4_mini_pricing() {
        let p = get_codex_pricing("o4-mini-2025-04-16");
        assert!((p.input - 1.10).abs() < 0.001);
        assert!((p.cached_input - 0.275).abs() < 0.001);
        assert!((p.output - 4.40).abs() < 0.001);
    }

    #[test]
    fn codex_gpt52_pricing() {
        let codex = get_codex_pricing("gpt-5.2-codex");
        assert!((codex.input - 1.75).abs() < 0.001);
        assert!((codex.cached_input - 0.175).abs() < 0.001);
        assert!((codex.output - 14.00).abs() < 0.001);

        let base = get_codex_pricing("gpt-5.2");
        assert!((base.input - 1.75).abs() < 0.001);
        assert!((base.cached_input - 0.175).abs() < 0.001);
        assert!((base.output - 14.00).abs() < 0.001);
    }

    #[test]
    fn codex_unknown_defaults_to_gpt54() {
        let p = get_codex_pricing("some-future-model");
        assert!((p.input - 2.50).abs() < 0.001);
    }

    // Regression guard: "gpt-5.5" must match its own entry, not fall through
    // to the default ("gpt-5.4") and get billed at GPT-5.4 rates ($2.50/$15).
    #[test]
    fn codex_gpt55_not_billed_as_gpt54() {
        let p = get_codex_pricing("gpt-5.5");
        assert!(
            (p.input - 5.00).abs() < 0.001,
            "GPT-5.5 input must be $5/MTok, got ${}",
            p.input
        );
        assert!(
            (p.output - 30.00).abs() < 0.001,
            "GPT-5.5 output must be $30/MTok, got ${}",
            p.output
        );
        assert!((p.cached_input - 0.50).abs() < 0.001);
    }

    #[test]
    fn codex_gpt55_pro_not_billed_as_gpt54() {
        let p = get_codex_pricing("gpt-5.5-pro");
        assert!(
            (p.input - 30.00).abs() < 0.001,
            "GPT-5.5 Pro input must be $30/MTok, got ${}",
            p.input
        );
        assert!(
            (p.output - 180.00).abs() < 0.001,
            "GPT-5.5 Pro output must be $180/MTok, got ${}",
            p.output
        );
    }

    // Regression guard: dated snapshot IDs (e.g. gpt-5.5-2026-04-23) must
    // resolve to the gpt-5.5 entry, not the gpt-5.4 default fallback.
    #[test]
    fn codex_gpt55_dated_snapshot_resolves_correctly() {
        let p = get_codex_pricing("gpt-5.5-2026-04-23");
        assert!(
            (p.input - 5.00).abs() < 0.001,
            "GPT-5.5 dated snapshot must match gpt-5.5, got input ${}",
            p.input
        );
        assert!((p.output - 30.00).abs() < 0.001);
    }

    #[test]
    fn codex_legacy_o3_uses_current_standard_api_price() {
        let p = get_codex_pricing("o3-2025-04-16");
        assert!((p.input - 2.00).abs() < 0.001);
        assert!((p.cached_input - 0.50).abs() < 0.001);
        assert!((p.output - 8.00).abs() < 0.001);
    }

    #[test]
    fn codex_mini_latest_uses_cached_input_price() {
        let p = get_codex_pricing("codex-mini-latest");
        assert!((p.input - 1.50).abs() < 0.001);
        assert!((p.cached_input - 0.375).abs() < 0.001);
        assert!((p.output - 6.00).abs() < 0.001);
    }

    #[test]
    fn codex_spark_uses_gpt53_codex_api_equivalent_estimate() {
        let p = get_codex_pricing("gpt-5.3-codex-spark");
        assert!((p.input - 1.75).abs() < 0.001);
        assert!((p.cached_input - 0.175).abs() < 0.001);
        assert!((p.output - 14.00).abs() < 0.001);
    }

    #[test]
    fn codex_auto_review_uses_gpt53_codex_price() {
        let p = get_codex_pricing("codex-auto-review");
        assert!((p.input - 1.75).abs() < 0.001);
        assert!((p.cached_input - 0.175).abs() < 0.001);
        assert!((p.output - 14.00).abs() < 0.001);
    }
}
