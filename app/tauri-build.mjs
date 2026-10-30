// Runs `tauri build` for a test build: picks this build's RC number first (src-tauri/build.json, read by the app so
// its updater can tell builds of the same version apart) and signs the update files with the key kept on this PC
// (%USERPROFILE%\.tauri\alogs-updater.key, never in the project). clean-installers.mjs then names the installer
// aLogs_<version>_RC<n>_x64-setup.exe.
import { execSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { signUpdate, verifyUpdate } from "./sign-update.mjs";

const version = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const counter = join("src-tauri", "target", "release", "bundle", `rc-${version}.txt`);   // the RC numbers of a version keep counting up
const rc = (existsSync(counter) ? Number(readFileSync(counter, "utf8")) || 0 : 0) + 1;
// what players see: the beta this build would be (B1, B2, ...), one past the last one published (betas.json); the RC
// number stays underneath, counting every build, for the updater
const beta = (JSON.parse(readFileSync("betas.json", "utf8"))[version] ?? 0) + 1;
writeFileSync("src-tauri/build.json", JSON.stringify({ version, rc, beta }) + "\n");
console.log(`building ${version} B${beta} (RC${rc})`);

const key = join(homedir(), ".tauri", "alogs-updater.key");
if (!existsSync(key)) throw new Error(`No update-signing key at ${key}: run npx tauri signer generate --ci -p "" -w "${key}"`);
execSync("npx tauri build", {
  stdio: "inherit",
  env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: readFileSync(key, "utf8"), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "" },
});

// The feeds announce this build as "<version>+rc.<n>" and the updater only installs an update signed for exactly
// the announced version, so the installer is signed again with that version (sign-update.mjs), then checked.
const installer = join("src-tauri", "target", "release", "bundle", "nsis", `aLogs_${version}_x64-setup.exe`);
signUpdate(installer, `${version}+rc.${rc}`, key);
const pubkey = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).plugins.updater.pubkey;
console.log(`update signature: ${verifyUpdate(installer, pubkey).replace(/\t/gu, "  ")}`);
