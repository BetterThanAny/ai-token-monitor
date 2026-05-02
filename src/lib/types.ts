export interface DailyUsage {
  date: string;
  tokens: Record<string, number>;
  cost_usd: number;
  messages: number;
  sessions: number;
  tool_calls: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
}

export interface ModelUsage {
  input_tokens: number;
  output_tokens: number;
  cache_read: number;
  cache_write: number;
  cost_usd: number;
}

export interface ProjectUsage {
  name: string;
  cost_usd: number;
  tokens: number;
  sessions: number;
  messages: number;
}

export interface ToolCount {
  name: string;
  count: number;
}

export interface McpServerUsage {
  server: string;
  calls: number;
}

export interface ActivityCategory {
  category: string;
  cost_usd: number;
  messages: number;
}

export interface AnalyticsData {
  project_usage: ProjectUsage[];
  tool_usage: ToolCount[];
  shell_commands: ToolCount[];
  mcp_usage: McpServerUsage[];
  activity_breakdown: ActivityCategory[];
}

export interface AllStats {
  daily: DailyUsage[];
  model_usage: Record<string, ModelUsage>;
  total_sessions: number;
  total_messages: number;
  first_session_date: string | null;
  analytics?: AnalyticsData;
}

export interface AccountState {
  provider: string;
  fetched_at?: string | null;
  is_stale: boolean;
  limit_windows: LimitWindowStatus[];
  rate_limits: RateLimitStatus[];
  balance?: BalanceInfo | null;
  client_distribution: ClientUsage[];
  diagnostics?: string[];
}

export interface LimitWindowStatus {
  name: string;
  used_percent?: number | null;
  used?: number | null;
  total?: number | null;
  remaining?: number | null;
  unit: string;
  window_minutes?: number | null;
  starts_at?: string | null;
  ends_at?: string | null;
  resets_at?: string | null;
  status: LimitStatus;
  source: string;
}

export interface RateLimitStatus {
  name: string;
  limit?: number | null;
  remaining?: number | null;
  used_percent?: number | null;
  unit: string;
  window_minutes?: number | null;
  resets_at?: string | null;
  status: LimitStatus;
  source: string;
}

export interface BalanceInfo {
  balance?: number | null;
  used?: number | null;
  total?: number | null;
  remaining?: number | null;
  unit: string;
  currency?: string | null;
  expires_at?: string | null;
  is_unlimited?: boolean;
  status: LimitStatus;
}

export interface ClientUsage {
  name: string;
  requests: number;
  tokens: number;
  cost_usd: number;
  percent: number;
}

export type LimitStatus = "ok" | "warning" | "critical" | "exhausted" | "unknown";

export interface UserPreferences {
  number_format: "compact" | "full";
  show_tray_cost: boolean;
  include_claude: boolean;
  include_codex: boolean;
  theme: "github" | "purple" | "ocean" | "sunset";
  color_mode: "system" | "light" | "dark";
  language: "en" | "ko" | "ja" | "zh-CN" | "zh-TW" | "fr" | "es" | "de" | "tr" | "it";
  config_dirs: string[];
  codex_dirs: string[];
  salary_enabled: boolean;
  monthly_salary?: number;
  usage_alerts_enabled: boolean;
  ai_keys?: {
    gemini?: string;
    openai?: string;
    anthropic?: string;
    webhook_discord_url?: string;
    webhook_slack_url?: string;
    webhook_telegram_bot_token?: string;
    webhook_telegram_chat_id?: string;
  };
  webhook_config?: WebhookConfig;
  autostart_enabled: boolean;
  quick_action_items: string[];
}

export interface WebhookConfig {
  discord_enabled: boolean;
  slack_enabled: boolean;
  telegram_enabled: boolean;
  thresholds: number[];
  notify_on_reset: boolean;
  monitored_windows: MonitoredWindows;
}

export interface MonitoredWindows {
  five_hour: boolean;
  seven_day: boolean;
  seven_day_sonnet: boolean;
  seven_day_opus: boolean;
  extra_usage: boolean;
}

export interface UsageWindow {
  utilization: number;
  resets_at?: string | null;
}

export interface ExtraUsage {
  is_enabled: boolean;
  monthly_limit: number;
  used_credits: number;
  utilization: number;
}
