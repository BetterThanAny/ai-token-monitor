import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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

interface UseLeaderboardSyncProps {
  stats: AllStats | null;
  user: User | null;
  optedIn: boolean;
  provider: LeaderboardProvider;
  period: LeaderboardPeriod;
  /**
   * Called after a successful snapshot upload. Used by the grid view to
   * refresh its own data, since `fetchLeaderboard` is a no-op in grid mode.
   */
  onAfterUpload?: () => void;
}

const LEADERBOARD_CACHE_TTL = 180_000; // 3 minutes
const LEADERBOARD_POLL_INTERVAL = 180_000; // 3 minutes
const stableDeviceIdCache = new Map<string, string>();

export type LeaderboardPeriod = "today" | "week" | "month" | "grid";

export function useLeaderboardSync({ stats, user, optedIn, provider, period, onAfterUpload }: UseLeaderboardSyncProps) {
  const [leaderboard, setLeaderboard] = useState<LeaderboardEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  // Keep a stable reference to onAfterUpload so the upload effect doesn't
  // re-run (and restart its debounce) every render when the caller passes
  // a non-memoized callback.
  const onAfterUploadRef = useRef(onAfterUpload);
  useEffect(() => { onAfterUploadRef.current = onAfterUpload; }, [onAfterUpload]);
  // Track which past days (before today) have already been synced this session, per provider
  const syncedPastDatesRef = useRef<Set<string>>(new Set());
  const cacheRef = useRef<{
    data: LeaderboardEntry[];
    fetchedAt: number;
    period: LeaderboardPeriod;
    provider: LeaderboardProvider;
  } | null>(null);

  // Fetch leaderboard data
  const fetchLeaderboard = useCallback(async (forceRefresh = false) => {
    if (!supabase) return;
    // Grid view uses a separate hook (useLeaderboardGrid); skip list fetch.
    if (period === "grid") return;

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
      let dateFrom: string;

      if (period === "today") {
        dateFrom = today;
      } else if (period === "week") {
        const now = new Date();
        const dow = now.getDay();
        const mondayOffset = dow === 0 ? 6 : dow - 1;
        const monday = new Date(now);
        monday.setDate(now.getDate() - mondayOffset);
        dateFrom = toLocalDateStr(monday);
      } else {
        const now = new Date();
        const firstOfMonth = new Date(now.getFullYear(), now.getMonth(), 1);
        dateFrom = toLocalDateStr(firstOfMonth);
      }

      const { data } = await supabase.rpc("get_leaderboard_entries", {
        p_provider: provider,
        p_date_from: dateFrom,
        p_date_to: today,
      });

      if (data) {
        const entries = (data as LeaderboardEntry[]).map((e) => ({
          ...e,
          cost_usd: Number(e.cost_usd),
        }));
        setLeaderboard(entries);
        cacheRef.current = { data: entries, fetchedAt: Date.now(), period, provider };
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
      // Always invalidate the list cache so the "other" tab reads fresh data
      // when the user eventually switches to it. fetchLeaderboard(true) is a
      // no-op in grid mode due to its early return, so without this line a
      // grid→list switch within the 3-minute TTL would show stale rankings.
      cacheRef.current = null;
      // List view: refresh cached leaderboard immediately (no-op in grid mode).
      fetchLeaderboard(true);
      // Grid view: the caller's refetch also invalidates its cache, so a
      // list→grid switch within TTL reads fresh data too.
      onAfterUploadRef.current?.();
    }, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [stats, user, optedIn, provider, deviceId, fetchLeaderboard]);

  // Auto-refresh with visibility-aware polling
  useEffect(() => {
    // Grid view is served by useLeaderboardGrid; skip list polling entirely.
    if (period === "grid") return;

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
  }, [period, fetchLeaderboard]);

  const dateRange = useMemo(() => {
    const now = new Date();
    const today = toLocalDateStr(now);
    if (period === "today") return { from: today, to: today };
    if (period === "week") {
      const dow = now.getDay();
      const mondayOffset = dow === 0 ? 6 : dow - 1;
      const monday = new Date(now);
      monday.setDate(now.getDate() - mondayOffset);
      return { from: toLocalDateStr(monday), to: today };
    }
    if (period === "grid") {
      const weekAgo = new Date(now);
      weekAgo.setDate(now.getDate() - 6);
      return { from: toLocalDateStr(weekAgo), to: today };
    }
    const firstOfMonth = new Date(now.getFullYear(), now.getMonth(), 1);
    return { from: toLocalDateStr(firstOfMonth), to: today };
  }, [period]);

  return { leaderboard, loading, dateRange, refetch: () => fetchLeaderboard(true) };
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
  const firstOfMonth = new Date(now.getFullYear(), now.getMonth(), 1);
  const monthStart = toLocalDateStr(firstOfMonth);
  const syncStart = monthStart < weekStart ? monthStart : weekStart;

  // Build full list of dates in the sync window (month or week, whichever is earlier)
  const allDatesInWindow: string[] = [];
  const cursor = new Date(syncStart);
  while (toLocalDateStr(cursor) <= today) {
    allDatesInWindow.push(toLocalDateStr(cursor));
    cursor.setDate(cursor.getDate() + 1);
  }

  // Dates that have local data within the sync window
  const localDatesInWindow = new Set(
    stats.daily
      .filter((d) => d.date >= syncStart && d.date <= today)
      .map((d) => d.date)
  );

  // Delete stale rows (dates in sync window with no local data)
  const cleanKey = (date: string) => `${provider}:clean:${date}`;
  const staleDates = allDatesInWindow.filter((d) => !localDatesInWindow.has(d));
  const toClean = staleDates.filter((d) => !syncedPastDates.has(cleanKey(d)));

  if (localDatesInWindow.has(today)) {
    syncedPastDates.delete(cleanKey(today));
  }

  // Always upload today; upload past days only once per session
  const syncKey = (date: string) => `${provider}:${date}`;
  const toSync = stats.daily.filter(
    (d) => d.date >= syncStart && d.date <= today &&
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
