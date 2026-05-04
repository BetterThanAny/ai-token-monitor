import { useState, useCallback, useRef, useEffect, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

export type ShareImageError = {
  action: "copy" | "save";
  message: string;
};

function formatShareImageError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function canCopyPngToClipboard(): boolean {
  const platform = `${navigator.userAgent} ${navigator.platform}`.toLowerCase();
  return platform.includes("mac");
}

export function useShareImage(ref: RefObject<HTMLElement | null>) {
  const [capturing, setCapturing] = useState(false);
  const [captured, setCaptured] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<ShareImageError | null>(null);
  const capturingRef = useRef(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const errorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (errorTimerRef.current) clearTimeout(errorTimerRef.current);
    };
  }, []);

  const clearError = useCallback(() => {
    if (errorTimerRef.current) {
      clearTimeout(errorTimerRef.current);
      errorTimerRef.current = null;
    }
    setError(null);
  }, []);

  const showError = useCallback((action: ShareImageError["action"], err: unknown) => {
    if (errorTimerRef.current) clearTimeout(errorTimerRef.current);
    const nextError = { action, message: formatShareImageError(err) };
    setError(nextError);
    const timer = setTimeout(() => {
      setError((current) =>
        current &&
        current.action === nextError.action &&
        current.message === nextError.message
          ? null
          : current
      );
      if (errorTimerRef.current === timer) {
        errorTimerRef.current = null;
      }
    }, 4000);
    errorTimerRef.current = timer;
  }, []);

  const renderPngData = useCallback(async () => {
    if (!ref.current) {
      throw new Error("No element to capture");
    }

    const { default: html2canvas } = await import("html2canvas");
    const canvas = await html2canvas(ref.current, {
      backgroundColor: null,
      scale: 2,
      useCORS: true,
      logging: false,
    });
    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png")
    );
    if (!blob) {
      throw new Error("PNG rendering produced no data");
    }

    const arrayBuffer = await blob.arrayBuffer();
    return Array.from(new Uint8Array(arrayBuffer));
  }, [ref]);

  const savePng = useCallback(async (defaultName = "ai-token-monitor-badge.png") => {
    if (!ref.current || capturingRef.current) return;
    capturingRef.current = true;
    setSaving(true);
    clearError();
    try {
      const uint8Array = await renderPngData();
      const path = await save({
        defaultPath: defaultName,
        filters: [{ name: "PNG Image", extensions: ["png"] }],
      });
      if (!path) return;

      await invoke("save_png_to_file", { pngData: uint8Array, path });
      setSaved(true);
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      saveTimerRef.current = setTimeout(() => {
        setSaved(false);
        saveTimerRef.current = null;
      }, 2000);
    } catch (e) {
      console.error("Save PNG failed:", e);
      showError("save", e);
    } finally {
      capturingRef.current = false;
      setSaving(false);
    }
  }, [ref, clearError, renderPngData, showError]);

  const capture = useCallback(async () => {
    if (!ref.current || capturingRef.current) return;
    if (!canCopyPngToClipboard()) {
      await savePng();
      return;
    }

    capturingRef.current = true;
    setCapturing(true);
    clearError();
    try {
      const uint8Array = await renderPngData();
      await invoke("copy_png_to_clipboard", { pngData: uint8Array });
      setCaptured(true);
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => {
        setCaptured(false);
        timerRef.current = null;
      }, 2000);
    } catch (e) {
      console.error("Share image capture failed:", e);
      showError("copy", e);
    } finally {
      capturingRef.current = false;
      setCapturing(false);
    }
  }, [ref, clearError, renderPngData, savePng, showError]);

  return {
    capture,
    capturing,
    captured,
    savePng,
    saving,
    saved,
    error,
    clearError,
    canCopyImage: canCopyPngToClipboard(),
  };
}
