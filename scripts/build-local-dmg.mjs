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

const localConfig = JSON.stringify({
  bundle: {
    createUpdaterArtifacts: false,
  },
});

run("npm", ["run", "tauri", "--", "build", "--bundles", "app", "--config", localConfig]);
run(process.execPath, ["scripts/package-macos-dmg.mjs"]);
