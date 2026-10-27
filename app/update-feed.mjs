// The update feeds the app reads (src-tauri/src/update.rs): Tauri updater JSON with this build's version as
// "<version>+rc.<n>", its signature (the .sig tauri-build.mjs makes next to the installer) and the download URL.
//   releases: latest.json on the release itself (reached through releases/latest/download/latest.json)
//   betas:    beta.json on the fixed "update-feed" pre-release; a release points it at itself too, so testers move on
import { existsSync, readFileSync } from "node:fs";

export const FEED_TAG = "update-feed";

/** A version's notes from CHANGELOG.md ("## <version>" up to the next "## "), each paragraph and list item on one line. */
export function changelogNotes(version) {
  const changelog = readFileSync("CHANGELOG.md", "utf8").replace(/\r\n/gu, "\n");
  const start = changelog.search(new RegExp(`^## ${version.replace(/\./gu, "\\.")}\\s*$`, "mu"));
  if (start < 0) return "";
  const rest = changelog.slice(start).split("\n").slice(1).join("\n");
  const end = rest.search(/^## /mu);
  return rest.slice(0, end >= 0 ? end : undefined).trim().replace(/\n(?![\n#-]|$)\s*/gu, " ");
}

export function feed({ version, rc, notes, url, installer }) {
  const sig = `${installer}.sig`;
  if (!existsSync(sig)) throw new Error(`${sig} is missing: this build wasn't signed for updates (build it with npm run build:rc).`);
  return {
    version: `${version}+rc.${rc}`,
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
