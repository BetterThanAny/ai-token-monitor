import { useSettings } from "../contexts/SettingsContext";

interface Props {
  visible: boolean;
  onClose: () => void;
}

export function SettingsOverlay({ visible, onClose }: Props) {
  const { prefs, updatePrefs } = useSettings();

  if (!visible) return null;

  return (
    <>
      <div
        onClick={onClose}
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 50,
        }}
      />
      <div style={{
        position: "absolute",
        top: 48,
        right: 16,
        background: "var(--bg-card)",
        borderRadius: "var(--radius-md)",
        boxShadow: "0 8px 24px rgba(0,0,0,0.15)",
        padding: 12,
        zIndex: 51,
        minWidth: 200,
        border: "1px solid rgba(124, 92, 252, 0.1)",
      }}>
        <div style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-secondary)",
          textTransform: "uppercase",
          letterSpacing: "0.5px",
          marginBottom: 10,
        }}>
          Settings
        </div>

        <SettingRow
          label="Number Format"
          description={prefs.number_format === "compact" ? "377.0K" : "377,000"}
        >
          <ToggleButton
            options={["compact", "full"]}
            value={prefs.number_format}
            onChange={(v) => updatePrefs({ number_format: v as "compact" | "full" })}
          />
        </SettingRow>

        <SettingRow label="Menu Bar Cost">
          <ToggleSwitch
            checked={prefs.show_tray_cost}
            onChange={(v) => updatePrefs({ show_tray_cost: v })}
          />
        </SettingRow>
      </div>
    </>
  );
}

function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "6px 0",
    }}>
      <div>
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>{label}</div>
        {description && (
          <div style={{ fontSize: 10, color: "var(--text-secondary)" }}>{description}</div>
        )}
      </div>
      {children}
    </div>
  );
}

function ToggleButton({
  options,
  value,
  onChange,
}: {
  options: string[];
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div style={{
      display: "flex",
      background: "var(--heat-0)",
      borderRadius: 6,
      padding: 2,
    }}>
      {options.map((opt) => (
        <button
          key={opt}
          onClick={() => onChange(opt)}
          style={{
            fontSize: 10,
            fontWeight: 600,
            padding: "3px 8px",
            borderRadius: 4,
            border: "none",
            cursor: "pointer",
            background: value === opt ? "var(--accent-purple)" : "transparent",
            color: value === opt ? "#fff" : "var(--text-secondary)",
            transition: "all 0.15s ease",
          }}
        >
          {opt === "compact" ? "K/M" : "Full"}
        </button>
      ))}
    </div>
  );
}

function ToggleSwitch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div
      onClick={() => onChange(!checked)}
      style={{
        width: 36,
        height: 20,
        borderRadius: 10,
        background: checked ? "var(--accent-purple)" : "var(--heat-0)",
        cursor: "pointer",
        position: "relative",
        transition: "background 0.2s ease",
        flexShrink: 0,
      }}
    >
      <div style={{
        width: 16,
        height: 16,
        borderRadius: 8,
        background: "#fff",
        position: "absolute",
        top: 2,
        left: checked ? 18 : 2,
        transition: "left 0.2s ease",
        boxShadow: "0 1px 3px rgba(0,0,0,0.2)",
      }} />
    </div>
  );
}
