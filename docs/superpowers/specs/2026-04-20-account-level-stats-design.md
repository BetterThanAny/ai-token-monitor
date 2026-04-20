# Account-level stats aggregation across devices

**Status:** Design approved, implementation pending
**Date:** 2026-04-20
**Context:** ai-token-monitor today reports token usage only from the local `~/.claude/projects/` JSONL files on the machine it runs on. Users who use Claude Code on multiple devices (desktop + laptop) must eyeball each device separately or export files manually. This spec adds a signed-in, cross-device account view that aggregates tokens, cache breakdown, and cost per day per model across all of a user's devices.

## Goals

- A logged-in user sees a unified daily × model-breakdown × cost view across every device that has uploaded snapshots to their account.
- Cost is computed **server-side** from a pricing table, so retroactive fixes (e.g., the Opus 4.7 → Opus 4.1 mis-pricing that motivated this spec) apply to all historical data without re-upload.
- The local (single-device) view continues to work unchanged and remains the default.
- Privacy: uploaded data contains only (date, model ID, token counts). No project names, file paths, or shell commands.

## Non-goals

- Cross-device aggregation of project breakdowns, tool-use stats, bash commands, or per-session data. These stay local.
- Aggregating two separate Anthropic subscriptions for the same GitHub user into one view. (Current auth model is GitHub OAuth, 1 GitHub = 1 account.)
- Real-time sync. Existing 15-minute throttle is retained.
- Admin dashboards, CSV export, or REST APIs.

## Architecture overview

Build on top of ATM's existing Supabase + GitHub OAuth infrastructure. The existing `daily_snapshots` table and leaderboard RPCs are **not modified** — leaderboard keeps working as-is. New surface area lives in two new tables and one new read RPC.

```
┌─────────────┐   auto upload (15m)      ┌──────────────────────┐
│  Device A   │  + manual backfill (60d) │  Supabase            │
│  (Tauri)    │ ────────────────────────▶│                      │
└─────────────┘                          │  daily_snapshots     │  ← leaderboard (unchanged)
                                         │  daily_model_        │  ← NEW, per-model rows
┌─────────────┐                          │    snapshots         │
│  Device B   │ ────────────────────────▶│  pricing_models      │  ← NEW, server pricing
│  (Tauri)    │                          └──────────────────────┘
└─────────────┘                                      │
                                                     │ get_my_account_stats()
                                                     ▼
                                         ┌──────────────────────┐
                                         │  Frontend (same app) │
                                         │  statsSource toggle: │
                                         │    "local" | "account"
                                         └──────────────────────┘
```

## Schema

### New table: `daily_model_snapshots`

Normalized: one row per (user, device, date, provider, model). Stores raw token counts only — cost is derived at read time.

```sql
CREATE TABLE daily_model_snapshots (
  user_id              uuid NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  device_id            text NOT NULL,
  date                 date NOT NULL,
  provider             text NOT NULL,            -- 'claude' | 'codex' | 'opencode' | 'kimi' | 'glm'
  model                text NOT NULL,            -- raw model ID, e.g. 'claude-opus-4-7-20260416'
  input_tokens         bigint NOT NULL DEFAULT 0,
  output_tokens        bigint NOT NULL DEFAULT 0,
  cache_read_tokens    bigint NOT NULL DEFAULT 0,
  cache_write_tokens   bigint NOT NULL DEFAULT 0,
  submitted_at         timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, device_id, date, provider, model)
);

CREATE INDEX daily_model_snapshots_user_provider_date_idx
  ON daily_model_snapshots (user_id, provider, date);

ALTER TABLE daily_model_snapshots ENABLE ROW LEVEL SECURITY;

CREATE POLICY "own rows: select" ON daily_model_snapshots
  FOR SELECT USING (auth.uid() = user_id);
CREATE POLICY "own rows: upsert" ON daily_model_snapshots
  FOR INSERT WITH CHECK (auth.uid() = user_id);
CREATE POLICY "own rows: update" ON daily_model_snapshots
  FOR UPDATE USING (auth.uid() = user_id) WITH CHECK (auth.uid() = user_id);
CREATE POLICY "own rows: delete" ON daily_model_snapshots
  FOR DELETE USING (auth.uid() = user_id);
```

### New table: `pricing_models`

Mirrors `src-tauri/pricing.json` structure as rows. Public read; writes only via migrations or service_role.

```sql
CREATE TABLE pricing_models (
  id                   bigserial PRIMARY KEY,
  provider             text NOT NULL,                     -- 'claude' | 'codex' | ...
  match_pattern        text NOT NULL,                     -- substring matched against model
  label                text NOT NULL,
  input_rate           numeric(10, 6) NOT NULL,           -- $ / MTok
  output_rate          numeric(10, 6) NOT NULL,
  cache_read_rate      numeric(10, 6) NOT NULL DEFAULT 0,
  cache_write_rate     numeric(10, 6) NOT NULL DEFAULT 0,
  cache_write_1h_rate  numeric(10, 6) NOT NULL DEFAULT 0,
  priority             int NOT NULL,                       -- lower = matched first
  effective_from       date NOT NULL DEFAULT '1970-01-01', -- inclusive
  effective_until      date,                               -- exclusive; NULL = current
  UNIQUE (provider, match_pattern, effective_from)
);

CREATE INDEX pricing_models_provider_priority_idx
  ON pricing_models (provider, priority);

ALTER TABLE pricing_models ENABLE ROW LEVEL SECURITY;
CREATE POLICY "public read" ON pricing_models FOR SELECT USING (true);
-- No insert/update/delete policy → only service_role can write.
```

Seeding: a one-off Node script reads `src-tauri/pricing.json` and emits an INSERT migration for each entry. Future rate changes are applied as new migrations that close out the current row (`effective_until`) and insert a new one, so historical data continues to price at its then-current rate.

### `daily_snapshots` (existing) — unchanged.

## Upload flow

`useSnapshotUploader.ts` extends to upload **both** the thin leaderboard payload (unchanged) and the new per-model rows.

### New local stat field

The Rust `build_stats` function (`src-tauri/src/providers/claude_code.rs`) currently aggregates model × day data internally but only exposes the daily sum + lifetime model totals. Add a `daily_model_usage: Vec<DailyModelUsage>` field to `AllStats`:

```rust
#[derive(Serialize, Clone)]
pub struct DailyModelUsage {
    pub date: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}
```

This is computed during the existing single pass over `entries` — no extra parse.

### New RPC

```sql
CREATE FUNCTION sync_device_model_rows(
  p_provider  text,
  p_device_id text,
  p_rows      jsonb     -- [{ date, model, input, output, cache_read, cache_write }, ...]
) RETURNS int
LANGUAGE plpgsql SECURITY DEFINER
AS $$
DECLARE
  inserted int;
BEGIN
  IF auth.uid() IS NULL THEN
    RAISE EXCEPTION 'not authenticated';
  END IF;

  INSERT INTO daily_model_snapshots (
    user_id, device_id, date, provider, model,
    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens
  )
  SELECT
    auth.uid(),
    p_device_id,
    (row->>'date')::date,
    p_provider,
    row->>'model',
    COALESCE((row->>'input')::bigint, 0),
    COALESCE((row->>'output')::bigint, 0),
    COALESCE((row->>'cache_read')::bigint, 0),
    COALESCE((row->>'cache_write')::bigint, 0)
  FROM jsonb_array_elements(p_rows) row
  ON CONFLICT (user_id, device_id, date, provider, model) DO UPDATE SET
    input_tokens = EXCLUDED.input_tokens,
    output_tokens = EXCLUDED.output_tokens,
    cache_read_tokens = EXCLUDED.cache_read_tokens,
    cache_write_tokens = EXCLUDED.cache_write_tokens,
    submitted_at = now();

  GET DIAGNOSTICS inserted = ROW_COUNT;
  RETURN inserted;
END;
$$;
```

Last-write-wins upsert, matching the existing `sync_device_snapshots` semantics.

### Client changes

- **Auto-upload path** (`useSnapshotUploader.ts:184-229`): after the existing `callSyncRpc`, call `sync_device_model_rows` with today's rows for the current user's models.
- **Backfill path** (lines 241-269): expand the 60-day window similarly, filter to `stats.daily_model_usage`.
- The two RPCs are called **sequentially** in a single promise chain. If the second fails, the first still counts; a retry is triggered on the next stats-change tick.

## Read flow

```sql
CREATE FUNCTION get_my_account_stats(
  p_provider  text,
  p_date_from date,
  p_date_to   date
) RETURNS TABLE (
  date               date,
  model              text,
  input_tokens       bigint,
  output_tokens      bigint,
  cache_read_tokens  bigint,
  cache_write_tokens bigint,
  cost_usd           numeric
) LANGUAGE sql STABLE
AS $$
  WITH summed AS (
    SELECT s.date, s.model,
           SUM(s.input_tokens)       AS input_tokens,
           SUM(s.output_tokens)      AS output_tokens,
           SUM(s.cache_read_tokens)  AS cache_read_tokens,
           SUM(s.cache_write_tokens) AS cache_write_tokens
    FROM daily_model_snapshots s
    WHERE s.user_id = auth.uid()
      AND s.provider = p_provider
      AND s.date BETWEEN p_date_from AND p_date_to
    GROUP BY s.date, s.model
  ),
  priced AS (
    SELECT s.*,
           (SELECT p.*
              FROM pricing_models p
              WHERE p.provider = p_provider
                AND position(p.match_pattern in s.model) > 0
                AND p.effective_from <= s.date
                AND (p.effective_until IS NULL OR s.date < p.effective_until)
              ORDER BY p.priority ASC
              LIMIT 1) AS pricing
    FROM summed s
  )
  SELECT p.date, p.model,
         p.input_tokens, p.output_tokens, p.cache_read_tokens, p.cache_write_tokens,
         COALESCE(
           (p.input_tokens::numeric       / 1e6) * (p.pricing).input_rate +
           (p.output_tokens::numeric      / 1e6) * (p.pricing).output_rate +
           (p.cache_read_tokens::numeric  / 1e6) * (p.pricing).cache_read_rate +
           (p.cache_write_tokens::numeric / 1e6) * (p.pricing).cache_write_rate,
           0
         ) AS cost_usd
  FROM priced p
  ORDER BY p.date, p.model;
$$;
```

Pricing resolution mirrors the Rust `find_pricing` logic: first-match-by-priority using substring match. The `effective_from`/`effective_until` window ensures historical rows price at their then-current rate.

## Frontend

### State

`SettingsContext` adds:
```ts
statsSource: "local" | "account";
accountSyncEnabled: boolean;  // default: mirror usage_tracking_enabled
```
Both persisted in `~/.claude/ai-token-monitor-prefs.json`.

### Data source hook

New `useAccountStats(provider, since, until)`: returns `AllStats` with `daily`, `model_usage`, and empty analytics (project/tool/heatmap fields are zero'd). Implementation:
1. Call `get_my_account_stats(...)` RPC.
2. Group rows by date → populate `daily[]` with per-model `tokens` map and summed `cost_usd`.
3. Sum rows by model → populate `model_usage` record.

New `useStatsSource(provider)`:
```ts
const { statsSource } = useSettings();
const local = useTokenStats(provider);
const account = useAccountStats(provider, ...);
return statsSource === "account" && user != null ? account : local;
```

### UI changes

- **Header**: add a small `Local ⇄ Account` toggle next to provider tabs. Disabled + tooltip "Sign in to see account stats" when not logged in.
- **Panels** (each reads `useStatsSource` now instead of `useTokenStats`):
  - ✅ Renders normally in both modes: `DailyChart`, `ModelBreakdown`, `PeriodTotals`, `CacheEfficiency`, `TodaySummary`, `Receipt`, `Heatmap` (daily granularity), `AnalyticsSummary` (daily cost/tokens).
  - ⚠ In Account mode, show "Account view" badge + disabled state: `ProjectBreakdown`, `ToolUsage`, `ShellCommands`, `ActivityGraph` (minute-level), `SalaryComparator` (requires per-session).
- **Onboarding**: first time user toggles to Account after signing in, trigger `manualBackfill(60)` automatically (gated by existing `first-visit localStorage flag` pattern used by the leaderboard path).

### Opt-in model

- **`accountSyncEnabled` (new)** gates uploads. Default: true if signed in + `usage_tracking_enabled`. Settings UI surfaces it under the existing sync section.
- **`profiles.leaderboard_hidden` (existing)** continues to gate leaderboard visibility. Decoupled — users can sync their account without appearing on the leaderboard.
- **Logged-out**: no uploads; Account toggle disabled.

## Error handling

- RPC failures on upload: log + skip; next stats-change tick retries. No blocking.
- Pricing-table miss (new model not yet seeded): `get_my_account_stats` returns `cost_usd = 0` for that row. Frontend shows `—` for cost, not a crash.
- Stale pricing on user's browser: N/A — cost is computed server-side.
- Authenticated user with zero rows in `daily_model_snapshots`: frontend gets empty `AllStats`, panels show "no data yet" empty states (existing behavior for local view already handles this).

## Testing

- **Migration test**: apply all migrations on a clean DB; insert a profile + rows; call `get_my_account_stats`; assert aggregated output matches hand-computed values, including cross-device sum and correct pricing per row.
- **Pricing window test**: insert a `pricing_models` row with `effective_until`, insert snapshot rows that span the boundary, assert correct historical vs current pricing.
- **Rust unit tests**: `build_stats` now emits `daily_model_usage`; add a test that asserts the length + per-(date,model) sums match the existing daily totals.
- **Client integration test**: mock the Supabase client; `useSnapshotUploader` uploads both RPCs in sequence on stats change; `manualBackfill` expands to N days × M models.

## Implementation order (milestones — feed into plan)

1. Supabase migration: `daily_model_snapshots` + RLS + indexes.
2. Supabase migration: `pricing_models` + seed script from `pricing.json`.
3. Supabase migration: `sync_device_model_rows` + `get_my_account_stats` functions.
4. Rust: extend `AllStats` with `daily_model_usage` (+ test).
5. Client upload: call new RPC in auto + backfill paths.
6. Client read: `useAccountStats` hook.
7. Frontend: `statsSource` in context + prefs + `useStatsSource` hook.
8. Frontend: Header toggle + panel disabled-state badges.
9. Frontend: first-visit auto-backfill on toggle to Account.
10. Docs: `pricing.json` → migration workflow (how to ship pricing changes to server).

## Open risks

- **Migration drift**: server `pricing_models` and client `pricing.json` must stay in sync. Mitigation: a CI check that runs the seed script against the current `pricing.json` and fails if the diff against the latest migration is non-empty. (Explicit follow-up task in the plan.)
- **Storage growth**: ~10 models × ~60 days × ~3 devices × 5 providers ≈ 9000 rows/user. Negligible.
- **Leaderboard / account drift**: leaderboard sums come from the thin table, account view sums from the model table. If one RPC succeeds and the other fails, totals briefly disagree. Acceptable — both converge within 15 minutes.
