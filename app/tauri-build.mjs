// Runs `tauri build` for a test build: picks this build's RC number first (src-tauri/build.json, read by the app so
// its updater can tell builds of the same version apart) and signs the update files with the key kept on this PC
// (%USERPROFILE%\.tauri\alogs-updater.key, never in the project). clean-installers.mjs then names the installer
// aLogs_<version>_RC<n>_x64-setup.exe.
import { execSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const version = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const counter = join("src-tauri", "target", "release", "bundle", `rc-${version}.txt`);   // the RC numbers of a version keep counting up
const rc = (existsSync(counter) ? Number(readFileSync(counter, "utf8")) || 0 : 0) + 1;
writeFileSync("src-tauri/build.json", JSON.stringify({ version, rc }) + "\n");
console.log(`building ${version} RC${rc}`);

const key = join(homedir(), ".tauri", "alogs-updater.key");
if (!existsSync(key)) throw new Error(`No update-signing key at ${key}: run npx tauri signer generate --ci -p "" -w "${key}"`);
execSync("npx tauri build", {
  stdio: "inherit",
  env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: readFileSync(key, "utf8"), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "" },
});
