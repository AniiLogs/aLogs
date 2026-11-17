// Writes this build into alogs-app.exe's file details (Properties › Details), after the program is built and before
// the installer is made (crates/alogs-app/tauri.conf.json, beforeBundleCommand): Product version "0.1.37 B14 (RC21)"
// for a beta, plain "0.1.37" for a release, and File version 0.1.37.21 (the last number is the RC, which counts every
// build). The program is the workspace's target/release/alogs-app.exe (paths.mjs).
import { existsSync, readFileSync } from "node:fs";
import { rcedit } from "rcedit";
import * as paths from "./paths.mjs";

const build = existsSync(paths.buildJson) ? JSON.parse(readFileSync(paths.buildJson, "utf8")) : {};
const version = paths.version();
const rc = Number(build.rc) || 0;
// a build made without tauri-build.mjs (no RC number): left as Tauri wrote it
if (rc && existsSync(paths.exe)) {
  const product = build.release || !build.beta ? version : `${version} B${build.beta} (RC${rc})`;
  await rcedit(paths.exe, { "file-version": `${version}.${rc}`, "product-version": product });
  console.log(`file details: Product version "${product}", File version ${version}.${rc}`);
}
