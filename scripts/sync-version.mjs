import { readFileSync, writeFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;

// package-lock.json
const lockPath = join(root, 'package-lock.json');
const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
lock.version = version;
if (lock.packages?.['']) {
  lock.packages[''].version = version;
}
writeFileSync(lockPath, JSON.stringify(lock, null, 2) + '\n');

// tauri.conf.json
const tauriConfPath = join(root, 'src-tauri/tauri.conf.json');
const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
tauriConf.version = version;
writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n');

// Cargo.toml
const cargoPath = join(root, 'src-tauri/Cargo.toml');
const cargo = readFileSync(cargoPath, 'utf8');
writeFileSync(cargoPath, cargo.replace(/^version = ".*"/m, `version = "${version}"`));

console.log(`Synced version to ${version}`);
