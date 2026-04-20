import { useSettings } from "../contexts/SettingsContext";
import { useAuth } from "../contexts/AuthContext";
import { useTokenStats } from "./useTokenStats";
import { useAccountStats } from "./useAccountStats";
import type { AllStats, LeaderboardProvider } from "../lib/types";

interface StatsSourceResult {
  stats: AllStats | null;
  loading: boolean;
  error: string | null;
}

/**
 * Returns either local file-based stats (default) or server-aggregated
 * cross-device stats based on SettingsContext.prefs.stats_source. Falls back
 * to local whenever the user is signed out, since account mode requires auth.
 *
 * Both hooks are always called (rules of hooks); `enabled` gates the network
 * fetch inside useAccountStats so local-mode users don't pay the RPC cost.
 */
export function useStatsSource(provider: LeaderboardProvider): StatsSourceResult {
  const { prefs } = useSettings();
  const { user } = useAuth();
  const local = useTokenStats(provider);

  const useAccount = prefs.stats_source === "account" && user != null;
  const account = useAccountStats({ provider, user, enabled: useAccount });

  if (useAccount) {
    return { stats: account.stats, loading: account.loading, error: account.error };
  }
  return { stats: local.stats, loading: local.loading, error: local.error };
}
