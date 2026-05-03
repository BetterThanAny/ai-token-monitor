import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AccountState } from "../lib/types";

interface Props {
  includeClaude: boolean;
  includeCodex: boolean;
}

const ACCOUNT_STATE_POLL_INTERVAL_MS = 5 * 60_000;

export function useAccountStates({ includeClaude, includeCodex }: Props) {
  const enabled = includeClaude || includeCodex;
  const sourceSelectionKey = `${includeClaude}:${includeCodex}`;
  const [states, setStates] = useState<AccountState[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(enabled);
  const requestIdRef = useRef(0);

  const fetchStates = useCallback(async () => {
    if (!enabled) return;

    const requestId = ++requestIdRef.current;
    try {
      const data = await invoke<AccountState[]>("get_account_states");
      if (requestId !== requestIdRef.current) return;
      setStates(data);
      setError(null);
    } catch (e) {
      if (requestId !== requestIdRef.current) return;
      setError(String(e));
    } finally {
      if (requestId !== requestIdRef.current) return;
      setLoading(false);
    }
  }, [enabled, sourceSelectionKey]);

  useEffect(() => {
    if (!enabled) {
      requestIdRef.current += 1;
      setStates([]);
      setError(null);
      setLoading(false);
      return;
    }

    setLoading(true);
    fetchStates();

    const statsUnlisten = listen("stats-updated", fetchStates).catch(() => null);
    const usageUnlisten = listen("usage-updated", fetchStates).catch(() => null);
    const interval = setInterval(fetchStates, ACCOUNT_STATE_POLL_INTERVAL_MS);

    return () => {
      statsUnlisten.then((fn) => fn?.()).catch(() => {});
      usageUnlisten.then((fn) => fn?.()).catch(() => {});
      clearInterval(interval);
    };
  }, [enabled, fetchStates]);

  return { states, error, loading, refetch: fetchStates };
}
