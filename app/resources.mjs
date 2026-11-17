// Lays out what the Rust app's installer carries, after the programs are built (npm run meter:build, sanctum:build)
// and before `tauri build` (tauri-build.mjs). All of it is git-ignored and replaced as a whole on every build:
//   crates/alogs-app/resources/meter          alogs-meter.exe, ocr-helper.exe and its model's licence, the meter's
//                                             data files (crates/alogs-meter/res) and the list of plug-ins
//                                             (crates/alogs-protocol/plugins.json, the one copy the meter and the app
//                                             also build in)
//   crates/alogs-app/resources/sanctumhelper  SanctumHelper.exe and its Assets (the game's map art: from the Sanctum
//                                             Helper checkout, SanctumHelper/ at the top of the repository, never
//                                             committed)
//   crates/alogs-app/resources/vendor         HidHide's signed driver (crates/alogs-app/vendor)
// The UI's pictures are tracked where the UI uses them (crates/alogs-app/ui/img): nothing to copy.
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";
import * as paths from "./paths.mjs";

const meterRes = join(paths.root, "crates", "alogs-meter", "res");
const pluginList = join(paths.root, "crates", "alogs-protocol", "plugins.json");
const vendorDir = join(paths.crate, "vendor");
const resources = join(paths.crate, "resources");
const ocrHelper = join(paths.appDir, "ocr-helper");
const DATA_FILES = ["boss_catalog.csv", "boss_rush_grades.csv", "boss_rush_cycles.json", "xn_templates.json", "boss_break.json"];

// Sanctum Helper's Assets: the checkout at the top of this repository, or, in a git worktree (.claude/worktrees/...),
// the one at the top of the main checkout
function sanctumAssets() {
  const rel = join("SanctumHelper", "app", "SanctumHelper", "Assets");
  const tops = [paths.root];
  try {
    const common = execFileSync("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], { cwd: paths.root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
    tops.push(join(common, ".."));
  } catch { /* not a git checkout: only the folder itself */ }
  const found = tops.map((top) => join(top, rel)).find((dir) => existsSync(join(dir, "layouts.json")));
  if (!found) {
    throw new Error(`Sanctum Helper's Assets (its map art) aren't here: check out https://github.com/AniiLogs/SanctumHelper as SanctumHelper/ at the top of the repository (${join(paths.root, "SanctumHelper")}). It is git-ignored: the game's art is never committed.`);
  }
  return found;
}

function need(file, how) {
  if (!existsSync(file)) throw new Error(`${file} is missing: ${how}`);
  return file;
}
// a folder emptied (the .gitkeep that keeps it in git stays)
function empty(dir) {
  mkdirSync(dir, { recursive: true });
  for (const name of readdirSync(dir)) if (name !== ".gitkeep") rmSync(join(dir, name), { recursive: true, force: true });
}
const copy = (from, to) => cpSync(from, to, { recursive: true });

const meterExe = need(join(paths.release, "alogs-meter.exe"), "build it with npm run meter:build");
const ocrExe = need(join(ocrHelper, "target", "release", "ocr-helper.exe"), "build it with npm run ocr:build");
const sanctumExe = need(join(paths.release, "SanctumHelper.exe"), "build it with npm run sanctum:build");
const assets = sanctumAssets();

const meter = join(resources, "meter");
empty(meter);
copy(meterExe, join(meter, "alogs-meter.exe"));
copy(ocrExe, join(meter, "ocr-helper.exe"));
copy(need(join(ocrHelper, "model", "LICENSE-model.txt"), "it comes with app/ocr-helper"), join(meter, "LICENSE-model.txt"));   // the Korean name model's licence (Apache-2.0)
for (const name of DATA_FILES) copy(need(join(meterRes, name), "it comes with crates/alogs-meter/res"), join(meter, name));
copy(need(pluginList, "it comes with crates/alogs-protocol"), join(meter, "plugins.json"));

const sanctum = join(resources, "sanctumhelper");
empty(sanctum);
copy(sanctumExe, join(sanctum, "SanctumHelper.exe"));
copy(assets, join(sanctum, "Assets"));

const vendor = join(resources, "vendor");
empty(vendor);
for (const name of readdirSync(need(vendorDir, "it comes with crates/alogs-app"))) copy(join(vendorDir, name), join(vendor, name));

const size = (dir) => readdirSync(dir, { withFileTypes: true }).reduce((sum, d) => sum + (d.isDirectory() ? size(join(dir, d.name)) : statSync(join(dir, d.name)).size), 0);
const mb = (dir) => `${(size(dir) / 1048576).toFixed(1)} MB`;
console.log(`resources: meter ${mb(meter)}, sanctumhelper ${mb(sanctum)} (Assets from ${assets}), vendor ${mb(vendor)}`);
