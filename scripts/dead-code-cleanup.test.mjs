import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const repoRoot = process.cwd();

function read(path) {
  return readFileSync(join(repoRoot, path), "utf8");
}

test("template public SVG assets are not shipped", () => {
  assert.equal(existsSync(join(repoRoot, "public", "tauri.svg")), false);
  assert.equal(existsSync(join(repoRoot, "public", "vite.svg")), false);
});

test("unused Rust SQLite dependency is removed", () => {
  assert.doesNotMatch(read("src-tauri/Cargo.toml"), /\brusqlite\b/);
  assert.doesNotMatch(read("src-tauri/Cargo.lock"), /name = "rusqlite"/);
});

test("unused frontend helpers and mirror types are removed", () => {
  assert.doesNotMatch(read("src/i18n/I18nContext.tsx"), /\bLANGUAGE_NAMES\b/);
  assert.doesNotMatch(read("src/lib/statsHelpers.ts"), /\bcomputeCacheHitRate\b/);
  assert.doesNotMatch(read("src/lib/types.ts"), /\b(rate_limits|RateLimitStatus|UsageWindow|ExtraUsage)\b/);
});

test("AccountState no longer exposes empty rate_limits fields", () => {
  assert.doesNotMatch(read("src-tauri/src/providers/types.rs"), /\brate_limits\b/);
  assert.doesNotMatch(read("src-tauri/src/providers/types.rs"), /\bstruct RateLimitStatus\b/);
  assert.doesNotMatch(read("src-tauri/src/commands.rs"), /rate_limits:\s*Vec::new\(\)/);
  assert.doesNotMatch(read("src-tauri/src/providers/codex.rs"), /rate_limits:\s*Vec::new\(\)/);
});

test("unused i18n keys are removed from all locales", () => {
  const unusedKeys = [
    "usageAlert.title",
    "usageAlert.stale",
    "usageAlert.resetsIn",
    "usageAlert.resetsNow",
    "usageAlert.refresh",
    "usageAlert.refreshing",
    "limits.rateLimits",
    "limits.empty.rateLimits",
  ];

  for (const locale of ["de", "en", "es", "fr", "it", "ja", "ko", "tr", "zh-CN", "zh-TW"]) {
    const translations = JSON.parse(read(`src/i18n/locales/${locale}.json`));
    for (const key of unusedKeys) {
      assert.equal(Object.hasOwn(translations, key), false, `${locale} still has ${key}`);
    }
  }
});

test("receipt model aggregation uses the shared stats helper", () => {
  const source = read("src/components/receipt/Receipt.tsx");
  assert.match(source, /\baggregateModelTokensFromDaily\b/);
  assert.doesNotMatch(source, /new Map<string,\s*\{\s*tokens:\s*number\s*\}>/);
});
