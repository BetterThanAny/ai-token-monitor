import { createContext, useContext, useEffect, useState, useCallback, useRef } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { UserPreferences } from "../lib/types";

interface SettingsContextType {
  prefs: UserPreferences;
  updatePrefs: (partial: Partial<UserPreferences>) => void;
}

const defaultPrefs: UserPreferences = {
  number_format: "compact",
  show_tray_cost: true,
};

const SettingsContext = createContext<SettingsContextType>({
  prefs: defaultPrefs,
  updatePrefs: () => {},
});

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<UserPreferences>(defaultPrefs);
  const initialized = useRef(false);

  useEffect(() => {
    invoke<UserPreferences>("get_preferences").then((p) => {
      setPrefs(p);
      initialized.current = true;
    }).catch(() => {
      initialized.current = true;
    });
  }, []);

  // Persist to disk when prefs change (skip initial load)
  useEffect(() => {
    if (!initialized.current) return;
    invoke("set_preferences", { prefs }).catch(() => {});
  }, [prefs]);

  const updatePrefs = useCallback((partial: Partial<UserPreferences>) => {
    setPrefs((prev) => ({ ...prev, ...partial }));
  }, []);

  return (
    <SettingsContext.Provider value={{ prefs, updatePrefs }}>
      {children}
    </SettingsContext.Provider>
  );
}

export function useSettings() {
  return useContext(SettingsContext);
}
