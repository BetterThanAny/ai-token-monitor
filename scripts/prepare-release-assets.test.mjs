import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const scriptPath = join(repoRoot, "scripts", "prepare-release-assets.mjs");

test("prepares one updater manifest from multi-platform artifacts", async () => {
  const tmp = await mkdtemp(join(tmpdir(), "atm-release-assets-"));
  try {
    const artifactDir = join(tmp, "artifacts");
    const uploadDir = join(tmp, "upload");
    await mkdir(join(artifactDir, "mac"), { recursive: true });
    await mkdir(join(artifactDir, "win"), { recursive: true });

    await writeFile(join(artifactDir, "mac", "latest.json"), "{}\n");
    await writeFile(join(artifactDir, "win", "latest.json"), "{}\n");
    await writeFile(join(artifactDir, "mac", "AI Token Monitor.app.tar.gz"), "archive");
    await writeFile(join(artifactDir, "mac", "AI Token Monitor.app.tar.gz.sig"), "mac-sig\n");
    await writeFile(join(artifactDir, "win", "AI Token Monitor_0.19.14_x64-setup.exe"), "setup");
    await writeFile(join(artifactDir, "win", "AI Token Monitor_0.19.14_x64-setup.exe.sig"), "win-sig\n");

    const result = spawnSync(process.execPath, [scriptPath, artifactDir, uploadDir], {
      cwd: repoRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        RELEASE_TAG: "v0.19.14",
        RELEASE_VERSION: "v0.19.14",
      },
    });

    assert.equal(result.status, 0, result.stderr || result.stdout);
    const manifest = JSON.parse(await readFile(join(uploadDir, "latest.json"), "utf8"));
    assert.equal(manifest.version, "0.19.14");
    assert.deepEqual(Object.keys(manifest.platforms).sort(), [
      "darwin-aarch64",
      "darwin-aarch64-app",
      "windows-x86_64",
      "windows-x86_64-nsis",
    ]);
  } finally {
    await rm(tmp, { recursive: true, force: true });
  }
});
