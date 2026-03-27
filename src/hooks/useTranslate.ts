import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export function useTranslate(targetLanguage: string) {
  const [translations, setTranslations] = useState<Record<string, string>>({});
  const [translating, setTranslating] = useState<Set<string>>(new Set());

  const translateText = useCallback(async (text: string, targetLang: string, sourceLang?: string): Promise<string | null> => {
    try {
      return await invoke<string>("translate_text", {
        text,
        targetLanguage: targetLang,
        sourceLanguage: sourceLang ?? null,
      });
    } catch (err) {
      console.error("Translation failed:", err);
      return null;
    }
  }, []);

  const translateReply = useCallback(async (text: string, originalMessage: string): Promise<string | null> => {
    try {
      return await invoke<string>("translate_reply", {
        text,
        originalMessage,
      });
    } catch (err) {
      console.error("Reply translation failed:", err);
      return null;
    }
  }, []);

  const translate = useCallback(async (messageId: string, text: string): Promise<string | null> => {
    setTranslating((prev) => new Set(prev).add(messageId));
    try {
      const result = await translateText(text, targetLanguage);
      if (result) {
        setTranslations((prev) => ({ ...prev, [messageId]: result }));
      }
      return result;
    } finally {
      setTranslating((prev) => {
        const next = new Set(prev);
        next.delete(messageId);
        return next;
      });
    }
  }, [targetLanguage, translateText]);

  return { translations, translating, translate, translateText, translateReply };
}
