import { useState, useMemo } from "react";
import { useTokenStats } from "./hooks/useTokenStats";
import { getTotalTokens } from "./lib/format";
import { SettingsProvider } from "./contexts/SettingsContext";
import { PopoverShell } from "./components/PopoverShell";
import { Header } from "./components/Header";
import { TabBar } from "./components/TabBar";
import { TodaySummary } from "./components/TodaySummary";
import { DailyChart } from "./components/DailyChart";
import { Heatmap } from "./components/Heatmap";
import { ModelBreakdown } from "./components/ModelBreakdown";
import { PeriodTotals } from "./components/PeriodTotals";
import { CacheEfficiency } from "./components/CacheEfficiency";

function AppContent() {
  const { stats, error, loading } = useTokenStats();
  const [activeTab, setActiveTab] = useState<"overview" | "analytics">("overview");

  const todayStr = new Date().toISOString().slice(0, 10);

  const { today, weekAvg } = useMemo(() => {
    if (!stats) return { today: null, weekAvg: 0 };

    const today = stats.daily.find((d) => d.date === todayStr) ?? null;

    const last7 = stats.daily
      .filter((d) => {
        const diff = (new Date(todayStr).getTime() - new Date(d.date).getTime()) / 86400000;
        return diff >= 1 && diff <= 7;
      })
      .map((d) => getTotalTokens(d.tokens));

    const weekAvg = last7.length > 0
      ? last7.reduce((a, b) => a + b, 0) / last7.length
      : 0;

    return { today, weekAvg };
  }, [stats, todayStr]);

  if (loading) {
    return (
      <PopoverShell>
        <Header />
        <div style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flex: 1,
          color: "var(--text-secondary)",
          fontSize: 13,
          fontWeight: 600,
        }}>
          Loading...
        </div>
      </PopoverShell>
    );
  }

  if (error || !stats) {
    return (
      <PopoverShell>
        <Header />
        <div style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          flex: 1,
          gap: 8,
          color: "var(--text-secondary)",
          fontSize: 12,
          fontWeight: 600,
          textAlign: "center",
          padding: 20,
        }}>
          <div style={{ fontSize: 24 }}>
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="12" cy="12" r="10"/>
              <path d="M12 8v4M12 16h.01"/>
            </svg>
          </div>
          <div>Claude Code stats not found</div>
          <div style={{ fontSize: 10, color: "var(--text-secondary)" }}>
            Make sure Claude Code is installed and has been used at least once.
          </div>
        </div>
      </PopoverShell>
    );
  }

  return (
    <PopoverShell>
      <Header stats={stats} />
      <TabBar activeTab={activeTab} onChange={setActiveTab} />

      {activeTab === "overview" ? (
        <>
          <TodaySummary today={today} weekAvg={weekAvg} />
          <DailyChart daily={stats.daily} days={7} />
          <PeriodTotals daily={stats.daily} />
          <Heatmap daily={stats.daily} weeks={8} />
        </>
      ) : (
        <>
          <DailyChart daily={stats.daily} days={30} />
          <PeriodTotals daily={stats.daily} />
          <ModelBreakdown modelUsage={stats.model_usage} />
          <CacheEfficiency stats={stats} />
        </>
      )}
    </PopoverShell>
  );
}

function App() {
  return (
    <SettingsProvider>
      <AppContent />
    </SettingsProvider>
  );
}

export default App;
