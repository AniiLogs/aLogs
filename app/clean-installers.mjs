// After a build: names this version's new installer (and its update signature) as the test build tauri-build.mjs
// numbered (aLogs_0.1.28_RC3_x64-setup.exe + .sig) and deletes every other installer in
// src-tauri/target/release/bundle/nsis, so only the newest one is left.
// The release (release.mjs) and betas (share.mjs) upload the newest RC under their own download names.
import { existsSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const version = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const dir = "src-tauri/target/release/bundle/nsis";
const built = join(dir, `aLogs_${version}_x64-setup.exe`);
const counter = join(dir, "..", `rc-${version}.txt`);      // the RC numbers of a version keep counting up
let keep = null;
if (existsSync(built)) {
  const info = existsSync("src-tauri/build.json") ? JSON.parse(readFileSync("src-tauri/build.json", "utf8")) : {};
  const rc = info.version === version && info.rc ? info.rc : (existsSync(counter) ? Number(readFileSync(counter, "utf8")) || 0 : 0) + 1;
  writeFileSync(counter, String(rc));
  keep = `aLogs_${version}_RC${rc}_x64-setup.exe`;
  renameSync(built, join(dir, keep));
  if (existsSync(built + ".sig")) renameSync(built + ".sig", join(dir, keep + ".sig"));
  console.log(`test build: ${keep}`);
}
for (const name of readdirSync(dir)) {
  const installer = name.endsWith("-setup.exe") || name.endsWith("-setup.exe.sig");
  if (installer && name !== keep && name !== `${keep}.sig` && (keep || !name.includes(`_${version}_`))) {
    rmSync(join(dir, name));
    console.log(`removed old installer ${name}`);
  }
}
