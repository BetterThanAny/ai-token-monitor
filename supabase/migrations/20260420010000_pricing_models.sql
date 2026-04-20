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
