import { useState, useCallback } from "react";
import html2canvas from "html2canvas";
import { SettingsOverlay } from "./SettingsOverlay";
import type { AllStats } from "../lib/types";
import { formatTokens, formatCost, getTotalTokens, toLocalDateStr } from "../lib/format";

interface Props {
  stats?: AllStats | null;
}

export function Header({ stats }: Props) {
  const [showSettings, setShowSettings] = useState(false);
  const [copied, setCopied] = useState(false);
  const [captured, setCaptured] = useState(false);

  const handleCapture = useCallback(async () => {
    const el = document.getElementById("app-root");
    if (!el) return;
    try {
      const canvas = await html2canvas(el, {
        backgroundColor: null,
        scale: 2,
      });
      canvas.toBlob((blob) => {
        if (!blob) return;
        navigator.clipboard.write([
          new ClipboardItem({ "image/png": blob }),
        ]).then(() => {
          setCaptured(true);
          setTimeout(() => setCaptured(false), 2000);
        });
      }, "image/png");
    } catch {
      // fallback: ignore
    }
  }, []);

  const handleExport = useCallback(() => {
    if (!stats) return;

    const todayStr = toLocalDateStr(new Date());
    const today = stats.daily.find((d) => d.date === todayStr);
    const todayTokens = today ? getTotalTokens(today.tokens) : 0;
    const todayCost = today?.cost_usd ?? 0;

    const totalTokens = stats.daily.reduce((sum, d) => sum + getTotalTokens(d.tokens), 0);
    const totalCost = stats.daily.reduce((sum, d) => sum + d.cost_usd, 0);

    const lines = [
      `# AI Token Monitor Summary`,
      `**Date:** ${todayStr}`,
      ``,
      `## Today`,
      `- Tokens: ${formatTokens(todayTokens, "full")}`,
      `- Cost: ${formatCost(todayCost)}`,
      `- Messages: ${today?.messages ?? 0}`,
      ``,
      `## All Time`,
      `- Total Tokens: ${formatTokens(totalTokens, "full")}`,
      `- Total Cost: ${formatCost(totalCost)}`,
      `- Total Sessions: ${stats.total_sessions}`,
      `- Total Messages: ${stats.total_messages}`,
      ``,
      `## Models`,
      ...Object.entries(stats.model_usage).map(
        ([model, u]) => `- **${model}**: ${formatTokens(u.input_tokens + u.output_tokens + u.cache_read, "full")} tokens, ${formatCost(u.cost_usd)}`
      ),
    ];

    navigator.clipboard.writeText(lines.join("\n")).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [stats]);

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      gap: 10,
      paddingBottom: 4,
      position: "relative",
    }}>
      <div style={{
        width: 36,
        height: 36,
        borderRadius: "var(--radius-sm)",
        background: "linear-gradient(135deg, var(--accent-purple), var(--accent-pink))",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontSize: 20,
        flexShrink: 0,
      }}>
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
          <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
          <circle cx="12" cy="16" r="1"/>
        </svg>
      </div>
      <div style={{ flex: 1 }}>
        <div style={{
          fontSize: 15,
          fontWeight: 800,
          letterSpacing: "-0.3px",
          color: "var(--text-primary)",
        }}>
          AI Token Monitor
        </div>
        <div style={{
          fontSize: 11,
          color: "var(--text-secondary)",
          fontWeight: 600,
        }}>
          Claude Code Usage Tracker
        </div>
      </div>

      {/* Share button */}
      <button
        onClick={handleExport}
        title="Copy summary to clipboard"
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: copied ? "var(--accent-mint)" : "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        {copied ? (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12"/>
          </svg>
        ) : (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
          </svg>
        )}
      </button>

      {/* Capture button */}
      <button
        onClick={handleCapture}
        title="Capture screenshot to clipboard"
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: captured ? "var(--accent-mint)" : "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        {captured ? (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12"/>
          </svg>
        ) : (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/>
            <circle cx="12" cy="13" r="4"/>
          </svg>
        )}
      </button>

      {/* Settings button */}
      <button
        onClick={() => setShowSettings(!showSettings)}
        title="Settings"
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: showSettings ? "var(--accent-purple)" : "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="3"/>
          <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/>
        </svg>
      </button>

      <SettingsOverlay
        visible={showSettings}
        onClose={() => setShowSettings(false)}
      />
    </div>
  );
}
