import { useOAuthUsage } from "../hooks/useOAuthUsage";
import { useI18n } from "../i18n/I18nContext";

function getBarColor(percent: number): string {
  if (percent >= 90) return "#ef4444";
  if (percent >= 80) return "#f97316";
  if (percent >= 50) return "#eab308";
  return "#22c55e";
}

function formatResetTime(resetsAt: string, t: (key: string, params?: Record<string, string>) => string): string {
  const reset = new Date(resetsAt);
  const now = new Date();
  const diffMs = reset.getTime() - now.getTime();
  if (diffMs <= 0) return t("usageAlert.resetsNow");
  const totalMin = Math.floor(diffMs / 60000);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  if (h > 0) return t("usageAlert.resetsIn", { time: `${h}h ${m}m` });
  return t("usageAlert.resetsIn", { time: `${m}m` });
}

function UsageRow({
  label,
  utilization,
  subtitle,
}: {
  label: string;
  utilization: number;
  subtitle: string;
}) {
  const pct = Math.min(utilization, 100);
  const color = getBarColor(utilization);

  return (
    <div style={{ marginBottom: 8 }}>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 3,
      }}>
        <span style={{ fontSize: 10, fontWeight: 600, color: "var(--text-primary)" }}>
          {label}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
            {subtitle}
          </span>
          <span style={{ fontSize: 11, fontWeight: 700, color }}>
            {utilization.toFixed(1)}%
          </span>
        </div>
      </div>
      <div style={{
        width: "100%",
        height: 5,
        borderRadius: 3,
        background: "var(--heat-0)",
        overflow: "hidden",
      }}>
        <div style={{
          width: `${pct}%`,
          height: "100%",
          borderRadius: 3,
          background: color,
          transition: "width 0.3s ease, background 0.3s ease",
        }} />
      </div>
    </div>
  );
}

export function UsageAlertBar() {
  const { usage } = useOAuthUsage();
  const t = useI18n();

  if (!usage) return null;

  const { five_hour, seven_day, extra_usage, is_stale } = usage;

  // Don't render if no data at all
  if (!five_hour && !seven_day && !extra_usage) return null;

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: "12px 16px",
    }}>
      {/* Header */}
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 8,
      }}>
        <span style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-primary)",
        }}>
          {t("usageAlert.title")}
        </span>
        {is_stale && (
          <span style={{
            fontSize: 9,
            fontWeight: 600,
            color: "var(--text-muted)",
          }}>
            {t("usageAlert.stale")}
          </span>
        )}
      </div>

      {/* Session (5h) */}
      {five_hour && (
        <UsageRow
          label={t("usageAlert.session")}
          utilization={five_hour.utilization}
          subtitle={formatResetTime(five_hour.resets_at, t)}
        />
      )}

      {/* Weekly (7d) */}
      {seven_day && (
        <UsageRow
          label={t("usageAlert.weekly")}
          utilization={seven_day.utilization}
          subtitle={formatResetTime(seven_day.resets_at, t)}
        />
      )}

      {/* Extra usage */}
      {extra_usage && extra_usage.is_enabled && (
        <UsageRow
          label={t("usageAlert.extraUsage")}
          utilization={extra_usage.utilization}
          subtitle={`$${extra_usage.used_credits.toFixed(0)} / $${extra_usage.monthly_limit.toFixed(0)}`}
        />
      )}
    </div>
  );
}
