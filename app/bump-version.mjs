// Bumps the patch version (0.1.0 -> 0.1.1) in tauri.conf.json, Cargo.toml and package.json, so every build installs
// as an upgrade over the previous one. `node bump-version.mjs minor` or `major` bumps those instead.
import { readFileSync, writeFileSync } from "node:fs";

const part = process.argv[2] || "patch";
const confPath = "src-tauri/tauri.conf.json";
const conf = JSON.parse(readFileSync(confPath, "utf8"));
const [major, minor, patch] = conf.version.split(".").map(Number);
const next = part === "major" ? `${major + 1}.0.0` : part === "minor" ? `${major}.${minor + 1}.0` : `${major}.${minor}.${patch + 1}`;

conf.version = next;
writeFileSync(confPath, `${JSON.stringify(conf, null, 2)}\n`);
const cargo = readFileSync("src-tauri/Cargo.toml", "utf8").replace(/^version = "[^"]+"/mu, `version = "${next}"`);
writeFileSync("src-tauri/Cargo.toml", cargo);
const pkg = JSON.parse(readFileSync("package.json", "utf8"));
pkg.version = next;
writeFileSync("package.json", `${JSON.stringify(pkg, null, 2)}\n`);
console.log(`version ${major}.${minor}.${patch} -> ${next}`);
