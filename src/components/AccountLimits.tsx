import type { ReactNode } from "react";
import type {
  AccountState,
  BalanceInfo,
  ClientUsage,
  LimitStatus,
  LimitWindowStatus,
} from "../lib/types";
import { formatCost, formatTokens } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  states: AccountState[];
  loading?: boolean;
}

const MAX_CLIENTS = 8;

function Card({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 16,
      boxShadow: "var(--shadow-card)",
    }}>
      <div style={{
        fontSize: 12,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
        marginBottom: 10,
      }}>
        {title}
      </div>
      {children}
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div style={{
      color: "var(--text-secondary)",
      fontSize: 12,
      fontWeight: 600,
      padding: "12px 0",
      textAlign: "center",
    }}>
      {text}
    </div>
  );
}

function DiagnosticRows({ items }: { items: Array<{ key: string; provider: string; message: string }> }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {items.map((item) => (
        <div
          key={item.key}
          style={{
            color: "var(--text-secondary)",
            fontSize: 12,
            fontWeight: 600,
            lineHeight: 1.45,
          }}
        >
          <span style={{ color: "var(--text-primary)", fontWeight: 800 }}>
            {providerLabel(item.provider)}
          </span>
          {" · "}
          {item.message}
        </div>
      ))}
    </div>
  );
}

function statusColor(status: LimitStatus): string {
  switch (status) {
    case "ok":
      return "var(--accent-mint)";
    case "warning":
      return "var(--accent-orange)";
    case "critical":
    case "exhausted":
      return "var(--accent-pink)";
    default:
      return "var(--text-secondary)";
  }
}

function providerLabel(provider: string): string {
  if (provider === "claude") return "Claude";
  if (provider === "codex") return "Codex";
  return provider;
}

function percentLabel(value?: number | null): string {
  if (value == null || Number.isNaN(value)) return "-";
  return `${Math.round(value)}%`;
}

function formatReset(value?: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatNumber(value?: number | null, unit?: string): string {
  if (value == null || Number.isNaN(value)) return "-";
  const normalizedUnit = unit?.toLowerCase();
  if (normalizedUnit === "usd") return formatCost(value);
  if (normalizedUnit === "percent" || unit === "%") return percentLabel(value);
  return `${value.toLocaleString(undefined, { maximumFractionDigits: 2 })} ${unit ?? ""}`.trim();
}

function LimitRow({ provider, window }: { provider: string; window: LimitWindowStatus }) {
  const t = useI18n();
  const color = statusColor(window.status);
  const percent = Math.max(0, Math.min(100, window.used_percent ?? 0));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 10 }}>
        <div style={{ minWidth: 0 }}>
          <span style={{ fontSize: 13, fontWeight: 700, color: "var(--text-primary)" }}>
            {providerLabel(provider)}
          </span>
          <span style={{
            marginLeft: 6,
            fontSize: 12,
            fontWeight: 600,
            color: "var(--text-secondary)",
          }}>
            {window.name}
          </span>
        </div>
        <span style={{ fontSize: 13, fontWeight: 800, color }}>
          {percentLabel(window.used_percent)}
        </span>
      </div>
      <div style={{
        height: 7,
        borderRadius: 4,
        background: "var(--heat-0)",
        overflow: "hidden",
      }}>
        <div style={{
          width: `${percent}%`,
          height: "100%",
          borderRadius: 4,
          background: color,
        }} />
      </div>
      <div style={{
        display: "flex",
        justifyContent: "space-between",
        gap: 8,
        color: "var(--text-secondary)",
        fontSize: 11,
        fontWeight: 600,
      }}>
        <span>{t(`limits.status.${window.status}`)}</span>
        <span>{t("limits.reset")}: {formatReset(window.resets_at)}</span>
      </div>
    </div>
  );
}

function BalanceRow({ provider, balance }: { provider: string; balance: BalanceInfo }) {
  const t = useI18n();
  const usedPercent = balance.total && balance.used != null && balance.total > 0
    ? (balance.used / balance.total) * 100
    : null;
  const color = statusColor(balance.status);
  const valueLabel = balance.is_unlimited
    ? t("limits.unlimited")
    : formatNumber(balance.remaining ?? balance.balance, balance.unit);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
        <span style={{ fontSize: 13, fontWeight: 700, color: "var(--text-primary)" }}>
          {providerLabel(provider)}
        </span>
        <span style={{ color, fontSize: 13, fontWeight: 800 }}>
          {valueLabel}
        </span>
      </div>
      {!balance.is_unlimited && (
        <div style={{ fontSize: 11, fontWeight: 600, color: "var(--text-secondary)" }}>
          {t("limits.used")}: {formatNumber(balance.used, balance.unit)}
          {" · "}
          {t("limits.total")}: {formatNumber(balance.total, balance.unit)}
          {usedPercent != null ? ` · ${percentLabel(usedPercent)}` : ""}
        </div>
      )}
    </div>
  );
}

function ClientRows({ clients }: { clients: Array<{ provider: string; client: ClientUsage }> }) {
  const { prefs } = useSettings();
  const items = [...clients]
    .sort((a, b) => b.client.requests - a.client.requests)
    .slice(0, MAX_CLIENTS);
  const maxRequests = items[0]?.client.requests ?? 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
      {items.map(({ provider, client }) => {
        const width = maxRequests > 0 ? (client.requests / maxRequests) * 100 : 0;
        return (
          <div key={`${provider}:${client.name}`} style={{ display: "grid", gridTemplateColumns: "88px 1fr 64px", gap: 8, alignItems: "center" }}>
            <span style={{
              color: "var(--text-primary)",
              fontSize: 12,
              fontWeight: 700,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}>
              {client.name}
            </span>
            <div style={{
              height: 7,
              borderRadius: 4,
              background: "var(--heat-0)",
              overflow: "hidden",
            }}>
              <div style={{
                width: `${width}%`,
                height: "100%",
                borderRadius: 4,
                background: "var(--accent-purple)",
              }} />
            </div>
            <span style={{
              color: "var(--text-secondary)",
              fontSize: 11,
              fontWeight: 700,
              textAlign: "right",
            }}>
              {client.requests.toLocaleString()}
            </span>
            <span />
            <span style={{ color: "var(--text-secondary)", fontSize: 10, fontWeight: 600 }}>
              {formatTokens(client.tokens, prefs.number_format)}
            </span>
            <span style={{ color: "var(--text-secondary)", fontSize: 10, fontWeight: 600, textAlign: "right" }}>
              {percentLabel(client.percent)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

export function AccountLimits({ states, loading = false }: Props) {
  const t = useI18n();
  const windows = states.flatMap((state) =>
    state.limit_windows.map((window) => ({ provider: state.provider, window })),
  );
  const balances = states.flatMap((state) =>
    state.balance ? [{ provider: state.provider, balance: state.balance }] : [],
  );
  const clients = states.flatMap((state) =>
    state.client_distribution.map((client) => ({ provider: state.provider, client })),
  );
  const diagnostics = states.flatMap((state) =>
    (state.diagnostics ?? []).map((message, index) => ({
      key: `${state.provider}:${index}:${message}`,
      provider: state.provider,
      message,
    })),
  );
  const hasStale = states.some((state) => state.is_stale);
  const hasData = windows.length > 0 || balances.length > 0 || clients.length > 0;

  if (loading && states.length === 0) {
    return (
      <Card title={t("limits.title")}>
        <Empty text={t("limits.loading")} />
      </Card>
    );
  }

  return (
    <>
      {hasStale && (
        <div style={{
          background: "var(--bg-card)",
          borderRadius: "var(--radius-lg)",
          padding: "10px 12px",
          boxShadow: "var(--shadow-card)",
          color: "var(--accent-orange)",
          fontSize: 12,
          fontWeight: 700,
        }}>
          {t("limits.stale")}
        </div>
      )}

      {diagnostics.length > 0 && (
        <Card title={t("limits.diagnostics")}>
          <DiagnosticRows items={diagnostics} />
        </Card>
      )}

      {windows.length > 0 && (
        <Card title={t("limits.windows")}>
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            {windows.map(({ provider, window }) => (
              <LimitRow key={`${provider}:${window.name}`} provider={provider} window={window} />
            ))}
          </div>
        </Card>
      )}

      {balances.length > 0 && (
        <Card title={t("limits.balance")}>
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {balances.map(({ provider, balance }) => (
              <BalanceRow key={provider} provider={provider} balance={balance} />
            ))}
          </div>
        </Card>
      )}

      {clients.length > 0 && (
        <Card title={t("limits.clients")}>
          <ClientRows clients={clients} />
        </Card>
      )}

      {!hasData && diagnostics.length === 0 && (
        <Card title={t("limits.title")}>
          <Empty text={t("limits.empty.all")} />
        </Card>
      )}
    </>
  );
}
