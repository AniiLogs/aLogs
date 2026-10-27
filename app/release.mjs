// Publishes the built installer as the latest GitHub Release, with that version's notes from CHANGELOG.md.
//   node release.mjs --dry-run     print the release notes and what would be uploaded, publish nothing
//   node release.mjs               create companion-v<version>, upload AniiLogs-Companion-Setup.exe, mark it latest
//   node release.mjs --notes-only  replace the notes of the existing companion-v<version> release
//   add --version 0.1.21 to work on an older release (with --dry-run or --notes-only)
// The Companion page links to releases/latest/download/AniiLogs-Companion-Setup.exe, so the asset name must not change.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { feed, feedRelease, putJson } from "./update-feed.mjs";

const REPO = "AniiLogs/aniilogs.github.io";
const ASSET = "AniiLogs-Companion-Setup.exe";
const dryRun = process.argv.includes("--dry-run");
const notesOnly = process.argv.includes("--notes-only");

const versionArg = process.argv.indexOf("--version");
const version = versionArg > 0 ? process.argv[versionArg + 1] : JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
if (versionArg > 0 && !notesOnly && !dryRun) throw new Error("--version only goes with --dry-run or --notes-only.");
const tag = `companion-v${version}`;
// the newest test build of this version (aLogs_<version>_RC<n>_x64-setup.exe, see clean-installers.mjs)
const nsis = "src-tauri/target/release/bundle/nsis";
const rc = (name) => {
  if (name === `aLogs_${version}_x64-setup.exe`) return 0;
  const m = /^aLogs_(.+)_RC(\d+)_x64-setup\.exe$/u.exec(name);
  return m && m[1] === version ? Number(m[2]) : -1;
};
const newest = readdirSync(nsis).filter((name) => rc(name) >= 0).sort((a, b) => rc(b) - rc(a))[0];
const installer = `${nsis}/${newest ?? `aLogs_${version}_x64-setup.exe`}`;

// This version's section of the changelog: from "## <version>" up to the next "## ".
const changelog = readFileSync("CHANGELOG.md", "utf8").replace(/\r\n/gu, "\n");
const start = changelog.search(new RegExp(`^## ${version.replace(/\./gu, "\\.")}\\s*$`, "mu"));
if (start < 0) throw new Error(`CHANGELOG.md has no "## ${version}" section: write the patch notes first.`);
const rest = changelog.slice(start).split("\n").slice(1).join("\n");
const section = rest.slice(0, rest.search(/^## /mu) >= 0 ? rest.search(/^## /mu) : undefined).trim();
// The changelog wraps lines for editing; GitHub would keep those breaks, so join each paragraph and list item.
const notes = section.replace(/\n(?![\n#-]|$)\s*/gu, " ");

const body = `${notes}

## Install
Download **${ASSET}** below and run it. It updates an existing install and keeps your settings.

The app isn't code-signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.`;

if (!notesOnly && !dryRun && !existsSync(installer)) throw new Error(`${installer} not found: build ${version} first.`);
console.log(`--- ${tag} ---\n${body}\n---`);
if (!notesOnly && existsSync(installer)) console.log(`asset: ${installer} (${(statSync(installer).size / 1048576).toFixed(1)} MB) as ${ASSET}`);
if (dryRun) process.exit(0);

// The GitHub login Git already uses on this PC.
const credential = execFileSync("git", ["credential", "fill"], { input: "protocol=https\nhost=github.com\n\n" }).toString();
const token = /^password=(.+)$/mu.exec(credential)?.[1];
if (!token) throw new Error("No saved GitHub login for git.");
const headers = { authorization: `Bearer ${token}`, accept: "application/vnd.github+json" };
async function github(url, options = {}) {
  const response = await fetch(url, { ...options, headers: { ...headers, ...options.headers } });
  if (!response.ok) throw new Error(`${options.method || "GET"} ${url}: HTTP ${response.status} ${await response.text()}`);
  return response.json();
}

if (notesOnly) {
  const release = await github(`https://api.github.com/repos/${REPO}/releases/tags/${tag}`);
  await github(`https://api.github.com/repos/${REPO}/releases/${release.id}`, { method: "PATCH", body: JSON.stringify({ body }) });
  console.log(`notes updated: ${release.html_url}`);
} else {
  const release = await github(`https://api.github.com/repos/${REPO}/releases`, {
    method: "POST",
    body: JSON.stringify({ tag_name: tag, target_commitish: "main", name: `aLogs ${version}`, body,
      draft: false, prerelease: false, make_latest: "true" }),
  });
  const asset = await github(`https://uploads.github.com/repos/${REPO}/releases/${release.id}/assets?name=${ASSET}`, {
    method: "POST", headers: { "content-type": "application/octet-stream" }, body: readFileSync(installer),
  });
  console.log(`published ${release.html_url} with ${asset.name} (${asset.size} bytes)`);
  // the app's update feeds: latest.json on this release, and the beta feed points here too (testers move on to it)
  const json = feed({ version, rc: rc(newest ?? ""), notes, url: asset.browser_download_url, installer });
  await putJson({ headers, repo: REPO, release, name: "latest.json", json });
  await putJson({ headers, repo: REPO, release: await feedRelease({ headers, repo: REPO }), name: "beta.json", json });
  console.log(`update feeds: ${json.version}`);
  const all = await github(`https://api.github.com/repos/${REPO}/releases?per_page=100`);
  for (const beta of all.filter((r) => r.tag_name === `${tag}-beta` || r.tag_name.startsWith(`${tag}-beta.`))) {
    await fetch(`https://api.github.com/repos/${REPO}/releases/${beta.id}`, { method: "DELETE", headers });
    await fetch(`https://api.github.com/repos/${REPO}/git/refs/tags/${beta.tag_name}`, { method: "DELETE", headers });
    console.log(`removed ${beta.tag_name}: the release replaces it`);
  }
}
