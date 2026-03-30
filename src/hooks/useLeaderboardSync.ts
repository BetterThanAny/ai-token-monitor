import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { supabase } from "../lib/supabase";
import type { AllStats, LeaderboardProvider } from "../lib/types";
import { getTotalTokens, toLocalDateStr } from "../lib/format";
import type { User } from "@supabase/supabase-js";

export interface LeaderboardEntry {
  user_id: string;
  nickname: string;
  avatar_url: string | null;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

interface SnapshotRow {
  user_id: string;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
  profiles: { nickname: string; avatar_url: string | null }
    | { nickname: string; avatar_url: string | null }[];
}

function toLeaderboardEntry(row: SnapshotRow): LeaderboardEntry {
  const profile = Array.isArray(row.profiles) ? row.profiles[0] : row.profiles;
  return {
    user_id: row.user_id,
    nickname: profile?.nickname ?? "Unknown",
    avatar_url: profile?.avatar_url ?? null,
    total_tokens: row.total_tokens,
    cost_usd: Number(row.cost_usd),
    messages: row.messages,
    sessions: row.sessions,
  };
}

interface UseLeaderboardSyncProps {
  stats: AllStats | null;
  user: User | null;
  optedIn: boolean;
  provider: LeaderboardProvider;
}

const LEADERBOARD_CACHE_TTL = 180_000; // 3 minutes
const LEADERBOARD_POLL_INTERVAL = 180_000; // 3 minutes
const stableDeviceIdCache = new Map<string, string>();

export function useLeaderboardSync({ stats, user, optedIn, provider }: UseLeaderboardSyncProps) {
  const [leaderboard, setLeaderboard] = useState<LeaderboardEntry[]>([]);
  const [period, setPeriod] = useState<"today" | "week">("today");
  const [loading, setLoading] = useState(false);
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  // Track which past days (before today) have already been synced this session, per provider
  const syncedPastDatesRef = useRef<Set<string>>(new Set());
  const cacheRef = useRef<{
    data: LeaderboardEntry[];
    fetchedAt: number;
    period: "today" | "week";
    provider: LeaderboardProvider;
  } | null>(null);

  // Fetch leaderboard data
  const fetchLeaderboard = useCallback(async (forceRefresh = false) => {
    if (!supabase) return;

    // Return cached data if still fresh and period+provider match
    if (
      !forceRefresh &&
      cacheRef.current &&
      cacheRef.current.period === period &&
      cacheRef.current.provider === provider &&
      Date.now() - cacheRef.current.fetchedAt < LEADERBOARD_CACHE_TTL
    ) {
      setLeaderboard(cacheRef.current.data);
      return;
    }

    setLoading(true);

    try {
      const today = toLocalDateStr(new Date());

      if (period === "today") {
        const { data } = await supabase
          .from("daily_snapshots")
          .select("user_id, total_tokens, cost_usd, messages, sessions, profiles(nickname, avatar_url)")
          .eq("date", today)
          .eq("provider", provider)
          .order("total_tokens", { ascending: false })
          .limit(100);

        if (data) {
          const entries = (data as SnapshotRow[]).map(toLeaderboardEntry);
          setLeaderboard(entries);
          cacheRef.current = { data: entries, fetchedAt: Date.now(), period, provider };
        }
      } else {
        const now = new Date();
        const dow = now.getDay();
        const mondayOffset = dow === 0 ? 6 : dow - 1;
        const monday = new Date(now);
        monday.setDate(now.getDate() - mondayOffset);
        const weekStart = toLocalDateStr(monday);

        const { data } = await supabase
          .from("daily_snapshots")
          .select("user_id, total_tokens, cost_usd, messages, sessions, profiles(nickname, avatar_url)")
          .gte("date", weekStart)
          .lte("date", today)
          .eq("provider", provider)
          .limit(5000);

        if (data) {
          const userMap = new Map<string, LeaderboardEntry>();
          for (const row of data as SnapshotRow[]) {
            const existing = userMap.get(row.user_id);
            if (existing) {
              existing.total_tokens += row.total_tokens;
              existing.cost_usd += Number(row.cost_usd);
              existing.messages += row.messages;
              existing.sessions += row.sessions;
            } else {
              userMap.set(row.user_id, toLeaderboardEntry(row));
            }
          }
          const sorted = Array.from(userMap.values()).sort((a, b) => b.total_tokens - a.total_tokens);
          setLeaderboard(sorted);
          cacheRef.current = { data: sorted, fetchedAt: Date.now(), period, provider };
        }
      }
    } finally {
      setLoading(false);
    }
  }, [period, provider]);

  useEffect(() => {
    let cancelled = false;

    if (!user) {
      setDeviceId(null);
      return () => {
        cancelled = true;
      };
    }

    const cached = stableDeviceIdCache.get(user.id);
    if (cached) {
      setDeviceId(cached);
      return () => {
        cancelled = true;
      };
    }

    invoke<string>("get_stable_device_id", { userId: user.id })
      .then((derivedId) => {
        if (cancelled) return;
        stableDeviceIdCache.set(user.id, derivedId);
        setDeviceId(derivedId);
      })
      .catch(() => {
        if (!cancelled) setDeviceId(null);
      });

    return () => {
      cancelled = true;
    };
  }, [user?.id]);

  // Upload snapshot (debounced), then immediately refresh leaderboard
  useEffect(() => {
    if (!supabase || !user || !optedIn || !stats || !deviceId) return;

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      await uploadSnapshot(stats, provider, deviceId, syncedPastDatesRef.current);
      fetchLeaderboard(true);
    }, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [stats, user, optedIn, provider, deviceId, fetchLeaderboard]);

  // Auto-refresh with visibility-aware polling
  useEffect(() => {
    fetchLeaderboard();

    let intervalId: ReturnType<typeof setInterval> | undefined;

    const startPolling = () => {
      if (intervalId) clearInterval(intervalId);
      intervalId = setInterval(() => fetchLeaderboard(false), LEADERBOARD_POLL_INTERVAL);
    };

    const handleVisibility = () => {
      if (document.hidden) {
        if (intervalId) { clearInterval(intervalId); intervalId = undefined; }
      } else {
        fetchLeaderboard(false);
        startPolling();
      }
    };

    startPolling();
    document.addEventListener("visibilitychange", handleVisibility);

    return () => {
      if (intervalId) clearInterval(intervalId);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [fetchLeaderboard]);

  return { leaderboard, loading, period, setPeriod, refetch: () => fetchLeaderboard(true) };
}

async function uploadSnapshot(
  stats: AllStats,
  provider: LeaderboardProvider,
  deviceId: string,
  syncedPastDates: Set<string>,
) {
  if (!supabase) return;

  const now = new Date();
  const today = toLocalDateStr(now);
  const dow = now.getDay();
  const mondayOffset = dow === 0 ? 6 : dow - 1;
  const monday = new Date(now);
  monday.setDate(now.getDate() - mondayOffset);
  const weekStart = toLocalDateStr(monday);

  // Build full list of dates in the week window
  const allDatesInWeek: string[] = [];
  const cursor = new Date(monday);
  while (toLocalDateStr(cursor) <= today) {
    allDatesInWeek.push(toLocalDateStr(cursor));
    cursor.setDate(cursor.getDate() + 1);
  }

  // Dates that have local data within this week
  const localDatesInWeek = new Set(
    stats.daily
      .filter((d) => d.date >= weekStart && d.date <= today)
      .map((d) => d.date)
  );

  // Delete stale rows (dates in week window with no local data)
  const cleanKey = (date: string) => `${provider}:clean:${date}`;
  const staleDates = allDatesInWeek.filter((d) => !localDatesInWeek.has(d));
  const toClean = staleDates.filter((d) => !syncedPastDates.has(cleanKey(d)));

  if (localDatesInWeek.has(today)) {
    syncedPastDates.delete(cleanKey(today));
  }

  // Always upload today; upload past days of this week only once per session
  const syncKey = (date: string) => `${provider}:${date}`;
  const toSync = stats.daily.filter(
    (d) => d.date >= weekStart && d.date <= today &&
      (d.date === today || !syncedPastDates.has(syncKey(d.date)))
  );
  if (toSync.length === 0 && toClean.length === 0) return;

  const rows = toSync.map((d) => ({
    date: d.date,
    total_tokens: getTotalTokens(d.tokens),
    cost_usd: d.cost_usd,
    messages: d.messages,
    sessions: d.sessions,
  }));

  const { error } = await supabase.rpc("sync_device_snapshots", {
    p_provider: provider,
    p_device_id: deviceId,
    p_rows: rows,
    p_stale_dates: toClean,
  });

  if (!error) {
    toClean.forEach((d) => syncedPastDates.add(cleanKey(d)));
    toSync.forEach((d) => { if (d.date !== today) syncedPastDates.add(syncKey(d.date)); });
  }
}
