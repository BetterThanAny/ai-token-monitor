import { createClient } from "@supabase/supabase-js";

// Public anon key — safe to expose. Security is enforced by RLS policies.
const SUPABASE_URL = "https://giunmtxxvapcgrpxjopq.supabase.co";
const SUPABASE_ANON_KEY = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImdpdW5tdHh4dmFwY2dycHhqb3BxIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NzQwNTY1NTgsImV4cCI6MjA4OTYzMjU1OH0.Hr_xtU1FGUrlNjWS8g4KeiYQWt0vC3bd16VVlAZdldk";

export const supabase = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
  auth: {
    flowType: 'pkce',
  },
});
