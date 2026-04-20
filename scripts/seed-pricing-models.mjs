// scripts/seed-pricing-models.mjs
// Reads src-tauri/pricing.json and emits a Supabase migration that seeds
// pricing_models. Run as `node scripts/seed-pricing-models.mjs > OUT.sql`
// or with --check to verify the latest seed migration matches.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pricingPath = join(__dirname, "..", "src-tauri", "pricing.json");
const pricing = JSON.parse(readFileSync(pricingPath, "utf8"));

const escape = (s) => String(s).replace(/'/g, "''");

function emit(provider, models) {
  const rows = [];
  models.forEach((m, i) => {
    const input = m.input ?? 0;
    const output = m.output ?? 0;
    const cacheRead = m.cache_read ?? 0;
    const cacheWrite = m.cache_write ?? 0;
    const cacheWrite1h = m.cache_write_1h ?? cacheWrite;
    rows.push(
      `  ('${provider}', '${escape(m.match)}', '${escape(m.label ?? m.match)}', ${input}, ${output}, ${cacheRead}, ${cacheWrite}, ${cacheWrite1h}, ${i})`,
    );
  });
  return rows;
}

const allRows = [];
for (const provider of ["claude", "codex", "opencode", "kimi", "glm"]) {
  const cfg = pricing[provider];
  if (!cfg?.models) continue;
  allRows.push(...emit(provider, cfg.models));
}

const sql = `-- Auto-generated from src-tauri/pricing.json v${pricing.version} (${pricing.last_updated}).
-- Regenerate via: node scripts/seed-pricing-models.mjs > supabase/migrations/<ts>_seed_pricing_models.sql

insert into public.pricing_models
  (provider, match_pattern, label, input_rate, output_rate, cache_read_rate, cache_write_rate, cache_write_1h_rate, priority)
values
${allRows.join(",\n")}
on conflict (provider, match_pattern, effective_from) do update set
  label               = excluded.label,
  input_rate          = excluded.input_rate,
  output_rate         = excluded.output_rate,
  cache_read_rate     = excluded.cache_read_rate,
  cache_write_rate    = excluded.cache_write_rate,
  cache_write_1h_rate = excluded.cache_write_1h_rate,
  priority            = excluded.priority;
`;

process.stdout.write(sql);
