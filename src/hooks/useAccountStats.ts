import { useEffect, useRef, useState } from "react";
import type { User } from "@supabase/supabase-js";
import { supabase } from "../lib/supabase";
import { toLocalDateStr } from "../lib/format";
import type { AllStats, DailyUsage, LeaderboardProvider, ModelUsage } from "../lib/types";

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
  enabled: boolean;
  daysBack?: number;
}

const REFRESH_MS = 5 * 60 * 1000;
const DEFAULT_DAYS_BACK = 60;

export function useAccountStats({
  provider,
  user,
  enabled,
  daysBack = DEFAULT_DAYS_BACK,
}: UseAccountStatsArgs) {
  const [stats, setStats] = useState<AllStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (!enabled || !user || !supabase) {
      setStats(null);
      setLoading(false);
      setError(null);
      return;
    }

    let cancelled = false;

    const fetchOnce = async () => {
      if (!supabase) return;
      setLoading(true);
      setError(null);
      const today = new Date();
      const start = new Date();
      start.setDate(start.getDate() - (daysBack - 1));
      const { data, error: err } = await supabase.rpc("get_my_account_stats", {
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
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
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

    const day: DailyUsage = dailyMap.get(r.date) ?? {
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
    day.tokens[r.model] =
      (day.tokens[r.model] ?? 0) +
      r.input_tokens +
      r.output_tokens +
      r.cache_read_tokens +
      r.cache_write_tokens;
    day.cost_usd += Number(r.cost_usd);
    day.input_tokens += r.input_tokens;
    day.output_tokens += r.output_tokens;
    day.cache_read_tokens += r.cache_read_tokens;
    day.cache_write_tokens += r.cache_write_tokens;
    dailyMap.set(r.date, day);

    const mu: ModelUsage = modelMap[r.model] ?? {
      input_tokens: 0,
      output_tokens: 0,
      cache_read: 0,
      cache_write: 0,
      cost_usd: 0,
    };
    mu.input_tokens += r.input_tokens;
    mu.output_tokens += r.output_tokens;
    mu.cache_read += r.cache_read_tokens;
    mu.cache_write += r.cache_write_tokens;
    mu.cost_usd += Number(r.cost_usd);
    modelMap[r.model] = mu;
  }

  const daily = Array.from(dailyMap.values()).sort((a, b) =>
    a.date.localeCompare(b.date),
  );

  return {
    daily,
    model_usage: modelMap,
    total_sessions: 0,
    total_messages: 0,
    first_session_date: firstDate,
  };
}
