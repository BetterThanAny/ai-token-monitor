import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AllStats } from "../lib/types";

export type StatsProvider = "claude" | "codex";

export function useTokenStats(provider: StatsProvider = "claude", enabled = true) {
  const [stats, setStats] = useState<AllStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(enabled);
  const hasDataRef = useRef(false);
  const requestIdRef = useRef(0);

  const fetchStats = useCallback(async () => {
    if (!enabled) {
      return;
    }

    const requestId = ++requestIdRef.current;
    try {
      const command = provider === "codex" ? "get_codex_stats" : "get_all_stats";
      const data = await invoke<AllStats>(command);
      if (requestId !== requestIdRef.current) return;
      setStats(data);
      setError(null);
      hasDataRef.current = true;
    } catch (e) {
      if (requestId !== requestIdRef.current) return;
      setError(String(e));
    } finally {
      if (requestId !== requestIdRef.current) return;
      setLoading(false);
    }
  }, [provider, enabled]);

  useEffect(() => {
    if (!enabled) {
      requestIdRef.current += 1;
      hasDataRef.current = false;
      setStats(null);
      setError(null);
      setLoading(false);
      return;
    }

    setLoading(!hasDataRef.current);
    fetchStats();

    // Listen for file watcher events
    const unlisten = listen("stats-updated", () => {
      fetchStats();
    }).catch(() => null);

    // Fallback polling every 60s
    const interval = setInterval(fetchStats, 60_000);

    return () => {
      unlisten.then((fn) => fn?.());
      clearInterval(interval);
    };
  }, [fetchStats]);

  return { stats, error, loading, refetch: fetchStats };
}
