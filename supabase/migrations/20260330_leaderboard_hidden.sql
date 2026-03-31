-- 1. profiles에 leaderboard_hidden 컬럼 추가 (관리자 수동 숨김용)
alter table profiles
add column if not exists leaderboard_hidden boolean not null default false;

-- 2. 사용자가 자기 자신의 hidden 상태를 변경하지 못하도록 RLS 정책 업데이트
drop policy if exists "profiles_update" on profiles;
create policy "profiles_update" on profiles
  for update using (auth.uid() = id)
  with check (
    auth.uid() = id
    and leaderboard_hidden = (select leaderboard_hidden from profiles where id = auth.uid())
  );

-- 3. get_leaderboard_entries() 함수에 이상치 자동 필터링 추가
--    - leaderboard_hidden = true인 사용자 제외 (관리자 수동 숨김)
--    - 토큰/메시지 비율 > 3000인 사용자 자동 제외 (캐시 토큰 뻥튀기 감지)
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
  having sum(s.messages) <= 0
    or sum(s.total_tokens)::float / greatest(sum(s.messages), 1) <= 3000
  order by sum(s.total_tokens) desc, s.user_id
  limit 200;
$$;
