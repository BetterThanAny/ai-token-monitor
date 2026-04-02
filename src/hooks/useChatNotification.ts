import { useEffect, useRef, useCallback } from "react";
import { supabase } from "../lib/supabase";
import { getOrFetchProfile } from "../lib/profileCache";
import type { RealtimeChannel } from "@supabase/supabase-js";
import type { ProfileData } from "../lib/profileCache";

let notificationModule: typeof import("@tauri-apps/plugin-notification") | null = null;
let permissionChecked = false;
let permissionReady = false;

async function ensureNotificationPermission() {
  if (permissionChecked) return;
  permissionChecked = true;
  try {
    if (!notificationModule) {
      notificationModule = await import("@tauri-apps/plugin-notification");
    }
    let granted = await notificationModule.isPermissionGranted();
    if (!granted) {
      const result = await notificationModule.requestPermission();
      granted = result === "granted";
    }
    permissionReady = granted;
  } catch {
    // Notification not available (e.g. dev mode)
  }
}

async function fireNotification(title: string, body: string) {
  if (!permissionReady || !notificationModule) return;
  try {
    notificationModule.sendNotification({ title, body });
  } catch {
    // Silently ignore
  }
}

interface Params {
  isChatActive: boolean;
  currentNickname: string | null;
  currentUserId: string | null;
}

/**
 * Listens for new chat messages and fires OS notifications when:
 * 1. Someone @mentions the current user
 * 2. Someone replies to the current user's message
 *
 * Only fires when the chat tab is not active.
 */
export function useChatNotification({ isChatActive, currentNickname, currentUserId }: Params) {
  const channelRef = useRef<RealtimeChannel | null>(null);
  const channelCounterRef = useRef(0);
  const isChatActiveRef = useRef(isChatActive);
  isChatActiveRef.current = isChatActive;
  const currentNicknameRef = useRef(currentNickname);
  currentNicknameRef.current = currentNickname;
  const currentUserIdRef = useRef(currentUserId);
  currentUserIdRef.current = currentUserId;

  // Request notification permission once
  useEffect(() => {
    if (currentUserId) {
      ensureNotificationPermission();
    }
  }, [currentUserId]);

  const fetchProfile = useCallback(async (uid: string): Promise<ProfileData> => {
    return getOrFetchProfile(uid, async () => {
      if (!supabase) return { nickname: "Unknown", avatar_url: null };
      const { data } = await supabase
        .from("profiles")
        .select("nickname, avatar_url")
        .eq("id", uid)
        .single();
      return {
        nickname: data?.nickname ?? "Unknown",
        avatar_url: data?.avatar_url ?? null,
      };
    });
  }, []);

  const checkMention = useCallback((content: string): boolean => {
    const nick = currentNicknameRef.current;
    if (!nick) return false;
    // Match @nickname with word boundary
    const re = new RegExp(`@${nick.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![a-zA-Z0-9_-])`, "i");
    return re.test(content);
  }, []);

  const checkReplyToMe = useCallback(async (replyTo: string | null): Promise<boolean> => {
    if (!replyTo || !supabase) return false;
    const uid = currentUserIdRef.current;
    if (!uid) return false;

    const { data } = await supabase
      .from("chat_messages")
      .select("user_id")
      .eq("id", replyTo)
      .single();

    return data?.user_id === uid;
  }, []);

  const setupChannel = useCallback(() => {
    if (!supabase || !currentUserIdRef.current) return;

    const channel = supabase
      .channel(`chat_notification_${++channelCounterRef.current}`)
      .on("postgres_changes", {
        event: "INSERT",
        schema: "public",
        table: "chat_messages",
      }, async (payload) => {
        try {
          const row = payload.new as {
            id: string;
            user_id: string;
            content: string;
            reply_to?: string | null;
          };

          // Skip own messages
          if (row.user_id === currentUserIdRef.current) return;

          // Skip if chat tab is active
          if (isChatActiveRef.current) return;

          const isMention = checkMention(row.content);
          // Short-circuit: skip DB query if already a mention
          const isReply = !isMention && await checkReplyToMe(row.reply_to ?? null);

          if (!isMention && !isReply) return;

          const senderProfile = await fetchProfile(row.user_id);
          const body = row.content.length > 100
            ? row.content.slice(0, 100) + "..."
            : row.content;

          fireNotification(
            isMention ? `@${senderProfile.nickname}` : senderProfile.nickname,
            body,
          );
        } catch {
          // Silently ignore notification errors
        }
      })
      .subscribe();

    channelRef.current = channel;
  }, [fetchProfile, checkMention, checkReplyToMe]);

  // Subscribe
  useEffect(() => {
    if (!supabase || !currentUserId) return;

    setupChannel();

    return () => {
      if (channelRef.current) {
        supabase.removeChannel(channelRef.current);
        channelRef.current = null;
      }
    };
  }, [currentUserId, setupChannel]);

  // Reconnect on visibility change
  const setupChannelRef = useRef(setupChannel);
  setupChannelRef.current = setupChannel;

  useEffect(() => {
    if (!supabase || !currentUserId) return;

    const handleVisibility = () => {
      if (document.hidden) return;

      if (channelRef.current) {
        supabase.removeChannel(channelRef.current);
        channelRef.current = null;
      }
      setupChannelRef.current();
    };

    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [currentUserId]);
}
