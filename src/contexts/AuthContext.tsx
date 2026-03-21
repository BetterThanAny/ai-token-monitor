import { createContext, useContext, useEffect, useState, useCallback } from "react";
import type { ReactNode } from "react";
import { supabase } from "../lib/supabase";
import type { User } from "@supabase/supabase-js";

interface AuthContextType {
  user: User | null;
  profile: { nickname: string; avatar_url: string | null } | null;
  loading: boolean;
  available: boolean;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
}

const AuthContext = createContext<AuthContextType>({
  user: null,
  profile: null,
  loading: true,
  available: false,
  signIn: async () => {},
  signOut: async () => {},
});

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [profile, setProfile] = useState<{ nickname: string; avatar_url: string | null } | null>(null);
  const [loading, setLoading] = useState(true);
  const available = supabase !== null;

  useEffect(() => {
    if (!supabase) {
      setLoading(false);
      return;
    }

    supabase.auth.getSession().then(({ data: { session } }) => {
      const u = session?.user ?? null;
      setUser(u);
      setLoading(false);
      if (u) upsertProfile(u);
    });

    const { data: { subscription } } = supabase.auth.onAuthStateChange((_event, session) => {
      const u = session?.user ?? null;
      setUser(u);
      if (u) {
        upsertProfile(u);
      } else {
        setProfile(null);
      }
    });

    return () => subscription.unsubscribe();
  }, []);

  const upsertProfile = useCallback(async (u: User) => {
    if (!supabase) return;

    const nickname = u.user_metadata?.user_name
      || u.user_metadata?.preferred_username
      || u.email?.split("@")[0]
      || "Anonymous";
    const avatar_url = u.user_metadata?.avatar_url || null;

    await supabase.from("profiles").upsert({
      id: u.id,
      nickname,
      avatar_url,
    }, { onConflict: "id" });

    setProfile({ nickname, avatar_url });
  }, []);

  const signIn = useCallback(async () => {
    if (!supabase) return;
    setLoading(true);
    await supabase.auth.signInWithOAuth({
      provider: "github",
      options: { redirectTo: window.location.origin },
    });
  }, []);

  const signOut = useCallback(async () => {
    if (!supabase) return;
    setLoading(true);
    await supabase.auth.signOut();
    setUser(null);
    setProfile(null);
    setLoading(false);
  }, []);

  return (
    <AuthContext.Provider value={{ user, profile, loading, available, signIn, signOut }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
