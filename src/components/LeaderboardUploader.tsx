import { useEffect, useMemo } from "react";
import { useTokenStats } from "../hooks/useTokenStats";
import { useSnapshotUploader } from "../hooks/useSnapshotUploader";
import { useAuth } from "../hooks/useAuth";
import { useSettings } from "../contexts/SettingsContext";
import {
  registerBackfillRunner,
  type BackfillRunner,
} from "../lib/backfillRegistry";
import type { LeaderboardProvider } from "../lib/types";

/**
 * Headless component that keeps each enabled provider's *today* snapshot
 * synced with Supabase. Past-day backfill is no longer automatic; instead
 * each uploader exposes a `manualBackfill` runner via the backfill registry,
 * which the leaderboard UI calls on first visit (one-time) and via a
 * "Upload my past data" button.
 *
 * Renders nothing.
 */
export function LeaderboardUploader() {
  const { user } = useAuth();
  const { prefs } = useSettings();
  const optedIn = !!prefs.leaderboard_opted_in;
  const accountSyncEnabled = !!prefs.account_sync_enabled;

  // Stats hooks always fire (rules of hooks); uploads are gated inside
  // useSnapshotUploader by the opt-in flags and each provider's include flag.
  const { stats: claudeStats } = useTokenStats("claude");
  const { stats: codexStats } = useTokenStats("codex");
  const { stats: opencodeStats } = useTokenStats("opencode");
  const { stats: kimiStats } = useTokenStats("kimi");
  const { stats: glmStats } = useTokenStats("glm");

  const claude = useSnapshotUploader({
    stats: prefs.include_claude ? claudeStats : null,
    user,
    optedIn,
    accountSyncEnabled,
    provider: "claude",
  });
  const codex = useSnapshotUploader({
    stats: prefs.include_codex ? codexStats : null,
    user,
    optedIn,
    accountSyncEnabled,
    provider: "codex",
  });
  const opencode = useSnapshotUploader({
    stats: prefs.include_opencode ? opencodeStats : null,
    user,
    optedIn,
    accountSyncEnabled,
    provider: "opencode",
  });
  const kimi = useSnapshotUploader({
    stats: prefs.include_kimi ? kimiStats : null,
    user,
    optedIn,
    accountSyncEnabled,
    provider: "kimi",
  });
  const glm = useSnapshotUploader({
    stats: prefs.include_glm ? glmStats : null,
    user,
    optedIn,
    accountSyncEnabled,
    provider: "glm",
  });

  const runners = useMemo<Partial<Record<LeaderboardProvider, BackfillRunner>>>(
    () => ({
      claude: prefs.include_claude && claude.ready ? claude.manualBackfill : undefined,
      codex: prefs.include_codex && codex.ready ? codex.manualBackfill : undefined,
      opencode: prefs.include_opencode && opencode.ready ? opencode.manualBackfill : undefined,
      kimi: prefs.include_kimi && kimi.ready ? kimi.manualBackfill : undefined,
      glm: prefs.include_glm && glm.ready ? glm.manualBackfill : undefined,
    }),
    [
      prefs.include_claude,
      prefs.include_codex,
      prefs.include_opencode,
      prefs.include_kimi,
      prefs.include_glm,
      claude.ready,
      codex.ready,
      opencode.ready,
      kimi.ready,
      glm.ready,
      claude.manualBackfill,
      codex.manualBackfill,
      opencode.manualBackfill,
      kimi.manualBackfill,
      glm.manualBackfill,
    ],
  );

  useEffect(() => {
    return registerBackfillRunner(runners);
  }, [runners]);

  // Auto-trigger first-ever 60-day backfill for Account view. Header used to
  // fire this on toggle-click but that raced the stats-load: clicking flipped
  // `stats_source` before Rust finished its first fetch_stats, so manualBackfill
  // bailed on `!stats` and the flag was lost. Here we have the full ready-state
  // for each provider, so we can wait until it's actually runnable.
  useEffect(() => {
    if (!user || !accountSyncEnabled) return;
    const runOnce = (provider: LeaderboardProvider, runner?: BackfillRunner) => {
      if (!runner) return;
      const flag = `account_initial_backfill_done_${user.id}_${provider}`;
      if (localStorage.getItem(flag)) return;
      runner(60).then((ok) => {
        if (ok) localStorage.setItem(flag, "1");
      });
    };
    if (prefs.include_claude) runOnce("claude", runners.claude);
    if (prefs.include_codex) runOnce("codex", runners.codex);
    if (prefs.include_opencode) runOnce("opencode", runners.opencode);
    if (prefs.include_kimi) runOnce("kimi", runners.kimi);
    if (prefs.include_glm) runOnce("glm", runners.glm);
  }, [
    user?.id, accountSyncEnabled,
    prefs.include_claude, prefs.include_codex, prefs.include_opencode,
    prefs.include_kimi, prefs.include_glm,
    runners,
  ]);

  return null;
}
