import type { AccountState, LimitWindowStatus } from "./types";

const CODEX_WEEKLY_WINDOW_MINUTES = 10080;

type Provider = "claude" | "codex";

type WindowLike = Pick<LimitWindowStatus, "name" | "window_minutes" | "resets_at">;

export interface WeeklyResetSummary {
  provider: Provider;
  resetsAt: string | null;
}

export interface CountdownLabels {
  unavailable: string;
  resetting: string;
  day: string;
  hour: string;
  minute: string;
}

export function formatWindowDuration(minutes?: number | null): string | null {
  if (minutes == null || !Number.isFinite(minutes) || minutes <= 0) return null;
  if (minutes === 300) return "5h";
  if (minutes === CODEX_WEEKLY_WINDOW_MINUTES) return "7d";
  if (minutes % 1440 === 0) return `${minutes / 1440}d`;
  if (minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

export function displayLimitWindowName(provider: string, window: WindowLike): string {
  const duration = formatWindowDuration(window.window_minutes);
  if (duration) return duration;

  if (provider === "claude") {
    const stripped = window.name.replace(/^Claude\s+/i, "").trim();
    return stripped || window.name;
  }

  if (provider === "codex") {
    const parenthetical = window.name.match(/\(([^)]+)\)/)?.[1]?.trim();
    if (parenthetical) return parenthetical;

    const stripped = window.name.replace(/\s*Usage\b/i, "").trim();
    return stripped || window.name;
  }

  return window.name;
}

export function getWeeklyResetSummaries(
  states: Array<Pick<AccountState, "provider" | "limit_windows">>,
): WeeklyResetSummary[] {
  return (["claude", "codex"] as const)
    .map((provider) => {
      const state = states.find((item) => item.provider === provider);
      if (!state) return null;

      const window = provider === "claude"
        ? state.limit_windows.find((item) => item.name.trim().toLowerCase() === "claude 7d")
        : state.limit_windows.find((item) => item.window_minutes === CODEX_WEEKLY_WINDOW_MINUTES);

      return {
        provider,
        resetsAt: window?.resets_at ?? null,
      };
    })
    .filter((item): item is WeeklyResetSummary => item != null);
}

export function formatResetCountdown(
  resetAt: string | null | undefined,
  now: Date,
  labels: CountdownLabels,
): string {
  if (!resetAt) return labels.unavailable;

  const resetDate = new Date(resetAt);
  if (Number.isNaN(resetDate.getTime())) return labels.unavailable;

  const diffMs = resetDate.getTime() - now.getTime();
  if (diffMs <= 0) return labels.resetting;

  const totalMinutes = Math.ceil(diffMs / 60_000);
  const days = Math.floor(totalMinutes / 1440);
  const hours = Math.floor((totalMinutes % 1440) / 60);
  const minutes = totalMinutes % 60;
  const paddedHours = String(hours).padStart(2, "0");
  const paddedMinutes = String(minutes).padStart(2, "0");

  if (days > 0) {
    return `${days}${labels.day} ${paddedHours}${labels.hour} ${paddedMinutes}${labels.minute}`;
  }
  if (hours > 0) {
    return `${hours}${labels.hour} ${paddedMinutes}${labels.minute}`;
  }
  return `${minutes}${labels.minute}`;
}
