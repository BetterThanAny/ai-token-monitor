# LiteLLM Pricing Source Plan

Goal: add an optional LiteLLM pricing source for `ai-token-monitor` without making network pricing a hard dependency. The app should keep embedded local pricing as the default, support manual or optional LiteLLM refresh, cache successful fetches, and fall back safely when LiteLLM is unavailable or missing a private model.

## Scope

- In: Codex/Claude cost-estimation pricing source, LiteLLM pricing cache, settings UI, refresh status, tests, and docs.
- Out: token parsing/deduplication changes, Codex `rate_limits` quota logic, and any claim that USD cost is the real ChatGPT/Codex bill.

## Action Items

- [ ] Add pricing-source preferences in `src-tauri/src/providers/types.rs`, for example `pricing_source = "embedded" | "litellm" | "auto"`, defaulting to `embedded`, plus either manual refresh only or a refresh interval such as `pricing_auto_refresh_hours`.
- [ ] Add a LiteLLM fetch/cache module, likely `src-tauri/src/providers/litellm_pricing.rs`, that fetches `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` with timeout, response-size limit, and an allowlisted host.
- [ ] Cache successful LiteLLM responses in the app config directory, for example `litellm-pricing-cache.json`, with metadata for `fetched_at`, source URL, schema/version marker, and parse diagnostics.
- [ ] Refactor `src-tauri/src/providers/pricing.rs`: replace the current startup-only `OnceLock<PricingConfig>` shape with a refreshable pricing state, such as an in-memory cache guarded by `RwLock`, or a small loader that can swap active pricing after refresh.
- [ ] Add LiteLLM-to-internal mapping from `input_cost_per_token`, `output_cost_per_token`, and `cache_read_input_token_cost` into the current per-million-token `CodexPricing` / `ClaudePricing` fields.
- [ ] Preserve local fallback pricing for private or missing model names such as `codex-auto-review` and `gpt-5.3-codex-spark`; do not silently turn them into `$0` just because LiteLLM has no entry.
- [ ] Avoid double-counting long-context pricing: either keep the current local `>272K` long-context multiplier logic in `src-tauri/src/providers/codex.rs`, or fully switch to LiteLLM above-threshold fields, but do not apply both.
- [ ] Add Tauri commands in `src-tauri/src/commands.rs`, such as `refresh_pricing_from_litellm` and `get_pricing_source_status`, returning current source, last fetched time, fallback reason, and missing-model diagnostics.
- [ ] Add settings UI in `src/components/SettingsOverlay.tsx`: price-source selector, refresh button, last-refresh status, and a short warning that USD is an API-equivalent estimate, not the real ChatGPT/Codex bill.
- [ ] Update pricing display in `src/components/TodaySummary.tsx` so `get_pricing_table` shows which source is active: embedded, LiteLLM cache, or fallback.
- [ ] Add unit tests with LiteLLM fixtures for valid parse, missing fields, missing model fallback, cache read/write, stale cache, and network failure fallback.
- [ ] Validate with `cargo test pricing codex`, full `cargo test`, `npm run build`, and a fixed JSONL sample compared against `npx @ccusage/codex --json --offline` for price conversion only, not as the sole token-count baseline.

## Open Questions

- Should LiteLLM affect Codex only, or Claude and Codex together?
- Should the app auto-refresh LiteLLM prices, or keep refresh manual by default?
- For missing model prices, should the UI show local fallback, unknown price, or zero cost?
