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
