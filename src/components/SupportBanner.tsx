import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

const KOFI_URL = "https://ko-fi.com/soulduse";
const TOSS_URL = "https://toss.me/soulduse";

export function SupportBanner() {
  const [showOptions, setShowOptions] = useState(false);

  return (
    <div style={{ position: "relative", marginTop: 4 }}>
      <button
        onClick={() => setShowOptions(!showOptions)}
        style={{
          width: "100%",
          padding: "10px 0",
          fontSize: 12,
          fontWeight: 700,
          border: "none",
          borderRadius: "var(--radius-md)",
          cursor: "pointer",
          background: "var(--bg-card)",
          color: "var(--text-secondary)",
          boxShadow: "var(--shadow-card)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 6,
          transition: "all 0.2s ease",
        }}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
        </svg>
        Support this project
      </button>

      {showOptions && (
        <>
          <div
            onClick={() => setShowOptions(false)}
            style={{ position: "fixed", inset: 0, zIndex: 60 }}
          />
          <div style={{
            position: "absolute",
            bottom: "100%",
            left: "50%",
            transform: "translateX(-50%)",
            marginBottom: 6,
            background: "var(--bg-card)",
            borderRadius: "var(--radius-md)",
            boxShadow: "0 8px 24px rgba(0,0,0,0.15)",
            padding: 8,
            zIndex: 61,
            minWidth: 200,
            border: "1px solid var(--heat-0)",
          }}>
            <SupportOption
              icon={
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M17 8h1a4 4 0 1 1 0 8h-1"/>
                  <path d="M3 8h14v9a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4Z"/>
                  <line x1="6" y1="2" x2="6" y2="4"/>
                  <line x1="10" y1="2" x2="10" y2="4"/>
                  <line x1="14" y1="2" x2="14" y2="4"/>
                </svg>
              }
              label="Ko-fi"
              description="International (PayPal)"
              onClick={() => {
                openUrl(KOFI_URL);
                setShowOptions(false);
              }}
            />
            <SupportOption
              icon={
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="2" y="5" width="20" height="14" rx="2"/>
                  <line x1="2" y1="10" x2="22" y2="10"/>
                </svg>
              }
              label="Toss"
              description="국내 송금"
              onClick={() => {
                openUrl(TOSS_URL);
                setShowOptions(false);
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}

function SupportOption({
  icon,
  label,
  description,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  description: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        width: "100%",
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "8px 10px",
        border: "none",
        borderRadius: "var(--radius-sm)",
        background: "transparent",
        cursor: "pointer",
        textAlign: "left",
        transition: "background 0.15s ease",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--heat-0)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
      }}
    >
      <div style={{ color: "var(--accent-purple)", flexShrink: 0 }}>
        {icon}
      </div>
      <div>
        <div style={{ fontSize: 12, fontWeight: 700, color: "var(--text-primary)" }}>
          {label}
        </div>
        <div style={{ fontSize: 9, color: "var(--text-secondary)" }}>
          {description}
        </div>
      </div>
    </button>
  );
}
