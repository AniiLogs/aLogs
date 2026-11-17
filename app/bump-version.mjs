// Bumps the patch version (0.1.0 -> 0.1.1) everywhere the Rust build reads it, so every build installs as an upgrade
// over the previous one: crates/alogs-app/tauri.conf.json (the installer and updater), crates/alogs-app/Cargo.toml,
// the workspace's Cargo.toml (every other crate's version.workspace), the workspace crates in Cargo.lock, and
// app/package.json. `node bump-version.mjs minor` or `major` bumps those instead. Every file is checked before any is
// written, so a file not looking as expected changes nothing.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import * as paths from "./paths.mjs";

const part = process.argv[2] || "patch";
const conf = JSON.parse(readFileSync(paths.conf, "utf8"));
const current = conf.version;
const [major, minor, patch] = current.split(".").map(Number);
const next = part === "major" ? `${major + 1}.0.0` : part === "minor" ? `${major}.${minor + 1}.0` : `${major}.${minor}.${patch + 1}`;
const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

const writes = new Map();                                   // file -> its new text
// a file's text with a replacement that must happen, keeping its line endings
function replace(file, pattern, to) {
  const text = writes.get(file) ?? readFileSync(file, "utf8");
  if (!pattern.test(text)) throw new Error(`${file}: no version ${current} where it was expected (${pattern}); nothing changed`);
  writes.set(file, text.replace(pattern, to));
}

conf.version = next;
writes.set(paths.conf, `${JSON.stringify(conf, null, 2)}\n`);
replace(join(paths.crate, "Cargo.toml"), new RegExp(`^version = "${escape(current)}"`, "mu"), `version = "${next}"`);
// [workspace.package] version = "..." (the version line under that heading)
replace(join(paths.root, "Cargo.toml"), new RegExp(`^(\\[workspace\\.package\\][^[]*?^version = )"${escape(current)}"`, "mu"), `$1"${next}"`);
// the workspace's own crates in Cargo.lock (every folder in crates/ with a Cargo.toml), so a --locked build agrees
const names = readdirSync(join(paths.root, "crates"), { withFileTypes: true }).filter((d) => d.isDirectory()).flatMap((d) => {
  try { return [/^name = "([^"]+)"/mu.exec(readFileSync(join(paths.root, "crates", d.name, "Cargo.toml"), "utf8"))[1]]; } catch { return []; }
});
for (const name of names) {
  replace(join(paths.root, "Cargo.lock"), new RegExp(`(^\\[\\[package\\]\\]\\r?\\nname = "${escape(name)}"\\r?\\nversion = )"${escape(current)}"`, "mu"), `$1"${next}"`);
}
const pkgFile = join(paths.appDir, "package.json");
const pkgText = readFileSync(pkgFile, "utf8");
const pkg = JSON.parse(pkgText);
pkg.version = next;
const pkgOut = `${JSON.stringify(pkg, null, 2)}\n`;
writes.set(pkgFile, pkgText.includes("\r\n") ? pkgOut.replace(/\n/gu, "\r\n") : pkgOut);

for (const [file, text] of writes) writeFileSync(file, text);
console.log(`version ${current} -> ${next} (${names.length} workspace crates in Cargo.lock)`);
