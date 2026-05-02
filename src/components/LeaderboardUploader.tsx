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

  // Stats hooks are always called (rules of hooks), but disabled providers do
  // not invoke backend scans.
  const { stats: claudeStats } = useTokenStats("claude", optedIn && prefs.include_claude);
  const { stats: codexStats } = useTokenStats("codex", optedIn && prefs.include_codex);

  const claude = useSnapshotUploader({
    stats: prefs.include_claude ? claudeStats : null,
    user,
    optedIn,
    provider: "claude",
  });
  const codex = useSnapshotUploader({
    stats: prefs.include_codex ? codexStats : null,
    user,
    optedIn,
    provider: "codex",
  });

  const runners = useMemo<Partial<Record<LeaderboardProvider, BackfillRunner>>>(
    () => ({
      claude: prefs.include_claude && claude.ready ? claude.manualBackfill : undefined,
      codex: prefs.include_codex && codex.ready ? codex.manualBackfill : undefined,
    }),
    [
      prefs.include_claude,
      prefs.include_codex,
      claude.ready,
      codex.ready,
      claude.manualBackfill,
      codex.manualBackfill,
    ],
  );

  useEffect(() => {
    return registerBackfillRunner(runners);
  }, [runners]);

  return null;
}
