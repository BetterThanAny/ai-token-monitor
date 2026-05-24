#!/usr/bin/env node
import { cp, mkdir, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";

const [artifactDir = "release-artifacts", uploadDir = "release-upload"] = process.argv.slice(2);

const releaseTag = process.env.RELEASE_TAG;
if (!releaseTag) {
  throw new Error("RELEASE_TAG is required");
}

const repository = process.env.GITHUB_REPOSITORY || "BetterThanAny/ai-token-monitor";
const serverUrl = process.env.GITHUB_SERVER_URL || "https://github.com";
const releaseVersion = (process.env.RELEASE_VERSION || releaseTag).replace(/^v/, "");
const releaseNotes = process.env.RELEASE_NOTES?.trim();

function normalizeAssetName(name) {
  return name.trim().replace(/\s+/g, ".");
}

function assetUrl(name) {
  return `${serverUrl}/${repository}/releases/download/${releaseTag}/${encodeURIComponent(name)}`;
}

async function walkFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const entryPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await walkFiles(entryPath)));
    } else if (entry.isFile()) {
      files.push(entryPath);
    }
  }

  return files;
}

await rm(uploadDir, { force: true, recursive: true });
await mkdir(uploadDir, { recursive: true });

const sourceFiles = (await walkFiles(artifactDir)).filter(
  (sourcePath) => basename(sourcePath) !== "latest.json",
);
if (sourceFiles.length === 0) {
  throw new Error(`No release artifacts found in ${artifactDir}`);
}

const uploadedFiles = new Map();
for (const sourcePath of sourceFiles) {
  const assetName = normalizeAssetName(basename(sourcePath));
  const targetPath = join(uploadDir, assetName);

  if (uploadedFiles.has(assetName)) {
    throw new Error(`Duplicate release asset after normalization: ${assetName}`);
  }

  await cp(sourcePath, targetPath);
  uploadedFiles.set(assetName, targetPath);
}

async function readSignature(assetName) {
  const signaturePath = uploadedFiles.get(`${assetName}.sig`);
  if (!signaturePath) {
    throw new Error(`Missing updater signature for ${assetName}`);
  }
  return (await readFile(signaturePath, "utf8")).trim();
}

async function addPlatform(platforms, key, assetName) {
  if (!uploadedFiles.has(assetName)) {
    throw new Error(`Missing updater archive for ${key}: ${assetName}`);
  }

  platforms[key] = {
    signature: await readSignature(assetName),
    url: assetUrl(assetName),
  };
}

const assetNames = [...uploadedFiles.keys()].sort();
const platforms = {};

const macArchive = assetNames.find((name) => name.endsWith(".app.tar.gz"));
if (macArchive) {
  await addPlatform(platforms, "darwin-aarch64-app", macArchive);
  await addPlatform(platforms, "darwin-aarch64", macArchive);
}

const windowsNsis = assetNames.find((name) => name.endsWith("_x64-setup.exe"));
if (windowsNsis) {
  await addPlatform(platforms, "windows-x86_64-nsis", windowsNsis);
  await addPlatform(platforms, "windows-x86_64", windowsNsis);
}

const windowsMsi = assetNames.find((name) => name.endsWith("_x64_en-US.msi"));
if (windowsMsi) {
  await addPlatform(platforms, "windows-x86_64-msi", windowsMsi);
}

if (Object.keys(platforms).length === 0) {
  throw new Error("No updater platforms could be generated from release artifacts");
}

const manifest = {
  version: releaseVersion,
  notes: releaseNotes || undefined,
  pub_date: new Date().toISOString(),
  platforms,
};

await writeFile(join(uploadDir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

for (const [assetName, path] of uploadedFiles) {
  const fileStat = await stat(path);
  console.log(`Prepared ${assetName} (${fileStat.size} bytes)`);
}
console.log(`Prepared latest.json with platforms: ${Object.keys(platforms).join(", ")}`);
