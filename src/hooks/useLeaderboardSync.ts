import { useEffect, useRef, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import type { AllStats } from "../lib/types";
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
}

const LEADERBOARD_CACHE_TTL = 60_000; // 60 seconds

export function useLeaderboardSync({ stats, user, optedIn }: UseLeaderboardSyncProps) {
  const [leaderboard, setLeaderboard] = useState<LeaderboardEntry[]>([]);
  const [period, setPeriod] = useState<"today" | "week">("today");
  const [loading, setLoading] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const cacheRef = useRef<{
    data: LeaderboardEntry[];
    fetchedAt: number;
    period: "today" | "week";
  } | null>(null);

  // Upload today's snapshot (debounced)
  useEffect(() => {
    if (!supabase || !user || !optedIn || !stats) return;

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      uploadSnapshot(user.id, stats);
    }, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [stats, user, optedIn]);

  // Fetch leaderboard data
  const fetchLeaderboard = useCallback(async (forceRefresh = false) => {
    if (!supabase) return;

    // Return cached data if still fresh and period matches
    if (
      !forceRefresh &&
      cacheRef.current &&
      cacheRef.current.period === period &&
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
          .order("total_tokens", { ascending: false })
          .limit(100);

        if (data) {
          const entries = data.map((row) => {
            const profile = Array.isArray(row.profiles) ? row.profiles[0] : row.profiles;
            return {
              user_id: row.user_id,
              nickname: (profile as { nickname?: string } | null)?.nickname ?? "Unknown",
              avatar_url: (profile as { avatar_url?: string | null } | null)?.avatar_url ?? null,
              total_tokens: row.total_tokens,
              cost_usd: Number(row.cost_usd),
              messages: row.messages,
              sessions: row.sessions,
            };
          });
          setLeaderboard(entries);
          cacheRef.current = { data: entries, fetchedAt: Date.now(), period };
        }
      } else {
        // Weekly: aggregate snapshots from monday to today
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
          .limit(5000);

        if (data) {
          const userMap = new Map<string, LeaderboardEntry>();
          for (const row of data) {
            const profile = Array.isArray(row.profiles) ? row.profiles[0] : row.profiles;
            const existing = userMap.get(row.user_id);
            if (existing) {
              existing.total_tokens += row.total_tokens;
              existing.cost_usd += Number(row.cost_usd);
              existing.messages += row.messages;
              existing.sessions += row.sessions;
            } else {
              userMap.set(row.user_id, {
                user_id: row.user_id,
                nickname: (profile as { nickname?: string } | null)?.nickname ?? "Unknown",
                avatar_url: (profile as { avatar_url?: string | null } | null)?.avatar_url ?? null,
                total_tokens: row.total_tokens,
                cost_usd: Number(row.cost_usd),
                messages: row.messages,
                sessions: row.sessions,
              });
            }
          }
          const sorted = Array.from(userMap.values()).sort((a, b) => b.total_tokens - a.total_tokens);
          setLeaderboard(sorted);
          cacheRef.current = { data: sorted, fetchedAt: Date.now(), period };
        }
      }
    } finally {
      setLoading(false);
    }
  }, [period]);

  // Auto-refresh every 60s (force refresh to bypass cache on interval)
  useEffect(() => {
    fetchLeaderboard();
    const interval = setInterval(() => fetchLeaderboard(true), 60_000);
    return () => clearInterval(interval);
  }, [fetchLeaderboard]);

  return { leaderboard, loading, period, setPeriod, refetch: () => fetchLeaderboard(true) };
}

async function uploadSnapshot(userId: string, stats: AllStats) {
  if (!supabase) return;

  const today = toLocalDateStr(new Date());
  const todayData = stats.daily.find((d) => d.date === today);
  if (!todayData) return;

  const totalTokens = getTotalTokens(todayData.tokens);

  await supabase.from("daily_snapshots").upsert({
    user_id: userId,
    date: today,
    total_tokens: totalTokens,
    cost_usd: todayData.cost_usd,
    messages: todayData.messages,
    sessions: todayData.sessions,
  }, { onConflict: "user_id,date" });
}
