// Writes this build into alogs-app.exe's file details (Properties › Details), after the program is built and before
// the installer is made (tauri.conf.json, beforeBundleCommand): Product version "0.1.37 B14 (RC21)" for a beta, plain
// "0.1.37" for a release, and File version 0.1.37.21 (the last number is the RC, which counts every build).
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { rcedit } from "rcedit";

const here = dirname(fileURLToPath(import.meta.url));
const exe = join(here, "src-tauri", "target", "release", "alogs-app.exe");
const file = join(here, "src-tauri", "build.json");
const build = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : {};
const version = JSON.parse(readFileSync(join(here, "src-tauri", "tauri.conf.json"), "utf8")).version;
const rc = Number(build.rc) || 0;
// a build made without tauri-build.mjs (no RC number): left as Tauri wrote it
if (rc && existsSync(exe)) {
  const product = build.release || !build.beta ? version : `${version} B${build.beta} (RC${rc})`;
  await rcedit(exe, { "file-version": `${version}.${rc}`, "product-version": product });
  console.log(`file details: Product version "${product}", File version ${version}.${rc}`);
}
