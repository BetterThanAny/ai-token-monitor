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
--    Model IDs must match a seeded pricing_models pattern. 'opus-4-6' and
--    'sonnet-4-6' are in the seed migration; 'opus-4-7' is not (it ships on
--    a separate fork/branch), so using it here would silently fall through
--    to the $15/$75 'opus-4' entry and break the cost asserts below.
do $$
declare
  v_user constant uuid := '11111111-1111-1111-1111-111111111111';
begin
  insert into public.daily_model_snapshots (user_id, device_id, date, provider, model,
                                            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens)
  values
    (v_user, 'dev_a', '2026-04-20', 'claude', 'claude-opus-4-6-20260320',
       1000000, 500000, 10000000, 100000),
    (v_user, 'dev_b', '2026-04-20', 'claude', 'claude-opus-4-6-20260320',
       2000000, 1000000, 20000000, 200000),
    (v_user, 'dev_a', '2026-04-20', 'claude', 'claude-sonnet-4-6-20260320',
       3000000, 500000, 0, 0);
end $$;

-- 3. Run the RPC with a forced auth context. Set both the legacy
--    `request.jwt.claim.sub` and the current `request.jwt.claims` form so
--    auth.uid() resolves regardless of the Supabase version's helper shape.
select set_config('role', 'authenticated', true);
select set_config('request.jwt.claim.sub', '11111111-1111-1111-1111-111111111111', true);
select set_config('request.jwt.claims',
                  json_build_object('sub', '11111111-1111-1111-1111-111111111111')::text,
                  true);

-- 4. Assertions.
do $$
declare
  row_count int;
  opus_input bigint;
  opus_cost numeric;
  sonnet_cost numeric;
begin
  -- Total rows expected: 1 day × 2 distinct models = 2.
  select count(*) into strict row_count
    from public.get_my_account_stats('claude', '2026-04-20', '2026-04-20');
  if row_count <> 2 then
    raise exception 'Expected 2 aggregated rows, got %', row_count;
  end if;

  -- Opus 4.6 row: cross-device SUM(input)=3_000_000, SUM(output)=1_500_000,
  -- SUM(cache_read)=30_000_000, SUM(cache_write)=300_000.
  -- At $5/$25/$0.5/$6.25 per MTok → cost = 15 + 37.5 + 15 + 1.875 = 69.375.
  select r2.input_tokens, r2.cost_usd
    into strict opus_input, opus_cost
    from public.get_my_account_stats('claude', '2026-04-20', '2026-04-20') r2
   where r2.model = 'claude-opus-4-6-20260320';
  if opus_input <> 3000000 then
    raise exception 'Opus input SUM expected 3000000, got %', opus_input;
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
