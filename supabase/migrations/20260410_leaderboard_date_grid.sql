-- Per-date leaderboard grid: returns top N users for each date in a range.
-- Used by the "Grid" view in the frontend leaderboard to display rank evolution.
--
-- Filter: exclude profiles with leaderboard_hidden = true (admin manual hide).
-- Mirrors get_leaderboard_entries().

create or replace function get_leaderboard_date_grid(
  p_provider text,
  p_date_from date,
  p_date_to date,
  p_top_n integer default 10
) returns table (
  date date,
  rank integer,
  user_id uuid,
  nickname text,
  avatar_url text,
  total_tokens bigint
)
language sql
security definer
set search_path = public
as $$
  with per_date as (
    select
      s.date,
      s.user_id,
      sum(s.total_tokens)::bigint as total_tokens
    from daily_snapshots s
    join profiles p on p.id = s.user_id
    where s.provider = p_provider
      and s.date >= p_date_from
      and s.date <= p_date_to
      and p.leaderboard_hidden = false
    group by s.date, s.user_id
  ),
  ranked as (
    select
      d.date,
      d.user_id,
      d.total_tokens,
      row_number() over (
        partition by d.date
        order by d.total_tokens desc, d.user_id
      )::integer as rank
    from per_date d
  )
  select
    r.date,
    r.rank,
    r.user_id,
    p.nickname,
    p.avatar_url,
    r.total_tokens
  from ranked r
  join profiles p on p.id = r.user_id
  where r.rank <= p_top_n
  order by r.date desc, r.rank asc;
$$;

revoke all on function get_leaderboard_date_grid(text, date, date, integer) from public;
grant execute on function get_leaderboard_date_grid(text, date, date, integer) to authenticated;
