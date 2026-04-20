import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { supabase } from "../lib/supabase";
import type { AllStats, LeaderboardProvider } from "../lib/types";
import { getTotalTokens, toLocalDateStr } from "../lib/format";
import type { User } from "@supabase/supabase-js";

interface UseSnapshotUploaderProps {
  stats: AllStats | null;
  user: User | null;
  /** Opt-in to public leaderboard: gates the thin `sync_device_snapshots` payload. */
  optedIn: boolean;
  /**
   * Opt-in to cross-device account view: gates the per-model `sync_device_model_rows`
   * payload. Decoupled from leaderboard so a user can see their own aggregated stats
   * without appearing on the public board.
   */
  accountSyncEnabled: boolean;
  provider: LeaderboardProvider;
}

/**
 * Custom event dispatched after a successful snapshot upload. Leaderboard
 * display hooks listen for this to optimistic-patch the current user's row
 * with the new numbers in `detail` instead of forcing a full refetch.
 */
export const SNAPSHOT_UPLOADED_EVENT = "leaderboard-snapshot-uploaded";

export interface SnapshotUploadedDetail {
  provider: LeaderboardProvider;
  today: string;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

// 15 min auto-upload floor. File watcher fires on every Claude/Codex write,
// which without this gate caused ~37 RPC/min cluster-wide (PR #117).
const MIN_AUTO_UPLOAD_INTERVAL_MS = 15 * 60 * 1000;
const BACKFILL_DAYS = 60;

// Shared caches so multiple uploader instances (e.g. one per provider)
// don't re-derive device IDs.
const stableDeviceIdCache = new Map<string, string>();

interface UploadState {
  lastUploadAt: number;
  lastTodayPayload: string | null;
  lastCleanupAt: number;
}

const uploadStateByKey = new Map<string, UploadState>();

function getUploadState(key: string): UploadState {
  let state = uploadStateByKey.get(key);
  if (!state) {
    state = { lastUploadAt: 0, lastTodayPayload: null, lastCleanupAt: 0 };
    uploadStateByKey.set(key, state);
  }
  return state;
}

interface RowPayload {
  date: string;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

interface ModelRowPayload {
  date: string;
  model: string;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
}

function buildModelRowsForDate(stats: AllStats, date: string): ModelRowPayload[] {
  const src = stats.daily_model_usage ?? [];
  return src
    .filter((d) => d.date === date)
    .map((d) => ({
      date: d.date,
      model: d.model,
      input: d.input_tokens,
      output: d.output_tokens,
      cache_read: d.cache_read_tokens,
      cache_write: d.cache_write_tokens,
    }));
}

function buildModelRowsInRange(stats: AllStats, startStr: string, today: string): ModelRowPayload[] {
  const src = stats.daily_model_usage ?? [];
  return src
    .filter((d) => d.date >= startStr && d.date <= today)
    .map((d) => ({
      date: d.date,
      model: d.model,
      input: d.input_tokens,
      output: d.output_tokens,
      cache_read: d.cache_read_tokens,
      cache_write: d.cache_write_tokens,
    }));
}

async function callSyncModelRpc(
  provider: LeaderboardProvider,
  deviceId: string,
  rows: ModelRowPayload[],
): Promise<boolean> {
  if (!supabase || rows.length === 0) return true; // nothing to do ≠ failure
  const { error } = await supabase.rpc("sync_device_model_rows", {
    p_provider: provider,
    p_device_id: deviceId,
    p_rows: rows,
  });
  if (error) console.warn("[snapshot] sync_device_model_rows failed", error.message);
  return !error;
}

function buildTodayRow(stats: AllStats, today: string): RowPayload | null {
  const todayEntry = stats.daily.find((d) => d.date === today);
  if (!todayEntry) return null;
  return {
    date: today,
    total_tokens: getTotalTokens(todayEntry.tokens),
    cost_usd: todayEntry.cost_usd,
    messages: todayEntry.messages,
    sessions: todayEntry.sessions,
  };
}

function payloadFingerprint(row: RowPayload): string {
  return `${row.total_tokens}|${row.cost_usd}|${row.messages}|${row.sessions}`;
}

function buildStaleDates(stats: AllStats, today: string): string[] {
  const start = new Date();
  start.setDate(start.getDate() - (BACKFILL_DAYS - 1));
  const startStr = toLocalDateStr(start);
  const local = new Set(
    stats.daily.filter((d) => d.date >= startStr && d.date <= today).map((d) => d.date),
  );
  const all: string[] = [];
  const cursor = new Date(startStr);
  while (toLocalDateStr(cursor) <= today) {
    const ds = toLocalDateStr(cursor);
    if (!local.has(ds)) all.push(ds);
    cursor.setDate(cursor.getDate() + 1);
  }
  return all;
}

async function callSyncRpc(
  provider: LeaderboardProvider,
  deviceId: string,
  rows: RowPayload[],
  staleDates: string[],
): Promise<boolean> {
  if (!supabase) return false;
  if (rows.length === 0 && staleDates.length === 0) return false;
  const { error } = await supabase.rpc("sync_device_snapshots", {
    p_provider: provider,
    p_device_id: deviceId,
    p_rows: rows,
    p_stale_dates: staleDates,
  });
  return !error;
}

function dispatchUploaded(provider: LeaderboardProvider, todayRow: RowPayload | null) {
  if (!todayRow) return;
  window.dispatchEvent(
    new CustomEvent<SnapshotUploadedDetail>(SNAPSHOT_UPLOADED_EVENT, {
      detail: {
        provider,
        today: todayRow.date,
        total_tokens: todayRow.total_tokens,
        cost_usd: todayRow.cost_usd,
        messages: todayRow.messages,
        sessions: todayRow.sessions,
      },
    }),
  );
}

/**
 * Uploads the user's *today* snapshot for a given provider to Supabase on a
 * throttled, change-only basis. Past-day backfill is no longer automatic — it
 * runs once per (user, provider) on first leaderboard entry, plus on demand
 * via the `manualBackfill` returned function.
 *
 * Why: Supabase Free/Nano hit the IO Budget because the previous policy
 * uploaded today + 60 days of history every time stats changed (~240 DB ops
 * per call). Restricting auto uploads to today only, with a 15-minute floor
 * and value-change skip, drops per-call ops to ~2 and call frequency by orders
 * of magnitude.
 */
export function useSnapshotUploader({ stats, user, optedIn, accountSyncEnabled, provider }: UseSnapshotUploaderProps) {
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const throttleRetryRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const statsRef = useRef<AllStats | null>(null);
  statsRef.current = stats;

  // Resolve stable device id once per user
  useEffect(() => {
    let cancelled = false;

    if (!user) {
      setDeviceId(null);
      return () => { cancelled = true; };
    }

    const cached = stableDeviceIdCache.get(user.id);
    if (cached) {
      setDeviceId(cached);
      return () => { cancelled = true; };
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

    return () => { cancelled = true; };
  }, [user?.id]);

  const stateKey = user && deviceId ? `${user.id}:${provider}:${deviceId}` : null;

  // Auto upload: today only, 15min throttle, skipped if values unchanged.
  // Activates whenever EITHER opt-in is on; each RPC fires conditionally inside.
  // If a stats change lands inside the throttle window, we defer it via
  // `throttleRetryRef` so the very last observed value still reaches Supabase.
  const uploadsEnabled = optedIn || accountSyncEnabled;
  useEffect(() => {
    if (!supabase || !user || !uploadsEnabled || !stats || !deviceId || !stateKey) return;

    const attempt = async () => {
      throttleRetryRef.current = undefined;
      const liveStats = statsRef.current;
      if (!liveStats) return;

      const today = toLocalDateStr(new Date());
      const todayRow = buildTodayRow(liveStats, today);
      if (!todayRow) return;

      const state = getUploadState(stateKey);
      const fingerprint = payloadFingerprint(todayRow);
      const now = Date.now();

      if (state.lastTodayPayload === fingerprint) return;

      const sinceLast = now - state.lastUploadAt;
      if (sinceLast < MIN_AUTO_UPLOAD_INTERVAL_MS) {
        const wait = MIN_AUTO_UPLOAD_INTERVAL_MS - sinceLast;
        if (throttleRetryRef.current) clearTimeout(throttleRetryRef.current);
        throttleRetryRef.current = setTimeout(attempt, wait);
        return;
      }

      // Leaderboard thin payload — only if user opted into public leaderboard.
      // Stale-date cleanup is intentionally skipped on the auto path: a 60-day
      // scan every 24h per (user × provider) was dominating IO on Nano; the
      // 30-day cutoff inside sync_device_snapshots prunes stale device rows,
      // and manualBackfill still runs buildStaleDates on explicit resync.
      if (optedIn) {
        const ok = await callSyncRpc(provider, deviceId, [todayRow], []);
        if (ok) {
          state.lastUploadAt = now;
          state.lastTodayPayload = fingerprint;
          dispatchUploaded(provider, todayRow);
        }
      }

      // Per-model rows for the Account view — only if user opted into account sync.
      if (accountSyncEnabled) {
        const modelRows = buildModelRowsForDate(liveStats, today);
        if (modelRows.length > 0) {
          const modelOk = await callSyncModelRpc(provider, deviceId, modelRows);
          // If leaderboard was skipped (optedIn=false) but model rows went
          // through, still advance the throttle state so we don't retry in
          // a tight loop.
          if (modelOk && !optedIn) {
            state.lastUploadAt = now;
            state.lastTodayPayload = fingerprint;
          }
        }
      }
    };

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(attempt, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [stats, user, optedIn, accountSyncEnabled, uploadsEnabled, provider, deviceId, stateKey]);

  // Clear any pending throttle retry when the uploader unmounts or loses its key
  useEffect(() => {
    return () => {
      if (throttleRetryRef.current) clearTimeout(throttleRetryRef.current);
    };
  }, []);

  // Explicit backfill entry point. Called by:
  //   1. Leaderboard first-visit auto-trigger (one-time, gated by localStorage flag)
  //   2. Manual "Upload my past data" button
  //   3. First flip to Account view in Header (separate localStorage flag)
  //
  // Gates each RPC independently — if only accountSyncEnabled is on, the
  // thin leaderboard payload is skipped but model rows still upload. Returns
  // true only if EVERY attempted RPC succeeded; a partial failure returns
  // false so the caller can surface it (fixes P1 #7 from code review).
  const manualBackfill = useCallback(
    async (days: number = BACKFILL_DAYS): Promise<boolean> => {
      if (!supabase || !user || !stats || !deviceId || !stateKey) return false;
      if (!optedIn && !accountSyncEnabled) return false;
      const today = toLocalDateStr(new Date());
      const start = new Date();
      start.setDate(start.getDate() - (days - 1));
      const startStr = toLocalDateStr(start);

      const rows: RowPayload[] = stats.daily
        .filter((d) => d.date >= startStr && d.date <= today)
        .map((d) => ({
          date: d.date,
          total_tokens: getTotalTokens(d.tokens),
          cost_usd: d.cost_usd,
          messages: d.messages,
          sessions: d.sessions,
        }));

      let leaderboardOk = true;
      if (optedIn) {
        const staleDates = buildStaleDates(stats, today);
        leaderboardOk = await callSyncRpc(provider, deviceId, rows, staleDates);
      }

      let modelOk = true;
      if (accountSyncEnabled) {
        const modelRows = buildModelRowsInRange(stats, startStr, today);
        if (modelRows.length > 0) {
          modelOk = await callSyncModelRpc(provider, deviceId, modelRows);
        }
      }

      const allOk = leaderboardOk && modelOk;
      if (allOk) {
        const state = getUploadState(stateKey);
        state.lastUploadAt = Date.now();
        state.lastCleanupAt = Date.now();
        const todayRow = rows.find((r) => r.date === today) ?? null;
        if (todayRow) state.lastTodayPayload = payloadFingerprint(todayRow);
        if (optedIn) dispatchUploaded(provider, todayRow);
      }
      return allOk;
    },
    [user, optedIn, accountSyncEnabled, stats, deviceId, provider, stateKey],
  );

  return { manualBackfill, deviceId, ready: !!stateKey };
}
