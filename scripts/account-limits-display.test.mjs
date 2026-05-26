import assert from "node:assert/strict";
import { readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import ts from "typescript";

async function importTsModule(path) {
  const source = await readFile(path, "utf8");
  const transpiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2020,
      importsNotUsedAsValues: ts.ImportsNotUsedAsValues.Remove,
    },
  });
  const tmpPath = join(tmpdir(), `account-limits-display-${Date.now()}-${Math.random()}.mjs`);
  await writeFile(tmpPath, transpiled.outputText);
  try {
    return await import(pathToFileURL(tmpPath).href);
  } finally {
    await rm(tmpPath, { force: true });
  }
}

const {
  displayLimitWindowName,
  formatResetCountdown,
  getWeeklyResetSummaries,
} = await importTsModule("src/lib/accountLimitsDisplay.ts");

const labels = {
  unavailable: "暂无周额度重置时间",
  resetting: "重置中...",
  day: "天",
  hour: "小时",
  minute: "分",
};

test("displayLimitWindowName removes duplicated provider names and Codex source labels", () => {
  assert.equal(displayLimitWindowName("claude", { name: "Claude 5h" }), "5h");
  assert.equal(displayLimitWindowName("claude", { name: "Claude 7d" }), "7d");
  assert.equal(
    displayLimitWindowName("codex", { name: "Primary Usage (5h)", window_minutes: 300 }),
    "5h",
  );
  assert.equal(
    displayLimitWindowName("codex", { name: "Secondary Usage (7d)", window_minutes: 10080 }),
    "7d",
  );
});

test("getWeeklyResetSummaries selects Claude total 7d and Codex explicit 7d windows", () => {
  const summaries = getWeeklyResetSummaries([
    {
      provider: "claude",
      limit_windows: [
        { name: "Claude 5h", resets_at: "2026-05-26T06:30:00Z" },
        { name: "Claude 7d", resets_at: "2026-05-30T07:00:00Z" },
        { name: "Claude Sonnet 7d", resets_at: "2026-05-31T07:00:00Z" },
      ],
    },
    {
      provider: "codex",
      limit_windows: [
        { name: "Primary Usage (5h)", window_minutes: 300, resets_at: "2026-05-26T06:34:00Z" },
        { name: "Secondary Usage (7d)", window_minutes: 10080, resets_at: "2026-05-31T01:52:00Z" },
      ],
    },
  ]);

  assert.deepEqual(summaries, [
    { provider: "claude", resetsAt: "2026-05-30T07:00:00Z" },
    { provider: "codex", resetsAt: "2026-05-31T01:52:00Z" },
  ]);
});

test("getWeeklyResetSummaries does not guess a Codex weekly reset from non-7d windows", () => {
  const summaries = getWeeklyResetSummaries([
    {
      provider: "codex",
      limit_windows: [
        { name: "Primary Usage (5h)", window_minutes: 300, resets_at: "2026-05-26T06:34:00Z" },
        { name: "Long Usage (5d)", window_minutes: 7200, resets_at: "2026-05-30T06:34:00Z" },
      ],
    },
  ]);

  assert.deepEqual(summaries, [{ provider: "codex", resetsAt: null }]);
});

test("formatResetCountdown renders minute-level countdowns and empty states", () => {
  const now = new Date("2026-05-25T00:00:00Z");

  assert.equal(
    formatResetCountdown("2026-05-31T03:12:00Z", now, labels),
    "6天 03小时 12分",
  );
  assert.equal(formatResetCountdown(null, now, labels), "暂无周额度重置时间");
  assert.equal(formatResetCountdown("not-a-date", now, labels), "暂无周额度重置时间");
  assert.equal(formatResetCountdown("2026-05-25T00:00:00Z", now, labels), "重置中...");
});
