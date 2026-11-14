// The update feeds the app reads (src-tauri/src/update.rs): Tauri updater JSON with this build's version as
// "<version>+rc.<n>", its signature (the .sig tauri-build.mjs makes next to the installer) and the download URL.
//   releases: latest.json on the release itself (reached through releases/latest/download/latest.json)
//   betas:    beta.json on the fixed "update-feed" pre-release; a release points it at itself too, so testers move on
import { existsSync, readFileSync } from "node:fs";
import { execSync } from "node:child_process";

export const FEED_TAG = "update-feed";

/** A version's notes from CHANGELOG.md ("## <version>" up to the next "## "), each paragraph and list item on one line. */
export function changelogNotes(version, text = null) {
  const changelog = (text ?? readFileSync("CHANGELOG.md", "utf8")).replace(/\r\n/gu, "\n");
  const start = changelog.search(new RegExp(`^## ${version.replace(/\./gu, "\\.")}\\s*$`, "mu"));
  if (start < 0) return "";
  const rest = changelog.slice(start).split("\n").slice(1).join("\n");
  const end = rest.search(/^## /mu);
  return rest.slice(0, end >= 0 ? end : undefined).trim().replace(/\n(?![\n#-]|$)\s*/gu, " ");
}

/** A beta's notes (the owner, 14 Nov: a beta's notes are what changed since the previous beta; a release's, everything
 * since the last release): the items of the version's section that weren't in it when the previous beta was published
 * (the CHANGELOG at the commit that recorded beta N-1 in betas.json), then the whole section. Just the section for a
 * first beta, or when that commit can't be found. */
export function betaNotes(version, beta) {
  const all = changelogNotes(version);
  if (!beta || beta < 2) return all;
  let old = null;
  try {
    const run = (cmd) => execSync(cmd, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    for (const sha of run("git log --format=%H -- betas.json").split("\n").filter(Boolean)) {
      const betas = JSON.parse(run(`git show ${sha}:app/betas.json`));
      if (betas[version] === beta - 1) { old = run(`git show ${sha}:app/CHANGELOG.md`); break; }
    }
  } catch { old = null; }
  if (old === null) return all;
  const items = (text) => text.split("\n").filter((line) => line.startsWith("- "));
  const before = new Set(items(changelogNotes(version, old)));
  const fresh = items(all).filter((line) => !before.has(line));
  if (!fresh.length) return all;
  return `**New in Beta ${beta}** (since Beta ${beta - 1}):\n\n${fresh.join("\n")}\n\n**Everything in ${version} so far** (since the last release):\n\n${all}`;
}

export function feed({ version, rc, beta, notes, url, installer }) {
  const sig = `${installer}.sig`;
  if (!existsSync(sig)) throw new Error(`${sig} is missing: this build wasn't signed for updates (build it with npm run build:rc).`);
  return {
    version: `${version}+rc.${rc}`,   // the updater orders builds by this, so it stays the RC number
    ...(beta ? { beta } : {}),        // what players call it (B2), shown in the sidebar
    notes,
    pub_date: new Date().toISOString(),
    platforms: { "windows-x86_64": { signature: readFileSync(sig, "utf8").trim(), url } },
  };
}

/** Puts `name` (JSON) on the release, replacing a file of that name. */
export async function putJson({ headers, repo, release, name, json }) {
  for (const asset of release.assets || []) {
    if (asset.name === name) await fetch(`https://api.github.com/repos/${repo}/releases/assets/${asset.id}`, { method: "DELETE", headers });
  }
  const r = await fetch(`https://uploads.github.com/repos/${repo}/releases/${release.id}/assets?name=${name}`, {
    method: "POST", headers: { ...headers, "content-type": "application/json" }, body: JSON.stringify(json, null, 1),
  });
  if (!r.ok) throw new Error(`upload ${name}: HTTP ${r.status} ${await r.text()}`);
  return r.json();
}

/** The beta feed's release (made once): a pre-release that is never "latest". */
export async function feedRelease({ headers, repo }) {
  const found = await fetch(`https://api.github.com/repos/${repo}/releases/tags/${FEED_TAG}`, { headers });
  if (found.ok) return found.json();
  const r = await fetch(`https://api.github.com/repos/${repo}/releases`, {
    method: "POST", headers,
    body: JSON.stringify({ tag_name: FEED_TAG, target_commitish: "main", name: "aLogs update feed", prerelease: true, make_latest: "false",
      body: "Used by the aLogs app to find beta updates (General › Get beta updates). Nothing to download here." }),
  });
  if (!r.ok) throw new Error(`create ${FEED_TAG}: HTTP ${r.status} ${await r.text()}`);
  return r.json();
}
