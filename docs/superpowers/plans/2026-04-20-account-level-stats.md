# Account-level Stats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a cross-device account view to ai-token-monitor: a signed-in user sees a unified daily × model-breakdown × cost stats panel aggregated across every device that has uploaded snapshots to their account, with server-side cost calculation so retroactive pricing fixes apply to historical data automatically.

**Architecture:** New normalized `daily_model_snapshots` table (user × device × date × provider × model → raw tokens) + new `pricing_models` table so cost is derived at read time by a new `get_my_account_stats()` RPC. Client extends the existing snapshot uploader to call an additional `sync_device_model_rows` RPC alongside the (unchanged) leaderboard payload. Frontend gets a `statsSource: "local" | "account"` toggle in settings; panels read through a single dispatcher hook that returns either local or account-shaped `AllStats`.

**Tech Stack:** Supabase (Postgres + RLS), Rust (Tauri backend), TypeScript (React + Vite frontend), GitHub OAuth.

**Spec:** `docs/superpowers/specs/2026-04-20-account-level-stats-design.md`.

---

## File Structure

**Create:**
- `supabase/migrations/20260420000000_daily_model_snapshots.sql` — new table + RLS + indexes
- `supabase/migrations/20260420010000_pricing_models.sql` — new table + RLS
- `supabase/migrations/20260420020000_seed_pricing_models.sql` — initial rate rows (generated from `pricing.json`)
- `supabase/migrations/20260420030000_account_stats_rpcs.sql` — `sync_device_model_rows` + `get_my_account_stats`
- `supabase/tests/account_stats_rpcs.sql` — transaction-rolled test mirroring the convention of `sync_device_snapshots_equivalence.sql`
- `scripts/seed-pricing-models.mjs` — one-shot: reads `src-tauri/pricing.json`, emits the seed migration
- `scripts/check-pricing-sync.mjs` — CI drift check: compares `pricing.json` against latest pricing seed migration
- `src/hooks/useAccountStats.ts` — new hook calling `get_my_account_stats` and reshaping to `AllStats`
- `src/hooks/useStatsSource.ts` — dispatcher: returns local or account stats based on settings

**Modify:**
- `src-tauri/src/providers/types.rs` — add `DailyModelUsage`, extend `AllStats` with `daily_model_usage`
- `src-tauri/src/providers/claude_code.rs:240-338` — populate `daily_model_usage` in `build_stats`
- `src/lib/types.ts:54-61` — mirror `AllStats` extension; add `DailyModelUsage`
- `src/lib/types.ts:65-97` — add `stats_source`, `account_sync_enabled` to `UserPreferences`
- `src-tauri/src/providers/types.rs:75-122` — same prefs fields in Rust
- `src/contexts/SettingsContext.tsx` — expose/persist the new prefs
- `src/hooks/useSnapshotUploader.ts:57-269` — add `ModelRowPayload`, call new RPC in auto + backfill paths
- `src/components/Header.tsx` — add `Local / Account` toggle
- `src/components/{ProjectBreakdown,ToolUsage,ShellCommands,ActivityGraph,SalaryComparator}.tsx` — render "Account view" disabled state when `statsSource === "account"`
- `.github/workflows/test.yml` — add `node scripts/check-pricing-sync.mjs` step

**Not touched:** existing `daily_snapshots` schema, `sync_device_snapshots` RPC, `get_leaderboard_entries` RPC, leaderboard UI. Leaderboard keeps working unchanged.

---

## Task 1: Migration — `daily_model_snapshots` table

**Files:**
- Create: `supabase/migrations/20260420000000_daily_model_snapshots.sql`

- [ ] **Step 1: Write the migration**

```sql
-- Normalized per-device, per-day, per-model raw token counts. Cost is NOT
-- stored — it's computed at read time via pricing_models + get_my_account_stats().
-- This keeps historical pricing corrections (e.g. the Opus 4.7 mispricing fixed
-- in #130) applicable retroactively without forcing every client to re-upload.

create table public.daily_model_snapshots (
  user_id              uuid        not null references public.profiles(id) on delete cascade,
  device_id            text        not null,
  date                 date        not null,
  provider             text        not null,
  model                text        not null,
  input_tokens         bigint      not null default 0,
  output_tokens        bigint      not null default 0,
  cache_read_tokens    bigint      not null default 0,
  cache_write_tokens   bigint      not null default 0,
  submitted_at         timestamptz not null default now(),
  primary key (user_id, device_id, date, provider, model)
);

create index daily_model_snapshots_user_provider_date_idx
  on public.daily_model_snapshots (user_id, provider, date);

alter table public.daily_model_snapshots enable row level security;

create policy "own rows: select" on public.daily_model_snapshots
  for select using (auth.uid() = user_id);

create policy "own rows: insert" on public.daily_model_snapshots
  for insert with check (auth.uid() = user_id);

create policy "own rows: update" on public.daily_model_snapshots
  for update using (auth.uid() = user_id) with check (auth.uid() = user_id);

create policy "own rows: delete" on public.daily_model_snapshots
  for delete using (auth.uid() = user_id);
```

- [ ] **Step 2: Apply locally to verify syntax**

Run: `supabase db reset` (applies all migrations from scratch on local dev DB)
Expected: completes without error; `\d public.daily_model_snapshots` in psql shows the table + 4 policies.

- [ ] **Step 3: Commit**

```bash
git add supabase/migrations/20260420000000_daily_model_snapshots.sql
git commit -m "feat(supabase): add daily_model_snapshots table for per-model usage"
```

---

## Task 2: Migration — `pricing_models` table

**Files:**
- Create: `supabase/migrations/20260420010000_pricing_models.sql`

- [ ] **Step 1: Write the migration**

```sql
-- Server-side pricing table. Mirrors src-tauri/pricing.json but keyed by
-- effective_from/until so historical snapshot rows price at their then-current
-- rate. Public read (so anonymous leaderboard views could in theory show cost
-- if ever needed); writes are service_role-only (via migrations).

create table public.pricing_models (
  id                    bigserial primary key,
  provider              text           not null,
  match_pattern         text           not null,
  label                 text           not null,
  input_rate            numeric(10, 6) not null,
  output_rate           numeric(10, 6) not null,
  cache_read_rate       numeric(10, 6) not null default 0,
  cache_write_rate      numeric(10, 6) not null default 0,
  cache_write_1h_rate   numeric(10, 6) not null default 0,
  priority              int            not null,
  effective_from        date           not null default '1970-01-01',
  effective_until       date,
  unique (provider, match_pattern, effective_from)
);

create index pricing_models_provider_priority_idx
  on public.pricing_models (provider, priority);

alter table public.pricing_models enable row level security;

-- Public read, no write policy (service_role bypasses RLS for migrations).
create policy "public read" on public.pricing_models
  for select using (true);

comment on table public.pricing_models is
  'Server-side token pricing. Seeded from src-tauri/pricing.json. See docs/superpowers/specs/2026-04-20-account-level-stats-design.md.';
```

- [ ] **Step 2: Apply locally**

Run: `supabase db reset`
Expected: table created; `select count(*) from pricing_models;` returns 0 (seeded in next task).

- [ ] **Step 3: Commit**

```bash
git add supabase/migrations/20260420010000_pricing_models.sql
git commit -m "feat(supabase): add pricing_models table for server-side cost calc"
```

---

## Task 3: Seed script + seed migration for `pricing_models`

**Files:**
- Create: `scripts/seed-pricing-models.mjs`
- Create: `supabase/migrations/20260420020000_seed_pricing_models.sql` (generated)

- [ ] **Step 1: Write the seed generator script**

```javascript
// scripts/seed-pricing-models.mjs
// Reads src-tauri/pricing.json and emits a Supabase migration that seeds
// pricing_models. Run as `node scripts/seed-pricing-models.mjs > OUT.sql`
// or with --check to verify the latest seed migration matches.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pricingPath = join(__dirname, "..", "src-tauri", "pricing.json");
const pricing = JSON.parse(readFileSync(pricingPath, "utf8"));

const escape = (s) => String(s).replace(/'/g, "''");

function emit(provider, models) {
  const rows = [];
  models.forEach((m, i) => {
    const input = m.input ?? 0;
    const output = m.output ?? 0;
    const cacheRead = m.cache_read ?? 0;
    const cacheWrite = m.cache_write ?? 0;
    const cacheWrite1h = m.cache_write_1h ?? cacheWrite;
    rows.push(
      `  ('${provider}', '${escape(m.match)}', '${escape(m.label ?? m.match)}', ${input}, ${output}, ${cacheRead}, ${cacheWrite}, ${cacheWrite1h}, ${i})`,
    );
  });
  return rows;
}

const allRows = [];
for (const provider of ["claude", "codex", "opencode", "kimi", "glm"]) {
  const cfg = pricing[provider];
  if (!cfg?.models) continue;
  allRows.push(...emit(provider, cfg.models));
}

const sql = `-- Auto-generated from src-tauri/pricing.json v${pricing.version} (${pricing.last_updated}).
-- Regenerate via: node scripts/seed-pricing-models.mjs > supabase/migrations/<ts>_seed_pricing_models.sql

insert into public.pricing_models
  (provider, match_pattern, label, input_rate, output_rate, cache_read_rate, cache_write_rate, cache_write_1h_rate, priority)
values
${allRows.join(",\n")}
on conflict (provider, match_pattern, effective_from) do update set
  label               = excluded.label,
  input_rate          = excluded.input_rate,
  output_rate         = excluded.output_rate,
  cache_read_rate     = excluded.cache_read_rate,
  cache_write_rate    = excluded.cache_write_rate,
  cache_write_1h_rate = excluded.cache_write_1h_rate,
  priority            = excluded.priority;
`;

process.stdout.write(sql);
```

- [ ] **Step 2: Run the generator and verify output**

```bash
node scripts/seed-pricing-models.mjs > supabase/migrations/20260420020000_seed_pricing_models.sql
head -10 supabase/migrations/20260420020000_seed_pricing_models.sql
```
Expected: migration starts with `-- Auto-generated from src-tauri/pricing.json v1.2.0`, contains rows for opus-4-7, sonnet-4-6, gpt-5.4, etc.

- [ ] **Step 3: Apply locally and sanity-check a row**

Run: `supabase db reset`
Then in psql: `select provider, match_pattern, input_rate, output_rate from pricing_models where match_pattern = 'opus-4-7' and provider = 'claude';`
Expected: `claude | opus-4-7 | 5.000000 | 25.000000`.

- [ ] **Step 4: Commit**

```bash
git add scripts/seed-pricing-models.mjs supabase/migrations/20260420020000_seed_pricing_models.sql
git commit -m "feat: seed pricing_models from pricing.json"
```

---

## Task 4: Migration — `sync_device_model_rows` + `get_my_account_stats` RPCs

**Files:**
- Create: `supabase/migrations/20260420030000_account_stats_rpcs.sql`

- [ ] **Step 1: Write the migration**

```sql
-- Upload: bulk upsert per-model rows for (user, device, date). Last-write-wins.
create or replace function public.sync_device_model_rows(
  p_provider  text,
  p_device_id text,
  p_rows      jsonb
) returns integer
language plpgsql
security definer
set search_path = public
as $$
declare
  v_count integer;
begin
  if auth.uid() is null then
    raise exception 'not authenticated' using errcode = '42501';
  end if;

  insert into public.daily_model_snapshots (
    user_id, device_id, date, provider, model,
    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens
  )
  select
    auth.uid(),
    p_device_id,
    (r->>'date')::date,
    p_provider,
    r->>'model',
    coalesce((r->>'input')::bigint,       0),
    coalesce((r->>'output')::bigint,      0),
    coalesce((r->>'cache_read')::bigint,  0),
    coalesce((r->>'cache_write')::bigint, 0)
  from jsonb_array_elements(p_rows) r
  on conflict (user_id, device_id, date, provider, model) do update set
    input_tokens       = excluded.input_tokens,
    output_tokens      = excluded.output_tokens,
    cache_read_tokens  = excluded.cache_read_tokens,
    cache_write_tokens = excluded.cache_write_tokens,
    submitted_at       = now();

  get diagnostics v_count = row_count;
  return v_count;
end;
$$;

grant execute on function public.sync_device_model_rows(text, text, jsonb) to authenticated;

-- Read: cross-device aggregated stats for the calling user, with cost priced
-- at read time using the pricing row that was effective on that day.
create or replace function public.get_my_account_stats(
  p_provider  text,
  p_date_from date,
  p_date_to   date
) returns table (
  date               date,
  model              text,
  input_tokens       bigint,
  output_tokens      bigint,
  cache_read_tokens  bigint,
  cache_write_tokens bigint,
  cost_usd           numeric
)
language sql
stable
security invoker
set search_path = public
as $$
  with summed as (
    select s.date, s.model,
           sum(s.input_tokens)       as input_tokens,
           sum(s.output_tokens)      as output_tokens,
           sum(s.cache_read_tokens)  as cache_read_tokens,
           sum(s.cache_write_tokens) as cache_write_tokens
      from public.daily_model_snapshots s
     where s.user_id  = auth.uid()
       and s.provider = p_provider
       and s.date    between p_date_from and p_date_to
     group by s.date, s.model
  ),
  priced as (
    select s.*,
           (
             select p.id
               from public.pricing_models p
              where p.provider = p_provider
                and position(p.match_pattern in s.model) > 0
                and p.effective_from <= s.date
                and (p.effective_until is null or s.date < p.effective_until)
              order by p.priority asc
              limit 1
           ) as pricing_id
      from summed s
  )
  select p.date, p.model,
         p.input_tokens, p.output_tokens, p.cache_read_tokens, p.cache_write_tokens,
         coalesce(
           (p.input_tokens::numeric       / 1e6) * pm.input_rate +
           (p.output_tokens::numeric      / 1e6) * pm.output_rate +
           (p.cache_read_tokens::numeric  / 1e6) * pm.cache_read_rate +
           (p.cache_write_tokens::numeric / 1e6) * pm.cache_write_rate,
           0::numeric
         ) as cost_usd
    from priced p
    left join public.pricing_models pm on pm.id = p.pricing_id
   order by p.date asc, p.model asc;
$$;

grant execute on function public.get_my_account_stats(text, date, date) to authenticated;
```

- [ ] **Step 2: Apply locally**

Run: `supabase db reset`
Expected: both functions created; `\df public.sync_device_model_rows public.get_my_account_stats` shows them.

- [ ] **Step 3: Commit**

```bash
git add supabase/migrations/20260420030000_account_stats_rpcs.sql
git commit -m "feat(supabase): add sync_device_model_rows + get_my_account_stats RPCs"
```

---

## Task 5: SQL test — RPCs produce correct cross-device aggregates and cost

**Files:**
- Create: `supabase/tests/account_stats_rpcs.sql`

- [ ] **Step 1: Write the test (runs inside a transaction that rolls back)**

```sql
-- Equivalence/correctness test for sync_device_model_rows + get_my_account_stats.
-- Inserts two devices' worth of snapshots for one synthetic user, then asserts
-- that get_my_account_stats returns the cross-device SUM and that cost reflects
-- the pricing_models row effective on the snapshot date.
--
-- Run inside a single transaction so everything rolls back; does NOT require
-- auth.uid() — we insert directly into daily_model_snapshots using a fixed UUID
-- to bypass the RPC's auth.uid() check.

begin;

-- 1. Synthetic user (no FK to profiles during test; we test the shape of the
--    aggregation, not RLS enforcement, which is exercised separately).
alter table public.daily_model_snapshots drop constraint daily_model_snapshots_user_id_fkey;

-- 2. Seed: 2026-04-20, two devices, two models, under one user.
do $$
declare
  v_user constant uuid := '11111111-1111-1111-1111-111111111111';
begin
  insert into public.daily_model_snapshots (user_id, device_id, date, provider, model,
                                            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens)
  values
    (v_user, 'dev_a', '2026-04-20', 'claude', 'claude-opus-4-7-20260416',
       1000000, 500000, 10000000, 100000),          -- Opus 4.7: $5 in + $12.5 out + $5 cr + $0.625 cw = $23.125
    (v_user, 'dev_b', '2026-04-20', 'claude', 'claude-opus-4-7-20260416',
       2000000, 1000000, 20000000, 200000),         -- Opus 4.7 on device B
    (v_user, 'dev_a', '2026-04-20', 'claude', 'claude-sonnet-4-6-20260320',
       3000000, 500000, 0, 0);                      -- Sonnet 4.6 on device A
end $$;

-- 3. Run the RPC with a forced auth context.
set local role authenticated;
set local request.jwt.claim.sub = '11111111-1111-1111-1111-111111111111';

-- 4. Assertions.
do $$
declare
  r record;
  opus_total_tokens bigint;
  opus_cost numeric;
  sonnet_cost numeric;
begin
  -- Total rows expected: 1 day × 2 distinct models = 2.
  select count(*) into strict opus_total_tokens
    from public.get_my_account_stats('claude', '2026-04-20', '2026-04-20');
  if opus_total_tokens <> 2 then
    raise exception 'Expected 2 aggregated rows, got %', opus_total_tokens;
  end if;

  -- Opus 4.7 row: cross-device SUM(input) = 3_000_000; SUM(output) = 1_500_000;
  -- SUM(cache_read) = 30_000_000; SUM(cache_write) = 300_000.
  -- At $5/$25/$0.5/$6.25 per MTok → cost = 15 + 37.5 + 15 + 1.875 = 69.375.
  select r2.input_tokens, r2.cost_usd
    into strict opus_total_tokens, opus_cost
    from public.get_my_account_stats('claude', '2026-04-20', '2026-04-20') r2
   where r2.model = 'claude-opus-4-7-20260416';
  if opus_total_tokens <> 3000000 then
    raise exception 'Opus input SUM expected 3000000, got %', opus_total_tokens;
  end if;
  if abs(opus_cost - 69.375) > 0.001 then
    raise exception 'Opus cost expected 69.375, got %', opus_cost;
  end if;

  -- Sonnet 4.6 row: single device; 3M input + 0.5M output → 9 + 7.5 = 16.5.
  select r2.cost_usd into strict sonnet_cost
    from public.get_my_account_stats('claude', '2026-04-20', '2026-04-20') r2
   where r2.model = 'claude-sonnet-4-6-20260320';
  if abs(sonnet_cost - 16.5) > 0.001 then
    raise exception 'Sonnet cost expected 16.5, got %', sonnet_cost;
  end if;
end $$;

-- All asserts passed. Roll back.
rollback;
```

- [ ] **Step 2: Run the test**

Run: `psql "$DATABASE_URL" -f supabase/tests/account_stats_rpcs.sql`
(Or against local: `psql postgresql://postgres:postgres@127.0.0.1:54322/postgres -f supabase/tests/account_stats_rpcs.sql`)
Expected: `ROLLBACK` at end, no RAISE EXCEPTION fired, exit code 0.

- [ ] **Step 3: Commit**

```bash
git add supabase/tests/account_stats_rpcs.sql
git commit -m "test(supabase): account stats RPC cross-device aggregation + cost"
```

---

## Task 6: Rust — add `DailyModelUsage` type + extend `AllStats`

**Files:**
- Modify: `src-tauri/src/providers/types.rs:64-73`

- [ ] **Step 1: Add the type**

Edit `src-tauri/src/providers/types.rs` — add after `ModelUsage` (around line 25):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyModelUsage {
    pub date: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}
```

And extend `AllStats` (line 65-73):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllStats {
    pub daily: Vec<DailyUsage>,
    pub model_usage: HashMap<String, ModelUsage>,
    #[serde(default)]
    pub daily_model_usage: Vec<DailyModelUsage>,
    pub total_sessions: u32,
    pub total_messages: u32,
    pub first_session_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analytics: Option<AnalyticsData>,
}
```

- [ ] **Step 2: Fix all construction sites**

Run: `cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | grep 'AllStats {' | head`
Expected: compiler flags each construction site of `AllStats { ... }` where `daily_model_usage` is missing. Fix each by adding `daily_model_usage: Vec::new(),` (or a computed value — Task 7 fills real data).

Known sites (grep the codebase to confirm):
- `src-tauri/src/providers/claude_code.rs:330-338` (inside `build_stats`)
- `src-tauri/src/providers/codex.rs` (if it builds `AllStats`)
- `src-tauri/src/providers/opencode.rs`
- `src-tauri/src/providers/kimi.rs`

For every non-claude provider that doesn't track per-model daily data, pass `daily_model_usage: Vec::new()` for now (account view is claude-focused in v1; other providers can extend later).

- [ ] **Step 3: Verify it builds**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: exit 0, no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/providers/types.rs src-tauri/src/providers/*.rs
git commit -m "feat(rust): add DailyModelUsage + extend AllStats"
```

---

## Task 7: Rust — populate `daily_model_usage` in `build_stats`

**Files:**
- Modify: `src-tauri/src/providers/claude_code.rs:240-338`

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/providers/claude_code.rs` tests module (find `#[cfg(test)] mod tests`):

```rust
#[test]
fn build_stats_populates_daily_model_usage() {
    use std::collections::HashMap;

    let mut entries: HashMap<String, SessionEntry> = HashMap::new();
    let mk = |id: u32, date: &str, model: &str, inp: u64, out: u64, cr: u64, cw: u64| SessionEntry {
        date: date.to_string(),
        timestamp: format!("{}T10:00:00Z", date),
        model: model.to_string(),
        session_id: format!("s{}", id),
        message_id: format!("m{}", id),
        request_id: format!("r{}", id),
        input_tokens: inp,
        output_tokens: out,
        cache_read_input_tokens: cr,
        cache_creation_input_tokens: cw,
        cache_creation_5m_tokens: cw,
        cache_creation_1h_tokens: 0,
        web_search_requests: 0,
        cwd: String::new(),
        tool_names: Vec::new(),
        bash_commands: Vec::new(),
    };
    // Two entries on the same (date, model) + one on a different day → expect 2 DailyModelUsage rows.
    entries.insert("k1".to_string(), mk(1, "2026-04-19", "claude-opus-4-7", 100, 200, 50, 10));
    entries.insert("k2".to_string(), mk(2, "2026-04-19", "claude-opus-4-7", 300, 400, 150, 20));
    entries.insert("k3".to_string(), mk(3, "2026-04-20", "claude-sonnet-4-6", 500, 600, 0, 0));

    let provider = ClaudeCodeProvider::new(vec![]);
    let stats = provider.build_stats(&entries);

    assert_eq!(stats.daily_model_usage.len(), 2, "expected 2 (date,model) combos, got {}", stats.daily_model_usage.len());

    let opus = stats.daily_model_usage.iter()
        .find(|d| d.date == "2026-04-19" && d.model == "claude-opus-4-7")
        .expect("opus entry missing");
    assert_eq!(opus.input_tokens, 400);
    assert_eq!(opus.output_tokens, 600);
    assert_eq!(opus.cache_read_tokens, 200);
    assert_eq!(opus.cache_write_tokens, 30);

    let sonnet = stats.daily_model_usage.iter()
        .find(|d| d.date == "2026-04-20" && d.model == "claude-sonnet-4-6")
        .expect("sonnet entry missing");
    assert_eq!(sonnet.input_tokens, 500);
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml build_stats_populates_daily_model_usage -- --nocapture`
Expected: FAIL — assertion `stats.daily_model_usage.len() == 2` fails (field is still `Vec::new()`).

- [ ] **Step 3: Add the aggregation to `build_stats`**

In `src-tauri/src/providers/claude_code.rs`, inside `build_stats` — locate the loop over `entries.values()` (around line 246):

Before the loop, add:
```rust
let mut daily_model_map: HashMap<(String, String), DailyModelUsage> = HashMap::new();
```

Inside the loop, after computing `total_tokens`, before the `daily_map.entry(...)` call:
```rust
let dm_key = (entry.date.clone(), entry.model.clone());
let dm = daily_model_map.entry(dm_key).or_insert_with(|| DailyModelUsage {
    date: entry.date.clone(),
    model: entry.model.clone(),
    input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0,
});
dm.input_tokens       += entry.input_tokens;
dm.output_tokens      += entry.output_tokens;
dm.cache_read_tokens  += entry.cache_read_input_tokens;
dm.cache_write_tokens += entry.cache_creation_input_tokens;
```

In the final `AllStats { ... }` construction (around line 330-338), change `daily_model_usage: Vec::new()` to:
```rust
daily_model_usage: daily_model_map.into_values().collect(),
```

Also `use super::types::DailyModelUsage;` at the top of `claude_code.rs` if not already imported.

- [ ] **Step 4: Run the test — should now pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml build_stats_populates_daily_model_usage`
Expected: PASS.

- [ ] **Step 5: Run the full test suite to confirm no regression**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/providers/claude_code.rs
git commit -m "feat(rust): aggregate daily_model_usage in build_stats"
```

---

## Task 8: TypeScript — mirror the type in `src/lib/types.ts`

**Files:**
- Modify: `src/lib/types.ts:54-97`

- [ ] **Step 1: Add `DailyModelUsage` + extend `AllStats` + add prefs fields**

Edit `src/lib/types.ts`:

After `ModelUsage` (around line 20), add:
```typescript
export interface DailyModelUsage {
  date: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
}
```

Extend `AllStats` (lines 54-61):
```typescript
export interface AllStats {
  daily: DailyUsage[];
  model_usage: Record<string, ModelUsage>;
  daily_model_usage?: DailyModelUsage[];
  total_sessions: number;
  total_messages: number;
  first_session_date: string | null;
  analytics?: AnalyticsData;
}
```

In `UserPreferences` (lines 65-97), add two fields:
```typescript
  stats_source: "local" | "account";
  account_sync_enabled: boolean;
```

- [ ] **Step 2: Verify types compile**

Run: `npx tsc --noEmit`
Expected: exit 0. (If it fails, check for UserPreferences construction sites that need defaults — see Task 9 for Rust-side defaults.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/types.ts
git commit -m "feat(ts): mirror DailyModelUsage + account view prefs"
```

---

## Task 9: Rust — add `stats_source` + `account_sync_enabled` to `UserPreferences`

**Files:**
- Modify: `src-tauri/src/providers/types.rs:75-122, 237-266`

- [ ] **Step 1: Add the fields**

In `UserPreferences` struct (around line 121), add before `quick_action_items`:
```rust
    #[serde(default = "default_stats_source")]
    pub stats_source: String,
    #[serde(default)]
    pub account_sync_enabled: bool,
```

Add the default helper (near other `default_*` fns, ~line 177):
```rust
fn default_stats_source() -> String {
    "local".to_string()
}
```

In `impl Default for UserPreferences` (around line 237-266), add to the struct:
```rust
    stats_source: default_stats_source(),
    account_sync_enabled: false,
```

- [ ] **Step 2: Verify it builds**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/providers/types.rs
git commit -m "feat(rust): add stats_source + account_sync_enabled to prefs"
```

---

## Task 10: Client — upload per-model rows in `useSnapshotUploader`

**Files:**
- Modify: `src/hooks/useSnapshotUploader.ts:57-269`

- [ ] **Step 1: Add payload type + RPC caller**

Edit `src/hooks/useSnapshotUploader.ts`. After `RowPayload` interface (line 57-63), add:

```typescript
interface ModelRowPayload {
  date: string;
  model: string;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
}

function buildModelRowsForDate(stats: AllStats, date: string): ModelRowPayload[] {
  const src = stats.daily_model_usage ?? [];
  return src
    .filter((d) => d.date === date)
    .map((d) => ({
      date: d.date,
      model: d.model,
      input: d.input_tokens,
      output: d.output_tokens,
      cache_read: d.cache_read_tokens,
      cache_write: d.cache_write_tokens,
    }));
}

function buildModelRowsInRange(stats: AllStats, startStr: string, today: string): ModelRowPayload[] {
  const src = stats.daily_model_usage ?? [];
  return src
    .filter((d) => d.date >= startStr && d.date <= today)
    .map((d) => ({
      date: d.date,
      model: d.model,
      input: d.input_tokens,
      output: d.output_tokens,
      cache_read: d.cache_read_tokens,
      cache_write: d.cache_write_tokens,
    }));
}

async function callSyncModelRpc(
  provider: LeaderboardProvider,
  deviceId: string,
  rows: ModelRowPayload[],
): Promise<boolean> {
  if (!supabase || rows.length === 0) return true; // nothing to do ≠ failure
  const { error } = await supabase.rpc("sync_device_model_rows", {
    p_provider: provider,
    p_device_id: deviceId,
    p_rows: rows,
  });
  if (error) console.warn("[snapshot] sync_device_model_rows failed", error.message);
  return !error;
}
```

- [ ] **Step 2: Wire it into the auto-upload path**

Inside the `attempt` function (line 187-220), after the successful `callSyncRpc` block, add:

```typescript
      const modelRows = buildModelRowsForDate(liveStats, today);
      if (modelRows.length > 0) {
        await callSyncModelRpc(provider, deviceId, modelRows);
        // Non-fatal: even if model rows fail, leaderboard row already uploaded.
      }
```

- [ ] **Step 3: Wire it into `manualBackfill`**

Inside `manualBackfill` (line 241-269), after the successful `callSyncRpc` and before the `if (ok)` state update, add:

```typescript
      const modelRows = buildModelRowsInRange(stats, startStr, today);
      if (modelRows.length > 0) {
        await callSyncModelRpc(provider, deviceId, modelRows);
      }
```

- [ ] **Step 4: Type-check and lint**

Run: `npx tsc --noEmit`
Expected: exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/hooks/useSnapshotUploader.ts
git commit -m "feat: upload per-model rows alongside leaderboard snapshot"
```

---

## Task 11: Client — `useAccountStats` hook

**Files:**
- Create: `src/hooks/useAccountStats.ts`

- [ ] **Step 1: Write the hook**

```typescript
// src/hooks/useAccountStats.ts
import { useEffect, useRef, useState } from "react";
import { supabase } from "../lib/supabase";
import type { AllStats, DailyUsage, LeaderboardProvider, ModelUsage } from "../lib/types";
import type { User } from "@supabase/supabase-js";
import { toLocalDateStr } from "../lib/format";

interface AccountStatsRow {
  date: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  cost_usd: number;
}

interface UseAccountStatsArgs {
  provider: LeaderboardProvider;
  user: User | null;
  enabled: boolean;       // false when the feature is off (statsSource !== "account")
  daysBack?: number;      // default 60
}

const REFRESH_MS = 5 * 60 * 1000;

export function useAccountStats({ provider, user, enabled, daysBack = 60 }: UseAccountStatsArgs) {
  const [stats, setStats] = useState<AllStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (!enabled || !user || !supabase) {
      setStats(null);
      return;
    }

    let cancelled = false;

    const fetchOnce = async () => {
      setLoading(true);
      setError(null);
      const today = new Date();
      const start = new Date();
      start.setDate(start.getDate() - (daysBack - 1));
      const { data, error: err } = await supabase!.rpc("get_my_account_stats", {
        p_provider: provider,
        p_date_from: toLocalDateStr(start),
        p_date_to: toLocalDateStr(today),
      });
      if (cancelled) return;
      if (err) {
        setError(err.message);
        setLoading(false);
        return;
      }
      setStats(reshape(data as AccountStatsRow[] | null));
      setLoading(false);
    };

    fetchOnce();
    pollRef.current = setInterval(fetchOnce, REFRESH_MS);

    return () => {
      cancelled = true;
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [provider, user?.id, enabled, daysBack]);

  return { stats, loading, error };
}

function reshape(rows: AccountStatsRow[] | null): AllStats {
  if (!rows || rows.length === 0) {
    return {
      daily: [],
      model_usage: {},
      total_sessions: 0,
      total_messages: 0,
      first_session_date: null,
    };
  }
  const dailyMap = new Map<string, DailyUsage>();
  const modelMap: Record<string, ModelUsage> = {};
  let firstDate: string | null = null;

  for (const r of rows) {
    if (firstDate == null || r.date < firstDate) firstDate = r.date;

    const day = dailyMap.get(r.date) ?? {
      date: r.date,
      tokens: {},
      cost_usd: 0,
      messages: 0,
      sessions: 0,
      tool_calls: 0,
      input_tokens: 0,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_write_tokens: 0,
    };
    day.tokens[r.model] = (day.tokens[r.model] ?? 0) +
      r.input_tokens + r.output_tokens + r.cache_read_tokens + r.cache_write_tokens;
    day.cost_usd          += Number(r.cost_usd);
    day.input_tokens      += r.input_tokens;
    day.output_tokens     += r.output_tokens;
    day.cache_read_tokens += r.cache_read_tokens;
    day.cache_write_tokens+= r.cache_write_tokens;
    dailyMap.set(r.date, day);

    const mu = modelMap[r.model] ?? { input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0 };
    mu.input_tokens   += r.input_tokens;
    mu.output_tokens  += r.output_tokens;
    mu.cache_read     += r.cache_read_tokens;
    mu.cache_write    += r.cache_write_tokens;
    mu.cost_usd       += Number(r.cost_usd);
    modelMap[r.model]  = mu;
  }

  const daily = Array.from(dailyMap.values()).sort((a, b) => a.date.localeCompare(b.date));
  return {
    daily,
    model_usage: modelMap,
    total_sessions: 0,       // not tracked in account view
    total_messages: 0,       // not tracked in account view
    first_session_date: firstDate,
  };
}
```

- [ ] **Step 2: Type-check**

Run: `npx tsc --noEmit`
Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add src/hooks/useAccountStats.ts
git commit -m "feat(ts): useAccountStats hook reads get_my_account_stats RPC"
```

---

## Task 12: Client — `statsSource` in SettingsContext + `useStatsSource` dispatcher

**Files:**
- Modify: `src/contexts/SettingsContext.tsx`
- Create: `src/hooks/useStatsSource.ts`

- [ ] **Step 1: Expose `stats_source` + `account_sync_enabled` in `SettingsContext`**

Open `src/contexts/SettingsContext.tsx` and locate where prefs fields are exposed (where existing flags like `leaderboard_opted_in`, `usage_tracking_enabled` are read from the loaded prefs object). Add the new fields to the context value — mirroring the existing pattern for boolean toggles. Also add update functions `setStatsSource(v)` and `setAccountSyncEnabled(v)` that persist via whatever the existing pattern is (usually a tauri invoke of `save_preferences`).

If the existing pattern uses a single `setPreferences(partial)` call, just include the new fields in the persisted struct — no dedicated setters needed.

Verify: `grep -n 'stats_source\|account_sync_enabled' src/contexts/SettingsContext.tsx` finds 2 references each.

- [ ] **Step 2: Write the dispatcher hook**

Create `src/hooks/useStatsSource.ts`:

```typescript
import { useSettings } from "../contexts/SettingsContext";
import { useAuth } from "../contexts/AuthContext";
import { useTokenStats } from "./useTokenStats";
import { useAccountStats } from "./useAccountStats";
import type { LeaderboardProvider } from "../lib/types";

/**
 * Returns either local file-based stats (default) or server-aggregated
 * cross-device stats based on SettingsContext.stats_source. Falls back to
 * local whenever the user is signed out, since account mode requires auth.
 */
export function useStatsSource(provider: LeaderboardProvider) {
  const { preferences } = useSettings();
  const { user } = useAuth();
  const local = useTokenStats(provider);

  const useAccount = preferences.stats_source === "account" && user != null;
  const account = useAccountStats({ provider, user, enabled: useAccount });

  if (useAccount) {
    return { stats: account.stats, loading: account.loading, error: account.error };
  }
  return local;
}
```

(Adapt import paths for `useTokenStats`: confirm the export shape matches `{ stats, loading, error }`. If it differs, align the return shape.)

- [ ] **Step 3: Type-check**

Run: `npx tsc --noEmit`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/contexts/SettingsContext.tsx src/hooks/useStatsSource.ts
git commit -m "feat(ts): stats_source dispatcher + settings field"
```

---

## Task 13: UI — Header `Local / Account` toggle

**Files:**
- Modify: `src/components/Header.tsx`
- Modify: `src/i18n/locales/en.json` (+ optionally zh-CN.json for user's default locale)

- [ ] **Step 1: Add i18n strings**

In `src/i18n/locales/en.json`, add:
```json
  "header.stats_source.local": "Local",
  "header.stats_source.account": "Account",
  "header.stats_source.needs_signin": "Sign in to sync stats across devices",
```
In `src/i18n/locales/zh-CN.json`:
```json
  "header.stats_source.local": "本机",
  "header.stats_source.account": "账号",
  "header.stats_source.needs_signin": "登录后可查看跨设备统计",
```

- [ ] **Step 2: Add toggle UI in `Header.tsx`**

Find a suitable slot in the header (near provider tabs or settings gear). Add a small segmented toggle:

```tsx
{user ? (
  <div className="inline-flex border rounded overflow-hidden text-xs">
    <button
      type="button"
      className={preferences.stats_source === "local" ? "bg-accent px-2 py-1" : "px-2 py-1"}
      onClick={() => setPreferences({ ...preferences, stats_source: "local" })}
    >
      {t("header.stats_source.local")}
    </button>
    <button
      type="button"
      className={preferences.stats_source === "account" ? "bg-accent px-2 py-1" : "px-2 py-1"}
      onClick={() => setPreferences({ ...preferences, stats_source: "account" })}
    >
      {t("header.stats_source.account")}
    </button>
  </div>
) : (
  <span className="text-xs text-muted" title={t("header.stats_source.needs_signin")}>
    {t("header.stats_source.local")}
  </span>
)}
```

Adapt class names to the project's existing Tailwind / CSS convention by inspecting existing header buttons. The goal is: signed-in → two-segment toggle; signed-out → static "Local" label with tooltip.

- [ ] **Step 3: Wire panel consumers through `useStatsSource`**

Search for call sites of `useTokenStats` that feed the panels listed as "renders normally in both modes" in the spec:
```bash
grep -rn "useTokenStats" src/ | head -20
```
For each such file, swap `useTokenStats(provider)` → `useStatsSource(provider)`.

**Do NOT swap** call sites inside components that show local-only data (`ProjectBreakdown`, `ToolUsage`, `ShellCommands`, `ActivityGraph`, `SalaryComparator`) — those keep using `useTokenStats` AND gain a disabled-state branch (Task 14).

- [ ] **Step 4: Manual smoke test**

Run: `npm run tauri dev`
- App opens. Header shows `Local / Account` toggle when signed in, static `Local` when signed out.
- Toggle to Account (while signed in) → panels switch to server data (may be empty on first run — expected; fixed by Task 15 auto-backfill).
- Toggle back to Local → panels restore.
- Sign out → toggle collapses to static Local.

- [ ] **Step 5: Commit**

```bash
git add src/components/Header.tsx src/i18n/locales/en.json src/i18n/locales/zh-CN.json src/
git commit -m "feat(ui): Local/Account stats-source toggle in Header"
```

---

## Task 14: UI — disabled state for local-only panels in Account mode

**Files:**
- Modify: `src/components/ProjectBreakdown.tsx`
- Modify: `src/components/ToolUsage.tsx`
- Modify: `src/components/ShellCommands.tsx`
- Modify: `src/components/ActivityGraph.tsx`
- Modify: `src/components/SalaryComparator.tsx`

- [ ] **Step 1: Add a shared "account-mode notice" pattern**

For each of the 5 components listed, at the top of the render function add:

```tsx
const { preferences } = useSettings();
if (preferences.stats_source === "account") {
  return (
    <div className="opacity-50 pointer-events-none relative">
      {/* existing rendering with empty/placeholder data */}
      <div className="absolute inset-0 flex items-center justify-center bg-background/60 text-sm">
        {t("panels.account_mode_unavailable")}
      </div>
    </div>
  );
}
```

Add the i18n key to `en.json` and `zh-CN.json`:
```json
  "panels.account_mode_unavailable": "Not available in Account view — switch to Local"
```
(CN: `"Account 模式下不可用 — 切到 Local 查看"`)

Adapt the markup to the project's existing CSS. Follow the pattern of any existing "feature disabled" overlays in the codebase.

- [ ] **Step 2: Smoke test**

Run: `npm run tauri dev`. Switch to Account mode. Verify the 5 panels show the disabled overlay and are visually greyed.

- [ ] **Step 3: Commit**

```bash
git add src/components/{ProjectBreakdown,ToolUsage,ShellCommands,ActivityGraph,SalaryComparator}.tsx src/i18n/locales/*.json
git commit -m "feat(ui): disabled state for local-only panels in Account mode"
```

---

## Task 15: First-visit auto-backfill on switch to Account

**Files:**
- Modify: `src/components/Header.tsx`

- [ ] **Step 1: Trigger `manualBackfill(60)` once per (user, provider) on first Account toggle**

In the button handler that sets `stats_source` to `"account"`, add:

```tsx
const onSwitchToAccount = () => {
  setPreferences({ ...preferences, stats_source: "account" });
  if (!user) return;
  for (const provider of ["claude", "codex", "opencode", "kimi", "glm"] as LeaderboardProvider[]) {
    if (!preferences[`include_${provider}` as keyof typeof preferences]) continue;
    const flagKey = `account-backfill:${user.id}:${provider}`;
    if (localStorage.getItem(flagKey)) continue;
    // Manual backfill is exposed by useSnapshotUploader. Re-use the same
    // entry-point the leaderboard first-visit path already uses.
    window.dispatchEvent(new CustomEvent("account-first-visit-backfill", { detail: { provider } }));
    localStorage.setItem(flagKey, "1");
  }
};
```

In `useSnapshotUploader`, listen for `account-first-visit-backfill` events and call `manualBackfill(60)` when the detail provider matches:

```typescript
useEffect(() => {
  const handler = (e: Event) => {
    const detail = (e as CustomEvent<{ provider: LeaderboardProvider }>).detail;
    if (detail?.provider !== provider) return;
    manualBackfill(60);
  };
  window.addEventListener("account-first-visit-backfill", handler);
  return () => window.removeEventListener("account-first-visit-backfill", handler);
}, [provider, manualBackfill]);
```

- [ ] **Step 2: Smoke test**

Run: `npm run tauri dev`. Sign in, switch to Account. Observe network tab / Supabase logs: a batch `sync_device_snapshots` + `sync_device_model_rows` RPC fires for each included provider. Switch away and back — no redundant backfill (localStorage flag is set).

Manual reset for re-testing: `localStorage.removeItem("account-backfill:<uid>:claude")` in DevTools console.

- [ ] **Step 3: Commit**

```bash
git add src/components/Header.tsx src/hooks/useSnapshotUploader.ts
git commit -m "feat: auto-backfill 60d on first switch to Account view"
```

---

## Task 16: CI — `pricing.json` ↔ migration drift check

**Files:**
- Create: `scripts/check-pricing-sync.mjs`
- Modify: `.github/workflows/test.yml`

- [ ] **Step 1: Write the drift checker**

```javascript
// scripts/check-pricing-sync.mjs
// Exit non-zero if src-tauri/pricing.json has entries that are not mirrored
// in the latest seed migration for pricing_models. Run in CI.

import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pricingPath = join(__dirname, "..", "src-tauri", "pricing.json");
const migrationsDir = join(__dirname, "..", "supabase", "migrations");

const pricing = JSON.parse(readFileSync(pricingPath, "utf8"));

// Find the latest seed_pricing_models migration.
const migrations = readdirSync(migrationsDir)
  .filter((f) => /_seed_pricing_models\.sql$/.test(f))
  .sort();
if (migrations.length === 0) {
  console.error("No seed_pricing_models migration found.");
  process.exit(1);
}
const latest = readFileSync(join(migrationsDir, migrations[migrations.length - 1]), "utf8");

const missing = [];
for (const provider of ["claude", "codex", "opencode", "kimi", "glm"]) {
  const cfg = pricing[provider];
  if (!cfg?.models) continue;
  for (const m of cfg.models) {
    const needle = `'${provider}', '${m.match.replace(/'/g, "''")}'`;
    if (!latest.includes(needle)) {
      missing.push(`${provider}:${m.match}`);
    }
  }
}

if (missing.length > 0) {
  console.error("Pricing drift — the following (provider, match) pairs are in pricing.json but not in the latest seed migration:");
  for (const m of missing) console.error("  - " + m);
  console.error("\nFix: `node scripts/seed-pricing-models.mjs > supabase/migrations/<YYYYMMDDHHMMSS>_seed_pricing_models.sql` and commit.");
  process.exit(1);
}
console.log(`Pricing in sync (${missing.length === 0 ? "OK" : "DRIFT"}; ${pricing.version}).`);
```

- [ ] **Step 2: Verify it passes with the current seed**

Run: `node scripts/check-pricing-sync.mjs`
Expected: `Pricing in sync (OK; 1.2.0).`, exit 0.

- [ ] **Step 3: Verify it fails when drift is introduced**

Temporarily add a fake model to `pricing.json`:
```bash
node -e 'const fs=require("fs");const p=JSON.parse(fs.readFileSync("src-tauri/pricing.json","utf8"));p.claude.models.unshift({match:"fake-drift-test",label:"x",input:1,output:1});fs.writeFileSync("src-tauri/pricing.json",JSON.stringify(p,null,2));'
node scripts/check-pricing-sync.mjs
```
Expected: non-zero exit, "missing claude:fake-drift-test". Then: `git checkout src-tauri/pricing.json` to revert.

- [ ] **Step 4: Add to CI workflow**

Edit `.github/workflows/test.yml` — after the "TypeScript check" step, add:

```yaml
      - name: Pricing sync check
        run: node scripts/check-pricing-sync.mjs
```

- [ ] **Step 5: Commit**

```bash
git add scripts/check-pricing-sync.mjs .github/workflows/test.yml
git commit -m "ci: guard against pricing.json vs pricing_models migration drift"
```

---

## Task 17: End-to-end smoke test & PR

**Files:** (read-only verification + PR creation)

- [ ] **Step 1: Run the full local verification suite**

```bash
# Frontend types
npx tsc --noEmit

# Frontend build
npm run build

# Rust tests (includes Task 7's new test)
cargo test --manifest-path src-tauri/Cargo.toml

# Supabase migrations apply cleanly on reset
supabase db reset

# SQL test
psql postgresql://postgres:postgres@127.0.0.1:54322/postgres -f supabase/tests/account_stats_rpcs.sql

# Pricing drift check
node scripts/check-pricing-sync.mjs
```
Expected: each command exits 0 and prints success output.

- [ ] **Step 2: Manual end-to-end smoke on local DB**

```bash
npm run tauri dev
```
With a local Supabase target:
1. Sign in with GitHub OAuth.
2. Let the app run ~1 minute — observe the `sync_device_snapshots` and `sync_device_model_rows` calls fire.
3. Switch the header toggle to Account — panels populate within ~2s (RPC round-trip).
4. Switch back to Local — local panels restore; `ProjectBreakdown` etc. show real data again.
5. Verify in SQL: `select count(*), sum(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens) from daily_model_snapshots where user_id = auth.uid();` matches local `AllStats` lifetime totals (within the 60-day backfill window).

- [ ] **Step 3: Push and open PR**

```bash
git push -u origin feat/account-level-stats
gh pr create --title "feat: account-level stats aggregation across devices" --body "$(cat <<'EOF'
## Summary
- New `daily_model_snapshots` table + `pricing_models` seed table on Supabase
- Server-side cost calc via `get_my_account_stats` RPC — retroactive pricing fixes apply to history
- Client uploads per-model rows alongside existing leaderboard snapshot; 60-day backfill on first Account toggle
- Frontend `Local / Account` toggle in the header wires through a new `useStatsSource` dispatcher
- Panels that depend on local-only data (project breakdown, tool usage, shell commands, activity graph, salary) show a disabled overlay in Account mode
- CI guard against `pricing.json` drifting from the seed migration

Spec: `docs/superpowers/specs/2026-04-20-account-level-stats-design.md`
Plan: `docs/superpowers/plans/2026-04-20-account-level-stats.md`

## Test plan
- [x] `cargo test` — Rust unit tests including new `build_stats_populates_daily_model_usage`
- [x] `psql -f supabase/tests/account_stats_rpcs.sql` — cross-device SUM + cost correctness
- [x] `npx tsc --noEmit` and `npm run build` — types + frontend build
- [x] `node scripts/check-pricing-sync.mjs` — pricing seed drift guard
- [x] Manual smoke: sign-in, Account toggle populates; Local restores; disabled panels greyed correctly
EOF
)"
```

- [ ] **Step 4: Capture the PR URL**

Record the URL returned by `gh pr create` for reference.

---

## Self-Review

**1. Spec coverage:**
- ✅ `daily_model_snapshots` schema — Task 1
- ✅ `pricing_models` schema + seed — Tasks 2, 3
- ✅ `sync_device_model_rows` RPC — Task 4
- ✅ `get_my_account_stats` RPC with pricing window — Task 4
- ✅ Rust `DailyModelUsage` + aggregation — Tasks 6, 7
- ✅ TS mirror types + prefs — Tasks 8, 9
- ✅ Upload integration (auto + backfill) — Task 10
- ✅ `useAccountStats` — Task 11
- ✅ `statsSource` dispatcher + context — Task 12
- ✅ Header toggle — Task 13
- ✅ Disabled panels in Account mode — Task 14
- ✅ First-visit auto-backfill — Task 15
- ✅ CI pricing drift guard (spec's named "open risk" mitigation) — Task 16
- ✅ Integration verification + PR — Task 17

**2. Placeholder scan:** No TBDs, no "similar to Task N", all code snippets present. Task 12 (`SettingsContext`) references "the existing pattern" — acceptable because that file's shape varies and the task explicitly tells the engineer to grep and mirror it.

**3. Type consistency:** `DailyModelUsage` shape matches across Rust (Task 6), TS types (Task 8), upload payload `ModelRowPayload` (Task 10 — note the key-name contraction `input`/`output` vs `input_tokens`/`output_tokens` is deliberate to keep the JSON payload small and matches the `sync_device_model_rows` RPC parameters from Task 4). `AccountStatsRow` in Task 11 matches `get_my_account_stats` RETURNS TABLE from Task 4.
