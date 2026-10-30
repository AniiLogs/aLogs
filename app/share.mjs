// Publishes the newest test build (src-tauri/target/release/bundle/nsis/aLogs_<version>_RC<n>_x64-setup.exe) to GitHub,
// never as "latest", so the site's Download button and the release feed (latest.json) keep serving the real release
// (release.mjs). Tester links are pre-releases; the public beta is a normal release (the user's call, 2026-10-30).
//   node share.mjs --test       one fixed tester link, replaced each time:
//                               releases/download/test-build/aLogs-Test-Setup.exe
//   node share.mjs --beta       folds the newest RC into this version's one public beta, "<version> Beta"
//                               (companion-v<version>-beta): same release and link, the installer swapped and the notes
//                               replaced with the version's CHANGELOG.md section; made the first time
//   node share.mjs --beta --notes-only   only replaces the beta's notes with CHANGELOG.md's (after editing them)
//   a beta also updates the app's beta feed (update-feed.mjs), so testers get it through the app
//   add --dry-run to print what would happen, publish nothing
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { changelogNotes, feed, feedRelease, putJson } from "./update-feed.mjs";

const REPO = "AniiLogs/aniilogs.github.io";
const mode = process.argv.includes("--beta") ? "beta" : process.argv.includes("--test") ? "test" : null;
const dryRun = process.argv.includes("--dry-run");
const notesOnly = process.argv.includes("--notes-only");
if (!mode) throw new Error("Use --test (tester link) or --beta (this version's beta).");

const version = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const nsis = "src-tauri/target/release/bundle/nsis";
const rc = (name) => { const m = /^aLogs_(.+)_RC(\d+)_x64-setup\.exe$/u.exec(name); return m && m[1] === version ? Number(m[2]) : -1; };
const newest = readdirSync(nsis).filter((name) => rc(name) >= 0).sort((a, b) => rc(b) - rc(a))[0];
if (!newest) throw new Error(`No test build of ${version} in ${nsis}: run npm run build:rc first.`);
const installer = `${nsis}/${newest}`;

// The GitHub login Git already uses on this PC.
const credential = execFileSync("git", ["credential", "fill"], { input: "protocol=https\nhost=github.com\n\n" }).toString();
const token = /^password=(.*)$/mu.exec(credential)?.[1]?.trim();
if (!token) throw new Error("No GitHub login found for git on this PC.");
const headers = { authorization: `Bearer ${token}`, accept: "application/vnd.github+json" };
async function github(url, options = {}) {
  const r = await fetch(url, { ...options, headers: { ...headers, ...(options.headers || {}) } });
  const text = await r.text();
  if (!r.ok && !(options.method === "DELETE" && r.status === 404)) throw new Error(`${r.status} ${url}: ${text.slice(0, 300)}`);
  return text ? JSON.parse(text) : null;
}
async function remove(release) {
  console.log(`removing ${release.tag_name} (${release.name})`);
  if (dryRun) return;
  await github(`https://api.github.com/repos/${REPO}/releases/${release.id}`, { method: "DELETE" });
  await github(`https://api.github.com/repos/${REPO}/git/refs/tags/${release.tag_name}`, { method: "DELETE" });
}

const releases = await github(`https://api.github.com/repos/${REPO}/releases?per_page=100`);
// this version's beta, and the numbered betas from before betas were folded into one (companion-v<version>-beta.N)
const betaTag = new RegExp(`^companion-v${version.replace(/\./gu, "\\.")}-beta(\\.\\d+)?$`, "u");
// tester uploads from before this script (test-<version>-rc<n>) count as tester links too
const isTest = (r) => r.tag_name === "test-build" || /^test-.+-rc\d+$/u.test(r.tag_name);
let tag, name, asset, body, stale, fold = null;
const built = `${version} RC${rc(newest)}, ${new Date().toISOString().slice(0, 10)}`;
if (mode === "test") {
  tag = "test-build"; name = `aLogs test build (${version} RC${rc(newest)})`; asset = "aLogs-Test-Setup.exe";
  body = `Test build for testers, replaced by each new one (this link always gives the newest). Not a release: ${built}.`;
  stale = releases.filter(isTest);
} else {
  const betas = releases.filter((r) => betaTag.test(r.tag_name));
  tag = `companion-v${version}-beta`; name = `aLogs ${version} Beta`; asset = `AniiLogs-Companion-${version}-beta-Setup.exe`;
  body = `Public beta of ${version}, updated as it goes (this one: ${built}). Not the current release: the site's Download button stays on the released version.\n\n${changelogNotes(version)}`;
  fold = betas.find((r) => r.tag_name === tag) ?? null;                 // the beta this build goes into
  stale = betas.filter((r) => r !== fold);                               // numbered betas from before go
}
body += `\n\nThe app isn't code-signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.`;

if (notesOnly) {                                  // the notes alone: no build swapped, nothing removed
  if (mode !== "beta" || !fold) throw new Error("--notes-only needs --beta and an existing beta of this version.");
  const keep = /^Public beta of .*?\n\n/su.exec(fold.body || "")?.[0] ?? "";   // which build it is stays as it was
  const notes = `${keep}${changelogNotes(version)}\n\nThe app isn't code-signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.`;
  if (!dryRun) await github(`https://api.github.com/repos/${REPO}/releases/${fold.id}`, { method: "PATCH", body: JSON.stringify({ body: notes }) });
  console.log(`${dryRun ? "[dry run] " : ""}${name}: notes replaced from CHANGELOG.md (${notes.length} characters)`);
  process.exitCode = 0;
} else {
console.log(`${dryRun ? "[dry run] " : ""}${name}${fold ? " (updated in place)" : ""}: ${installer} (${(statSync(installer).size / 1048576).toFixed(1)} MB) as ${asset}, tag ${tag}`);
for (const r of stale) await remove(r);
if (!dryRun) await publish();
}

async function publish() {
let release;
const listing = { prerelease: mode !== "beta", make_latest: "false" };   // "latest" must stay the real release
if (fold) {                                       // same release and link: new notes, the installer swapped
  release = await github(`https://api.github.com/repos/${REPO}/releases/${fold.id}`, { method: "PATCH", body: JSON.stringify({ name, body, ...listing }) });
  for (const a of fold.assets || []) await github(`https://api.github.com/repos/${REPO}/releases/assets/${a.id}`, { method: "DELETE" });
} else {
  release = await github(`https://api.github.com/repos/${REPO}/releases`, {
    method: "POST", body: JSON.stringify({ tag_name: tag, name, body, ...listing }),
  });
}
const uploaded = await github(`https://uploads.github.com/repos/${REPO}/releases/${release.id}/assets?name=${asset}`, {
  method: "POST", headers: { "content-type": "application/octet-stream", "content-length": String(statSync(installer).size) },
  body: readFileSync(installer),
});
if (mode === "beta") {                            // the app's beta feed points at this build
  const json = feed({ version, rc: rc(newest), notes: body, url: uploaded.browser_download_url, installer });
  await putJson({ headers, repo: REPO, release: await feedRelease({ headers, repo: REPO }), name: "beta.json", json });
  console.log(`beta feed: ${json.version}`);
}
const latest = await github(`https://api.github.com/repos/${REPO}/releases/latest`).catch(() => null);
console.log(`published ${release.html_url}\ndownload  ${uploaded.browser_download_url} (${uploaded.size} bytes)\nlatest release is still ${latest?.tag_name}`);
}
