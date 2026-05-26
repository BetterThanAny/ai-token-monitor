import { useState, useEffect, useRef, useCallback } from "react";
import { check as checkForUpdates, type CheckOptions, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke } from "@tauri-apps/api/core";

export interface UpdaterState {
  updateAvailable: boolean;
  version: string;
  downloading: boolean;
  downloaded: boolean;
  progress: number;
  error: string | null;
  restartFailed: boolean;
  download: () => void;
  install: () => void;
}

const UPDATE_CHECK_TIMEOUT_MS = 15_000;

async function getUpdateCheckOptions(): Promise<CheckOptions> {
  const options: CheckOptions = { timeout: UPDATE_CHECK_TIMEOUT_MS };

  try {
    const proxy = await invoke<string | null>("get_update_proxy");
    if (proxy) {
      options.proxy = proxy;
    }
  } catch (e) {
    console.warn("[updater] proxy detection failed:", e);
  }

  return options;
}

export function useUpdater(): UpdaterState {
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [version, setVersion] = useState("");
  const [downloading, setDownloading] = useState(false);
  const [downloaded, setDownloaded] = useState(false);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [restartFailed, setRestartFailed] = useState(false);
  const updateRef = useRef<Update | null>(null);

  const checkForUpdate = useCallback(async (cancelled?: () => boolean) => {
    try {
      const update = await checkForUpdates(await getUpdateCheckOptions());
      if (cancelled?.()) return;
      setError(null);
      if (update) {
        updateRef.current = update;
        setVersion(update.version);
        setUpdateAvailable(true);
      } else {
        updateRef.current = null;
        setVersion("");
        setUpdateAvailable(false);
      }
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      console.warn("[updater] check failed:", e);
      if (!cancelled?.()) {
        setError(message);
      }
    }
  }, []);

  useEffect(() => {
    let cancelled = false;

    checkForUpdate(() => cancelled);

    const interval = setInterval(() => {
      checkForUpdate(() => cancelled);
    }, 30 * 60 * 1000);

    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [checkForUpdate]);

  const download = useCallback(async () => {
    const update = updateRef.current;
    if (!update || downloading) return;

    setDownloading(true);
    setError(null);
    setProgress(0);

    let contentLength = 0;
    let downloaded = 0;

    try {
      // downloadAndInstall() downloads AND installs the update in one call.
      // After it resolves, the user clicks "Restart" which calls relaunch().
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          contentLength = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (contentLength > 0) {
            setProgress(Math.round((downloaded / contentLength) * 100));
          }
        } else if (event.event === "Finished") {
          setProgress(100);
        }
      });
      setDownloaded(true);
      setDownloading(false);
    } catch (e) {
      console.error("[updater] download failed:", e);
      setError(e instanceof Error ? e.message : String(e));
      setDownloading(false);
    }
  }, [downloading]);

  const install = useCallback(async () => {
    setRestartFailed(false);
    try {
      // Use custom restart command to avoid single-instance plugin blocking relaunch
      await invoke("restart_app");
    } catch {
      console.warn("[updater] restart_app failed, falling back to relaunch");
      try {
        await relaunch();
      } catch (e) {
        console.error("[updater] relaunch also failed:", e);
        setRestartFailed(true);
      }
    }
  }, []);

  return {
    updateAvailable,
    version,
    downloading,
    downloaded,
    progress,
    error,
    restartFailed,
    download,
    install,
  };
}
