import type { ReactNode } from "react";

interface Props {
  children: ReactNode;
}

export function PopoverShell({ children }: Props) {
  return (
    <div id="app-root" style={{
      width: "100vw",
      height: "100vh",
      display: "flex",
      flexDirection: "column",
      background: "var(--bg-primary)",
      borderRadius: "var(--radius-lg)",
      boxShadow: "0 8px 32px rgba(0,0,0,0.12), 0 2px 8px rgba(0,0,0,0.08)",
      overflow: "hidden",
      border: "1px solid rgba(124, 92, 252, 0.1)",
    }}>
      {/* Arrow indicator */}
      <div style={{
        position: "absolute",
        top: -6,
        left: "50%",
        transform: "translateX(-50%)",
        width: 12,
        height: 12,
        background: "var(--bg-primary)",
        border: "1px solid rgba(124, 92, 252, 0.1)",
        borderRight: "none",
        borderBottom: "none",
        borderRadius: 2,
        rotate: "45deg",
        zIndex: 1,
      }} />
      <div style={{
        flex: 1,
        overflowY: "auto",
        overflowX: "hidden",
        padding: 16,
        display: "flex",
        flexDirection: "column",
        gap: 14,
        scrollBehavior: "smooth",
        WebkitOverflowScrolling: "touch",
      }}>
        {children}
      </div>
    </div>
  );
}
