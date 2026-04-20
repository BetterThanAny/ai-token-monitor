// scripts/check-pricing-sync.mjs
// Exit non-zero if src-tauri/pricing.json has entries that are not mirrored
// in the latest seed migration for pricing_models. Run in CI.

import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pricingPath = join(__dirname, "..", "src-tauri", "pricing.json");
const migrationsDir = join(__dirname, "..", "supabase", "migrations");

const pricing = JSON.parse(readFileSync(pricingPath, "utf8"));

// Find the latest seed_pricing_models migration.
const migrations = readdirSync(migrationsDir)
  .filter((f) => /_seed_pricing_models\.sql$/.test(f))
  .sort();
if (migrations.length === 0) {
  console.error("No seed_pricing_models migration found.");
  process.exit(1);
}
const latest = readFileSync(join(migrationsDir, migrations[migrations.length - 1]), "utf8");

const missing = [];
for (const provider of ["claude", "codex", "opencode", "kimi", "glm"]) {
  const cfg = pricing[provider];
  if (!cfg?.models) continue;
  for (const m of cfg.models) {
    const needle = `'${provider}', '${m.match.replace(/'/g, "''")}'`;
    if (!latest.includes(needle)) {
      missing.push(`${provider}:${m.match}`);
    }
  }
}

if (missing.length > 0) {
  console.error("Pricing drift — the following (provider, match) pairs are in pricing.json but not in the latest seed migration:");
  for (const m of missing) console.error("  - " + m);
  console.error("\nFix: `node scripts/seed-pricing-models.mjs > supabase/migrations/<YYYYMMDDHHMMSS>_seed_pricing_models.sql` and commit.");
  process.exit(1);
}
console.log(`Pricing in sync (${missing.length === 0 ? "OK" : "DRIFT"}; ${pricing.version}).`);
