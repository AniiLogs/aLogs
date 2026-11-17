// Runs `tauri build` of the Rust app (crates/alogs-app) for a test build: picks this build's RC number first
// (crates/alogs-app/build.json, read by the app so its updater can tell builds of the same version apart) and signs the
// update files with the key kept on this PC (%USERPROFILE%\.tauri\alogs-updater.key, never in the project).
// resources.mjs has laid out what the installer carries before this; clean-installers.mjs then names the installer
// aLogs_<version>_RC<n>_x64-setup.exe. Everything lands in the workspace's target/release (paths.mjs).
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { homedir } from "node:os";
import { join } from "node:path";
import * as paths from "./paths.mjs";
import { announcedRc, nextRc, recordedRc } from "./rc-number.mjs";
import { signUpdate, verifyUpdate } from "./sign-update.mjs";

const version = paths.version();
const betas = JSON.parse(readFileSync(join(paths.appDir, "betas.json"), "utf8"));
// the RC numbers of a version keep counting up across both PCs that build (rc-number.mjs): one past the highest of
// this PC's counters, the RC betas.json records as published, and the RC the beta feed announces
const local = paths.lastRc(version);
const recorded = recordedRc(betas, version);
const feed = await announcedRc(version);
if (feed.error) console.warn(`beta feed not reachable (${feed.error}): the RC comes from this PC's counters and betas.json only`);
const rc = nextRc({ local, recorded, feed: feed.rc });
console.log(`RC${rc}: one past this PC's last build (RC${local}), betas.json (RC${recorded}) and the beta feed (${feed.error ? "not read" : feed.rc == null ? `nothing for ${version}` : `RC${feed.rc}`})`);
// what players see: the beta this build would be (B1, B2, ...), one past the last one published (betas.json); the RC
// number stays underneath, counting every build, for the updater. A release build (npm run build:release) is no beta:
// it shows as plain "0.1.28".
const release = process.argv.includes("--release");
const beta = release ? 0 : (betas[version] ?? 0) + 1;

for (const file of [join("meter", "alogs-meter.exe"), join("meter", "ocr-helper.exe"), join("sanctumhelper", "SanctumHelper.exe")]) {
  if (!existsSync(join(paths.crate, "resources", file))) throw new Error(`crates/alogs-app/resources/${file} is missing: run npm run build:rc (it lays them out with resources.mjs).`);
}
const key = join(homedir(), ".tauri", "alogs-updater.key");
if (!existsSync(key)) throw new Error(`No update-signing key at ${key}: run npx tauri signer generate --ci -p "" -w "${key}"`);

writeFileSync(paths.buildJson, JSON.stringify({ version, rc, beta, ...(release ? { release } : {}) }) + "\n");
console.log(`building ${version} ${release ? "release" : `B${beta}`} (RC${rc})`);
// this folder's Tauri CLI (package.json pins it), run in the app's own folder so it builds that app
const cli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
execFileSync(process.execPath, [cli, "build"], {
  cwd: paths.crate,
  stdio: "inherit",
  env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: readFileSync(key, "utf8"), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "" },
});

// The map of this build's functions, kept by RC number: a "window: not answering" line in a player's app.log gives
// addresses in alogs-app.exe, and only this build's map says which functions those are. Never shipped.
if (existsSync(paths.map)) {
  mkdirSync(join(paths.bundle, "maps"), { recursive: true });
  copyFileSync(paths.map, join(paths.bundle, "maps", `aLogs_${version}_RC${rc}.map`));
  console.log(`map: target/release/bundle/maps/aLogs_${version}_RC${rc}.map`);
} else {
  console.warn(`no ${paths.map}: this build's map wasn't written (crates/alogs-app/build.rs)`);
}
// The feeds announce this build as "<version>+rc.<n>" and the updater only installs an update signed for exactly
// the announced version, so the installer is signed again with that version (sign-update.mjs), then checked.
const installer = join(paths.nsis, `aLogs_${version}_x64-setup.exe`);
signUpdate(installer, `${version}+rc.${rc}`, key);
const pubkey = JSON.parse(readFileSync(paths.conf, "utf8")).plugins.updater.pubkey;
console.log(`update signature: ${verifyUpdate(installer, pubkey).replace(/\t/gu, "  ")}`);
