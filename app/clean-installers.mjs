// After a build: names this version's new installer (and its update signature) as the test build tauri-build.mjs
// numbered (aLogs_0.1.28_RC3_x64-setup.exe + .sig) and deletes every other installer in the workspace's
// target/release/bundle/nsis (paths.mjs), so only the newest one is left.
// The release (release.mjs) and betas (share.mjs) upload the newest RC under their own download names.
import { existsSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import * as paths from "./paths.mjs";

const version = paths.version();
const dir = paths.nsis;
const built = join(dir, `aLogs_${version}_x64-setup.exe`);
let keep = null;
if (existsSync(built)) {
  const info = existsSync(paths.buildJson) ? JSON.parse(readFileSync(paths.buildJson, "utf8")) : {};
  const rc = info.version === version && info.rc ? info.rc : paths.lastRc(version) + 1;
  writeFileSync(paths.counter(version), String(rc));        // the RC numbers of a version keep counting up
  keep = `aLogs_${version}_RC${rc}_x64-setup.exe`;
  renameSync(built, join(dir, keep));
  if (existsSync(built + ".sig")) renameSync(built + ".sig", join(dir, keep + ".sig"));
  console.log(`test build: ${keep}`);
}
for (const name of existsSync(dir) ? readdirSync(dir) : []) {
  const installer = name.endsWith("-setup.exe") || name.endsWith("-setup.exe.sig");
  if (installer && name !== keep && name !== `${keep}.sig` && (keep || !name.includes(`_${version}_`))) {
    rmSync(join(dir, name));
    console.log(`removed old installer ${name}`);
  }
}
