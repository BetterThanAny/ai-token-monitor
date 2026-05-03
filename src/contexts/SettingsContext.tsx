import { createContext, useContext, useEffect, useState, useCallback, useRef } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { enable as enableAutostart, disable as disableAutostart, isEnabled as isAutostartEnabled } from "@tauri-apps/plugin-autostart";
import type { UserPreferences } from "../lib/types";

interface SettingsContextType {
  prefs: UserPreferences;
  updatePrefs: (partial: Partial<UserPreferences>) => void;
  refreshPrefs: () => Promise<void>;
  ready: boolean;
}

const defaultPrefs: UserPreferences = {
  number_format: "compact",
  show_tray_cost: true,
  include_claude: true,
  include_codex: false,
  theme: "github",
  color_mode: "system",
  language: "en",
  config_dirs: ["~/.claude"],
  codex_dirs: ["~/.codex"],
  salary_enabled: false,
  usage_alerts_enabled: true,
  autostart_enabled: false,
  quick_action_items: [],
};

const SettingsContext = createContext<SettingsContextType>({
  prefs: defaultPrefs,
  updatePrefs: () => {},
  refreshPrefs: async () => {},
  ready: false,
});

async function loadPreferencesWithKeys(): Promise<UserPreferences> {
  const prefs = await invoke<UserPreferences>("get_preferences");
  try {
    const keys = await invoke<UserPreferences["ai_keys"] | null>("get_ai_keys");
    return keys ? { ...prefs, ai_keys: keys } : prefs;
  } catch {
    return prefs;
  }
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<UserPreferences>(defaultPrefs);
  const [ready, setReady] = useState(false);
  const skipNextPersist = useRef(true);
  const prevConfigDirsRef = useRef<string>(JSON.stringify(defaultPrefs.config_dirs));
  const prevCodexDirsRef = useRef<string>(JSON.stringify(defaultPrefs.codex_dirs));
  const prevIncludeClaudeRef = useRef(defaultPrefs.include_claude);
  const prevIncludeCodexRef = useRef(defaultPrefs.include_codex);

  useEffect(() => {
    loadPreferencesWithKeys().then((p) => {
      setPrefs(p);
      // Skip the persist effect triggered by the initial load from disk.
      skipNextPersist.current = true;
      prevConfigDirsRef.current = JSON.stringify(p.config_dirs);
      prevCodexDirsRef.current = JSON.stringify(p.codex_dirs);
      prevIncludeClaudeRef.current = p.include_claude;
      prevIncludeCodexRef.current = p.include_codex;
      setReady(true);
    }).catch(() => {
      setReady(true);
    });
  }, []);

  // Apply theme to document
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", prefs.theme);
  }, [prefs.theme]);

  // Apply color mode (light/dark/system)
  useEffect(() => {
    const root = document.documentElement;
    const apply = (isDark: boolean) => {
      root.setAttribute("data-color-mode", isDark ? "dark" : "light");
    };

    if (prefs.color_mode === "dark") {
      apply(true);
      return;
    }
    if (prefs.color_mode === "light") {
      apply(false);
      return;
    }
    // system: follow OS preference
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    apply(mq.matches);
    const handler = (e: MediaQueryListEvent) => apply(e.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [prefs.color_mode]);

  // Reconcile autostart plugin state with the stored preference.
  // Preference is the source of truth — if the OS state drifts (e.g. user removed
  // the login item manually), we restore it on next launch.
  useEffect(() => {
    if (!ready) return;
    let cancelled = false;
    (async () => {
      try {
        const actual = await isAutostartEnabled();
        if (cancelled) return;
        if (prefs.autostart_enabled && !actual) {
          await enableAutostart();
        } else if (!prefs.autostart_enabled && actual) {
          await disableAutostart();
        }
      } catch (err) {
        console.warn("[autostart] sync failed", err);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [ready, prefs.autostart_enabled]);

  // Persist to disk when prefs change
  useEffect(() => {
    if (skipNextPersist.current) {
      skipNextPersist.current = false;
      prevConfigDirsRef.current = JSON.stringify(prefs.config_dirs);
      prevCodexDirsRef.current = JSON.stringify(prefs.codex_dirs);
      prevIncludeClaudeRef.current = prefs.include_claude;
      prevIncludeCodexRef.current = prefs.include_codex;
      return;
    }
    if (!ready) return;

    // If source selection or config dirs changed, refresh after prefs are persisted.
    const newDirsJson = JSON.stringify(prefs.config_dirs);
    const newCodexDirsJson = JSON.stringify(prefs.codex_dirs);
    const shouldRefreshStats =
      newDirsJson !== prevConfigDirsRef.current ||
      newCodexDirsJson !== prevCodexDirsRef.current ||
      prefs.include_claude !== prevIncludeClaudeRef.current ||
      prefs.include_codex !== prevIncludeCodexRef.current;

    prevConfigDirsRef.current = newDirsJson;
    prevCodexDirsRef.current = newCodexDirsJson;
    prevIncludeClaudeRef.current = prefs.include_claude;
    prevIncludeCodexRef.current = prefs.include_codex;

    invoke("set_preferences", { prefs })
      .then(() => {
        if (shouldRefreshStats) emit("stats-updated").catch(() => {});
      })
      .catch(() => {});
  }, [prefs, ready]);

  const updatePrefs = useCallback((partial: Partial<UserPreferences>) => {
    if (!ready) return; // Block updates until loaded
    setPrefs((prev) => ({ ...prev, ...partial }));
  }, [ready]);

  const refreshPrefs = useCallback(async () => {
    try {
      const p = await loadPreferencesWithKeys();
      skipNextPersist.current = true;
      prevConfigDirsRef.current = JSON.stringify(p.config_dirs);
      prevCodexDirsRef.current = JSON.stringify(p.codex_dirs);
      prevIncludeClaudeRef.current = p.include_claude;
      prevIncludeCodexRef.current = p.include_codex;
      setPrefs(p);
    } catch {
      // ignore
    }
  }, []);

  return (
    <SettingsContext.Provider value={{ prefs, updatePrefs, refreshPrefs, ready }}>
      {children}
    </SettingsContext.Provider>
  );
}

export function useSettings() {
  return useContext(SettingsContext);
}
