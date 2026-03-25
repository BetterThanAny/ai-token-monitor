import { useState, useCallback, useRef, useEffect, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import html2canvas from "html2canvas";

export function useShareImage(ref: RefObject<HTMLElement | null>) {
  const [capturing, setCapturing] = useState(false);
  const [captured, setCaptured] = useState(false);
  const capturingRef = useRef(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
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

  return { capture, capturing, captured };
}
