import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AllStats } from "../lib/types";

export function useTokenStats() {
  const [stats, setStats] = useState<AllStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const hasDataRef = useRef(false);

  const fetchStats = useCallback(async () => {
    try {
      const data = await invoke<AllStats>("get_all_stats");
      setStats(data);
      setError(null);
      hasDataRef.current = true;
    } catch (e) {
      // Only show error if we never had valid data — keeps last known data on transient failures
      if (!hasDataRef.current) {
        setError(String(e));
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchStats();

    // Listen for file watcher events
    const unlisten = listen("stats-updated", () => {
      fetchStats();
    });

    // Fallback polling every 60s
    const interval = setInterval(fetchStats, 60_000);

    return () => {
      unlisten.then((fn) => fn());
      clearInterval(interval);
    };
  }, [fetchStats]);

  return { stats, error, loading, refetch: fetchStats };
}
