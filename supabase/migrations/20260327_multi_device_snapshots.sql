alter table daily_snapshots
add column if not exists device_id text;

alter table daily_snapshots
drop constraint if exists daily_snapshots_user_id_date_provider_key;

drop index if exists idx_snapshots_date_provider;

create unique index if not exists daily_snapshots_legacy_uq
on daily_snapshots (user_id, date, provider)
where device_id is null;

create unique index if not exists daily_snapshots_device_uq
on daily_snapshots (user_id, date, provider, device_id)
where device_id is not null;

create index if not exists idx_snapshots_date_provider
on daily_snapshots(date, provider);

create index if not exists idx_snapshots_provider_date_user_device
on daily_snapshots(provider, date, user_id, device_id);

drop policy if exists "snapshots_insert" on daily_snapshots;
drop policy if exists "snapshots_update" on daily_snapshots;
drop policy if exists "snapshots_delete" on daily_snapshots;

create policy "snapshots_insert" on daily_snapshots
for insert
with check (auth.uid() = user_id and device_id is null);

create policy "snapshots_update" on daily_snapshots
for update
using (auth.uid() = user_id and device_id is null)
with check (auth.uid() = user_id and device_id is null);

create policy "snapshots_delete" on daily_snapshots
for delete
using (auth.uid() = user_id and device_id is null);

create or replace function sync_device_snapshots(
  p_provider text,
  p_device_id text,
  p_rows jsonb default '[]'::jsonb,
  p_stale_dates date[] default '{}'::date[]
) returns void
language plpgsql
security definer
set search_path = public
as $$
declare
  v_user_id uuid;
  v_row jsonb;
  v_date date;
begin
  v_user_id := auth.uid();

  if v_user_id is null then
    raise exception 'Not authenticated';
  end if;

  if p_provider not in ('claude', 'codex') then
    raise exception 'Invalid provider';
  end if;

  if p_device_id is null or btrim(p_device_id) = '' then
    raise exception 'Missing device_id';
  end if;

  if p_stale_dates is not null and array_length(p_stale_dates, 1) is not null then
    delete from daily_snapshots
    where user_id = v_user_id
      and provider = p_provider
      and device_id = p_device_id
      and date = any(p_stale_dates);
  end if;

  for v_row in
    select value
    from jsonb_array_elements(coalesce(p_rows, '[]'::jsonb))
  loop
    v_date := (v_row->>'date')::date;

    delete from daily_snapshots
    where user_id = v_user_id
      and provider = p_provider
      and date = v_date
      and device_id is null;

    insert into daily_snapshots (
      user_id,
      date,
      provider,
      device_id,
      total_tokens,
      cost_usd,
      messages,
      sessions,
      submitted_at
    )
    values (
      v_user_id,
      v_date,
      p_provider,
      p_device_id,
      coalesce((v_row->>'total_tokens')::bigint, 0),
      coalesce((v_row->>'cost_usd')::numeric(10,4), 0),
      coalesce((v_row->>'messages')::integer, 0),
      coalesce((v_row->>'sessions')::integer, 0),
      now()
    )
    on conflict (user_id, date, provider, device_id)
      where device_id is not null
    do update set
      total_tokens = excluded.total_tokens,
      cost_usd = excluded.cost_usd,
      messages = excluded.messages,
      sessions = excluded.sessions,
      submitted_at = now();
  end loop;
end;
$$;

create or replace function get_leaderboard_entries(
  p_provider text,
  p_date_from date,
  p_date_to date
) returns table (
  user_id uuid,
  nickname text,
  avatar_url text,
  total_tokens bigint,
  cost_usd numeric(10,4),
  messages integer,
  sessions integer
)
language sql
security definer
set search_path = public
as $$
  with effective_daily as (
    select
      s.user_id,
      s.date,
      s.provider,
      sum(s.total_tokens)::bigint as total_tokens,
      sum(s.cost_usd)::numeric(10,4) as cost_usd,
      sum(s.messages)::integer as messages,
      sum(s.sessions)::integer as sessions
    from daily_snapshots s
    where s.provider = p_provider
      and s.date >= p_date_from
      and s.date <= p_date_to
      and (
        (
          exists (
            select 1
            from daily_snapshots d
            where d.user_id = s.user_id
              and d.date = s.date
              and d.provider = s.provider
              and d.device_id is not null
          )
          and s.device_id is not null
        )
        or
        (
          not exists (
            select 1
            from daily_snapshots d
            where d.user_id = s.user_id
              and d.date = s.date
              and d.provider = s.provider
              and d.device_id is not null
          )
          and s.device_id is null
        )
      )
    group by s.user_id, s.date, s.provider
  )
  select
    e.user_id,
    p.nickname,
    p.avatar_url,
    sum(e.total_tokens)::bigint as total_tokens,
    sum(e.cost_usd)::numeric(10,4) as cost_usd,
    sum(e.messages)::integer as messages,
    sum(e.sessions)::integer as sessions
  from effective_daily e
  join profiles p on p.id = e.user_id
  group by e.user_id, p.nickname, p.avatar_url
  order by sum(e.total_tokens) desc, e.user_id;
$$;

revoke all on function sync_device_snapshots(text, text, jsonb, date[]) from public;
grant execute on function sync_device_snapshots(text, text, jsonb, date[]) to authenticated;

revoke all on function get_leaderboard_entries(text, date, date) from public;
grant execute on function get_leaderboard_entries(text, date, date) to authenticated;
