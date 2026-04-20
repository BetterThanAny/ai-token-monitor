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
