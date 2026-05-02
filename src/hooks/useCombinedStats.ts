import { useMemo } from "react";
import { useTokenStats } from "./useTokenStats";
import type {
  ActivityCategory,
  AllStats,
  AnalyticsData,
  DailyUsage,
  McpServerUsage,
  ModelUsage,
  ProjectUsage,
  ToolCount,
} from "../lib/types";

interface UseCombinedStatsProps {
  includeClaude: boolean;
  includeCodex: boolean;
}

export function useCombinedStats({ includeClaude, includeCodex }: UseCombinedStatsProps) {
  const claude = useTokenStats("claude", includeClaude);
  const codex = useTokenStats("codex", includeCodex);

  const stats = useMemo<AllStats | null>(() => {
    const sources: (AllStats | null)[] = [];
    if (includeClaude) sources.push(claude.stats);
    if (includeCodex) sources.push(codex.stats);

    const validStats = sources.filter((s): s is AllStats => s !== null);
    if (validStats.length === 0) {
      if (includeClaude) return claude.stats;
      if (includeCodex) return codex.stats;
      return null;
    }
    if (validStats.length === 1) return validStats[0];

    return mergeStats(validStats);
  }, [claude.stats, codex.stats, includeClaude, includeCodex]);

  const loading = (includeClaude && claude.loading) || (includeCodex && codex.loading);
  const error = useMemo(() => {
    if (stats) return null;

    if (includeClaude && claude.error) return claude.error;
    if (includeCodex && codex.error) return codex.error;

    return null;
  }, [stats, includeClaude, includeCodex, claude.error, codex.error]);

  return { stats, loading, error };
}

function mergeStats(statsList: AllStats[]): AllStats {
  const dailyMap = new Map<string, DailyUsage>();
  const modelUsage: Record<string, ModelUsage> = {};
  let totalMessages = 0;
  let totalSessions = 0;
  let firstDate: string | null = null;

  for (const s of statsList) {
    totalMessages += s.total_messages;
    totalSessions += s.total_sessions;

    if (s.first_session_date && (!firstDate || s.first_session_date < firstDate)) {
      firstDate = s.first_session_date;
    }

    for (const d of s.daily) {
      const existing = dailyMap.get(d.date);
      if (existing) {
        for (const [model, tokens] of Object.entries(d.tokens)) {
          existing.tokens[model] = (existing.tokens[model] ?? 0) + tokens;
        }
        existing.cost_usd += d.cost_usd;
        existing.messages += d.messages;
        existing.sessions += d.sessions;
        existing.tool_calls += d.tool_calls;
        existing.input_tokens += d.input_tokens;
        existing.output_tokens += d.output_tokens;
        existing.cache_read_tokens += d.cache_read_tokens;
        existing.cache_write_tokens += d.cache_write_tokens;
      } else {
        dailyMap.set(d.date, {
          date: d.date,
          tokens: { ...d.tokens },
          cost_usd: d.cost_usd,
          messages: d.messages,
          sessions: d.sessions,
          tool_calls: d.tool_calls,
          input_tokens: d.input_tokens,
          output_tokens: d.output_tokens,
          cache_read_tokens: d.cache_read_tokens,
          cache_write_tokens: d.cache_write_tokens,
        });
      }
    }

    for (const [model, usage] of Object.entries(s.model_usage)) {
      const e = modelUsage[model];
      if (e) {
        e.input_tokens += usage.input_tokens;
        e.output_tokens += usage.output_tokens;
        e.cache_read += usage.cache_read;
        e.cache_write += usage.cache_write;
        e.cost_usd += usage.cost_usd;
      } else {
        modelUsage[model] = { ...usage };
      }
    }
  }

  const daily = Array.from(dailyMap.values()).sort((a, b) => a.date.localeCompare(b.date));

  const analytics = mergeAnalytics(statsList);

  return {
    daily,
    model_usage: modelUsage,
    total_sessions: totalSessions,
    total_messages: totalMessages,
    first_session_date: firstDate,
    analytics,
  };
}

function mergeAnalytics(statsList: AllStats[]): AnalyticsData | undefined {
  const analyticsList = statsList
    .map((s) => s.analytics)
    .filter((a): a is AnalyticsData => !!a);

  if (analyticsList.length === 0) return undefined;
  if (analyticsList.length === 1) return analyticsList[0];

  const projects = new Map<string, ProjectUsage>();
  const tools = new Map<string, ToolCount>();
  const shell = new Map<string, ToolCount>();
  const mcp = new Map<string, McpServerUsage>();
  const activity = new Map<string, ActivityCategory>();

  for (const analytics of analyticsList) {
    for (const item of analytics.project_usage) {
      const existing = projects.get(item.name);
      if (existing) {
        existing.cost_usd += item.cost_usd;
        existing.tokens += item.tokens;
        existing.sessions += item.sessions;
        existing.messages += item.messages;
      } else {
        projects.set(item.name, { ...item });
      }
    }

    for (const item of analytics.tool_usage) {
      const existing = tools.get(item.name);
      if (existing) existing.count += item.count;
      else tools.set(item.name, { ...item });
    }

    for (const item of analytics.shell_commands) {
      const existing = shell.get(item.name);
      if (existing) existing.count += item.count;
      else shell.set(item.name, { ...item });
    }

    for (const item of analytics.mcp_usage) {
      const existing = mcp.get(item.server);
      if (existing) existing.calls += item.calls;
      else mcp.set(item.server, { ...item });
    }

    for (const item of analytics.activity_breakdown) {
      const existing = activity.get(item.category);
      if (existing) {
        existing.cost_usd += item.cost_usd;
        existing.messages += item.messages;
      } else {
        activity.set(item.category, { ...item });
      }
    }
  }

  return {
    project_usage: Array.from(projects.values()).sort((a, b) => b.cost_usd - a.cost_usd),
    tool_usage: Array.from(tools.values()).sort((a, b) => b.count - a.count),
    shell_commands: Array.from(shell.values()).sort((a, b) => b.count - a.count),
    mcp_usage: Array.from(mcp.values()).sort((a, b) => b.calls - a.calls),
    activity_breakdown: Array.from(activity.values()).sort((a, b) => b.cost_usd - a.cost_usd),
  };
}
