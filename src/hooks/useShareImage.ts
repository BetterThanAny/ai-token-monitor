import { useState, useCallback, useRef, useEffect, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import html2canvas from "html2canvas";

export function useShareImage(ref: RefObject<HTMLElement | null>) {
  const [capturing, setCapturing] = useState(false);
  const [captured, setCaptured] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const capturingRef = useRef(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    };
  }, []);

  const capture = useCallback(async () => {
    if (!ref.current || capturingRef.current) return;
    capturingRef.current = true;
    setCapturing(true);
    try {
      const canvas = await html2canvas(ref.current, {
        backgroundColor: null,
        scale: 2,
        useCORS: true,
        logging: false,
      });
      const blob = await new Promise<Blob | null>((resolve) =>
        canvas.toBlob(resolve, "image/png")
      );
      if (blob) {
        const arrayBuffer = await blob.arrayBuffer();
        const uint8Array = Array.from(new Uint8Array(arrayBuffer));
        await invoke("copy_png_to_clipboard", { pngData: uint8Array });
        setCaptured(true);
        if (timerRef.current) clearTimeout(timerRef.current);
        timerRef.current = setTimeout(() => setCaptured(false), 2000);
      }
    } catch (e) {
      console.error("Share image capture failed:", e);
    } finally {
      capturingRef.current = false;
      setCapturing(false);
    }
  }, [ref]);

  const savePng = useCallback(async (defaultName = "ai-token-monitor-badge.png") => {
    if (!ref.current || capturingRef.current) return;
    capturingRef.current = true;
    setSaving(true);
    try {
      const canvas = await html2canvas(ref.current, {
        backgroundColor: null,
        scale: 2,
        useCORS: true,
        logging: false,
      });
      const blob = await new Promise<Blob | null>((resolve) =>
        canvas.toBlob(resolve, "image/png")
      );
      if (!blob) return;

      const path = await save({
        defaultPath: defaultName,
        filters: [{ name: "PNG Image", extensions: ["png"] }],
      });
      if (!path) return;

      const arrayBuffer = await blob.arrayBuffer();
      const uint8Array = Array.from(new Uint8Array(arrayBuffer));
      await invoke("save_png_to_file", { pngData: uint8Array, path });
      setSaved(true);
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      saveTimerRef.current = setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error("Save PNG failed:", e);
    } finally {
      capturingRef.current = false;
      setSaving(false);
    }
  }, [ref]);

  return { capture, capturing, captured, savePng, saving, saved };
}
