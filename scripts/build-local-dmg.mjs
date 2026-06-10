import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    env: process.env,
    stdio: "inherit",
  });

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

// signingIdentity null skips tauri's internal codesign. On iCloud-synced
// paths the File Provider tags fresh bundles with FinderInfo xattrs that make
// codesign fail; package-macos-dmg.mjs clears those xattrs and signs instead.
const localConfig = JSON.stringify({
  bundle: {
    createUpdaterArtifacts: false,
    macOS: {
      signingIdentity: null,
    },
  },
});

run("npm", ["run", "tauri", "--", "build", "--bundles", "app", "--config", localConfig]);
run(process.execPath, ["scripts/package-macos-dmg.mjs"]);
