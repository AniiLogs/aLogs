// Which RC number a build gets. Two PCs build and publish betas, and the app's updater installs a build of the same
// version only when its RC is higher than the installed one's (crates/alogs-app/src/update.rs), so the number can't
// come from one PC's counter alone: a build is one past the highest of
//   - this PC's own counters (paths.lastRc: the Rust build's and the B16 app's),
//   - the RC recorded in the repository for the last published beta (betas.json "rc", written by share.mjs --beta),
//   - the RC the beta feed announces now (beta.json on the update-feed release), when it can be reached.
// The feed's number only counts for the same version (a feed still on an older version says nothing about this one).

/** The beta feed the app reads ("Get beta updates"), the same one update.rs checks. */
export const BETA_FEED = "https://github.com/AniiLogs/aniilogs.github.io/releases/download/update-feed/beta.json";

/** The RC a feed's JSON announces for `version` ("0.1.37+rc.25" -> 25), null for another version or none. */
export function feedRc(json, version) {
  const m = /^(\d+\.\d+\.\d+)(?:\+rc\.(\d+))?$/u.exec(String(json?.version ?? ""));
  return m && m[1] === version ? Number(m[2] ?? 0) : null;
}

/** The RC betas.json records as last published for `version` (0 when none is recorded). */
export function recordedRc(betas, version) {
  return Number(betas?.rc?.[version]) || 0;
}

/** The next build's RC: one past the highest known one (the feed's left out when it couldn't be read: null). */
export function nextRc({ local = 0, recorded = 0, feed = null }) {
  return Math.max(Number(local) || 0, Number(recorded) || 0, feed == null ? 0 : Number(feed) || 0) + 1;
}

/** What the beta feed announces for `version`: { rc } (null: no feed yet, or another version), or { rc: null, error }
 * when it couldn't be read (no network, GitHub down, not JSON). */
export async function announcedRc(version, { url = BETA_FEED, timeoutMs = 10000, fetchImpl = globalThis.fetch } = {}) {
  try {
    const r = await fetchImpl(url, { signal: AbortSignal.timeout(timeoutMs), headers: { "cache-control": "no-cache" } });
    if (r.status === 404) return { rc: null };                       // no beta feed published yet
    if (!r.ok) return { rc: null, error: `HTTP ${r.status}` };
    const json = await r.json();
    return { rc: feedRc(json, version), announced: json?.version ?? null };
  } catch (e) {
    return { rc: null, error: e?.message || String(e) };
  }
}
