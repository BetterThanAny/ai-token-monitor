import { useEffect, useRef, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import type { RealtimeChannel } from "@supabase/supabase-js";

/**
 * Tracks unread chat message count while the chat tab is not active.
 * Listens to Supabase Realtime INSERT events on chat_messages.
 */
export function useUnreadChat(isChatActive: boolean, userId: string | null) {
  const [unreadCount, setUnreadCount] = useState(0);
  const channelRef = useRef<RealtimeChannel | null>(null);

  // Reset when chat tab becomes active
  useEffect(() => {
    if (isChatActive) {
      setUnreadCount(0);
    }
  }, [isChatActive]);

  // Subscribe to new messages
  useEffect(() => {
    if (!supabase || !userId) return;

    const channel = supabase
      .channel("chat_unread_badge")
      .on("postgres_changes", {
        event: "INSERT",
        schema: "public",
        table: "chat_messages",
      }, (payload) => {
        const row = payload.new as { user_id: string };
        // Don't count own messages
        if (row.user_id === userId) return;
        setUnreadCount((prev) => prev + 1);
      })
      .subscribe();

    channelRef.current = channel;

    return () => {
      channel.unsubscribe();
      channelRef.current = null;
    };
  }, [userId]);

  // When chat is active, always report 0 (already reset above)
  const resetUnread = useCallback(() => setUnreadCount(0), []);

  return { unreadCount: isChatActive ? 0 : unreadCount, resetUnread };
}
