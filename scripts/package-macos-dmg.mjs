import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function argValue(name) {
  const index = process.argv.indexOf(name);
  if (index === -1) return undefined;
  return process.argv[index + 1];
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? root,
    env: process.env,
    stdio: options.stdio ?? "inherit",
  });

  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
}

function archLabel(targetTriple) {
  if (targetTriple?.startsWith("aarch64-")) return "aarch64";
  if (targetTriple?.startsWith("x86_64-")) return "x64";
  return process.arch === "arm64" ? "aarch64" : "x64";
}

function firstNonEmpty(...values) {
  return values.find((value) => typeof value === "string" && value.trim() !== "");
}

const packageJson = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
const tauriConfig = JSON.parse(await readFile(join(root, "src-tauri", "tauri.conf.json"), "utf8"));

const targetTriple = argValue("--target") ?? process.env.TAURI_TARGET_TRIPLE;
const releaseDir = targetTriple
  ? join(root, "src-tauri", "target", targetTriple, "release")
  : join(root, "src-tauri", "target", "release");
const productName = tauriConfig.productName ?? packageJson.name;
const appName = `${productName}.app`;
const appSource = join(releaseDir, "bundle", "macos", appName);
const dmgDir = join(releaseDir, "bundle", "dmg");
const dmgPath = join(dmgDir, `${productName}_${packageJson.version}_${archLabel(targetTriple)}.dmg`);

if (process.platform !== "darwin") {
  throw new Error("macOS DMG packaging must run on macOS.");
}

if (!existsSync(appSource)) {
  throw new Error(`App bundle not found: ${appSource}`);
}

await mkdir(dmgDir, { recursive: true });

const stagingDir = await mkdtemp(join(dmgDir, ".dmg-staging-"));
const mountDir = await mkdtemp(join(tmpdir(), "ai-token-monitor-dmg-"));
const appInStaging = join(stagingDir, appName);

try {
  await rm(dmgPath, { force: true });

  run("ditto", ["--noqtn", appSource, appInStaging]);
  run("xattr", ["-cr", appInStaging]);

  await symlink("/Applications", join(stagingDir, "Applications"));

  const signingIdentity = firstNonEmpty(
    process.env.CODESIGN_IDENTITY,
    process.env.APPLE_SIGNING_IDENTITY,
    "-"
  );
  run("codesign", [
    "--force",
    "--deep",
    "--options",
    "runtime",
    "--sign",
    signingIdentity,
    appInStaging,
  ]);
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appInStaging]);

  run("hdiutil", [
    "create",
    "-volname",
    productName,
    "-srcfolder",
    stagingDir,
    "-ov",
    "-format",
    "UDZO",
    dmgPath,
  ]);
  run("hdiutil", ["verify", dmgPath]);

  const canNotarize =
    signingIdentity !== "-" &&
    firstNonEmpty(process.env.APPLE_ID) &&
    firstNonEmpty(process.env.APPLE_PASSWORD) &&
    firstNonEmpty(process.env.APPLE_TEAM_ID);
  if (canNotarize) {
    run("xcrun", [
      "notarytool",
      "submit",
      dmgPath,
      "--apple-id",
      process.env.APPLE_ID,
      "--password",
      process.env.APPLE_PASSWORD,
      "--team-id",
      process.env.APPLE_TEAM_ID,
      "--wait",
    ]);
    run("xcrun", ["stapler", "staple", dmgPath]);
  }

  run("hdiutil", ["attach", "-readonly", "-nobrowse", "-mountpoint", mountDir, dmgPath]);
  try {
    run("codesign", [
      "--verify",
      "--deep",
      "--strict",
      "--verbose=2",
      join(mountDir, appName),
    ]);
  } finally {
    spawnSync("hdiutil", ["detach", mountDir], { stdio: "inherit" });
  }

  console.log(`Packaged verified DMG: ${dmgPath}`);
} finally {
  await rm(stagingDir, { recursive: true, force: true });
  await rm(mountDir, { recursive: true, force: true });
}
