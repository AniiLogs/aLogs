// Where the build and publish scripts find the Rust workspace (crates/, Cargo.toml at the top of the repository) that
// betas and releases are built from since 17 Nov 2026. The scripts and their bookkeeping (CHANGELOG.md, betas.json)
// stay in app/; the program, its config and its build output are the workspace's. Every path here is absolute, so a
// script works from any folder.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const appDir = dirname(fileURLToPath(import.meta.url));       // app/: these scripts, CHANGELOG.md, betas.json
export const root = join(appDir, "..");                               // the repository, with the workspace's Cargo.toml
export const crate = join(root, "crates", "alogs-app");               // the Tauri app
export const conf = join(crate, "tauri.conf.json");
export const buildJson = join(crate, "build.json");                   // this build's version, RC and beta (update.rs reads it)
export const release = join(root, "target", "release");               // the workspace's release build
export const bundle = join(release, "bundle");
export const nsis = join(bundle, "nsis");
export const map = join(release, "alogs-app.map");                    // written by crates/alogs-app/build.rs
export const exe = join(release, "alogs-app.exe");

/** The app's version (crates/alogs-app/tauri.conf.json). */
export const version = () => JSON.parse(readFileSync(conf, "utf8")).version;

/** Where a version's RC numbers are counted (the number of its last build), so they keep counting up. */
export const counter = (v) => join(bundle, `rc-${v}.txt`);
// the B16 app's counter, from when betas were built in app/src-tauri: the Rust build carries on from its number
const oldCounter = (v) => join(appDir, "src-tauri", "target", "release", "bundle", `rc-${v}.txt`);

/** The last RC number built of a version, by either build: the next one is one past it. */
export function lastRc(v) {
  const read = (file) => (existsSync(file) ? Number(readFileSync(file, "utf8")) || 0 : 0);
  return Math.max(read(counter(v)), read(oldCounter(v)));
}
