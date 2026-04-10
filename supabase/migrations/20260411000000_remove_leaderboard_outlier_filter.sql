-- Remove the per-user outlier HAVING filter from get_leaderboard_entries().
--
-- Rationale: the tokens/messages > 3000 threshold introduced in
-- 20260330_leaderboard_hidden.sql turned out to be ineffective in practice.
-- Claude cache hits naturally inflate the ratio for normal heavy users, so
-- the filter would either hide legitimate top users or (as actually
-- happened on our remote) silently not take effect at all. Abuse cases are
-- now handled exclusively via the manual `profiles.leaderboard_hidden`
-- admin-hide switch, which this function still respects.
--
-- This is a forward migration so it reaches every environment that already
-- applied 20260330. The new date-grid RPC (20260410_leaderboard_date_grid)
-- was authored without the outlier filter from the start, so no further
-- grid-side change is required.

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
  select
    s.user_id,
    p.nickname,
    p.avatar_url,
    sum(s.total_tokens)::bigint as total_tokens,
    sum(s.cost_usd)::numeric(10,4) as cost_usd,
    sum(s.messages)::integer as messages,
    sum(s.sessions)::integer as sessions
  from daily_snapshots s
  join profiles p on p.id = s.user_id
  where s.provider = p_provider
    and s.date >= p_date_from
    and s.date <= p_date_to
    and p.leaderboard_hidden = false
  group by s.user_id, p.nickname, p.avatar_url
  order by sum(s.total_tokens) desc, s.user_id
  limit 200;
$$;
