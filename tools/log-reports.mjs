#!/usr/bin/env node
// Log reports players sent with aLogs' "Send logs" button, for AniiLogs developers (and the AI agents they work with)
// to diagnose a problem and fix it in a pull request. Plain Node 18+, nothing to install. See docs/log-reports.md.
//
//   node tools/log-reports.mjs list [--all] [--beta]  newest first: id, UID, when (UTC), size, app build, the player's
//                                                     note and developers' notes (--all: cleared reports too; --beta:
//                                                     only testers' Beta Menu sends, with their results or bugs)
//   node tools/log-reports.mjs get <id> [folder]      download a report and unzip it (default: ./log-reports/<id>),
//                                                     with the player's screenshots (screenshot-1.png, ...) and, for a
//                                                     Beta Menu send, beta.json (the test results or the bugs)
//   node tools/log-reports.mjs note <id> "<text>"     leave a note: what you found, what you changed, the PR
//   node tools/log-reports.mjs clear <id> | all       administrators only: delete the logs once the fix is in
//                                                     (the report's line and notes stay)
//
// Needs a developer's own AniiLogs desktop token (alb_...): the ALOGS_DEV_TOKEN environment variable, or the first
// line of ~/.aniilogs/dev-token. The account must be an administrator (or the owner); anyone else is turned away.
// ALOGS_API overrides the API (default https://aniilogs-api.pages.dev/api).
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

const API = (process.env.ALOGS_API || "https://aniilogs-api.pages.dev/api").replace(/\/+$/u, "");
const TOKEN_FILE = join(process.env.USERPROFILE || homedir(), ".aniilogs", "dev-token");
const token = (process.env.ALOGS_DEV_TOKEN || (existsSync(TOKEN_FILE) ? readFileSync(TOKEN_FILE, "utf8") : ""))
  .split(/\r?\n/u)[0].trim();
const idOk = (id) => /^[A-Za-z0-9_-]{8,40}$/u.test(id || "");

class Refused extends Error {}

async function call(path, method = "GET", body) {
  const headers = { authorization: `Bearer ${token}`, ...(body ? { "content-type": "application/json" } : {}) };
  const response = await fetch(`${API}${path}`, { method, headers, body: body && JSON.stringify(body) });
  if (!response.ok) {
    const answer = await response.json().catch(() => ({}));
    throw new Refused(`${answer.error || response.statusText} (${response.status})`);
  }
  return response;
}

const when = (t) => `${new Date(t * 1000).toISOString().replace("T", " ").slice(0, 16)} UTC`;
// Windows' own tar unzips; Git Bash's GNU tar (often first on PATH) can't
const tar = process.platform === "win32" && process.env.SystemRoot ? join(process.env.SystemRoot, "System32", "tar.exe") : "tar";

const [action, arg, folder] = process.argv.slice(2);
const isBeta = (r) => String(r.kind || "").startsWith("beta");
// a Beta Menu send in a few lines: what didn't work and every note, or each bug's title
function betaLines(r) {
  const body = r.body || {};
  if (Array.isArray(body.tests)) {
    return body.tests.filter((t) => t.tested && (t.working === false || t.note))
      .map((t) => `${t.working === false ? "NOT WORKING" : "working"}: ${t.title}${t.note ? ` - "${t.note}"` : ""}`);
  }
  if (Array.isArray(body.bugs)) {
    return body.bugs.map((b, i) => `bug ${i + 1}: ${b.title}${b.area ? ` [${b.area}]` : ""}${b.severity ? ` (${b.severity})` : ""}${b.screenshots ? `, ${b.screenshots} screenshot(s)` : ""}`);
  }
  return [];
}
if (!token && action) {
  console.error(`No developer token: set ALOGS_DEV_TOKEN or put it in ${TOKEN_FILE} (see docs/log-reports.md).`);
  process.exit(1);
}
try {
if (action === "list") {
  const listFlags = new Set(process.argv.slice(3));
  let { reports } = await (await call(`/admin/log-reports${listFlags.has("--all") ? "?all=1" : ""}`)).json();
  if (listFlags.has("--beta")) reports = reports.filter(isBeta);
  if (!reports.length) console.log("No log reports.");
  for (const r of reports) {
    const cleared = r.clearedAt ? `  [cleared ${when(r.clearedAt)}]` : "";
    const shots = r.screenshots ? `  ${r.screenshots} screenshot${r.screenshots === 1 ? "" : "s"}` : "";
    console.log(`${r.id}  UID ${r.gameUid}  ${when(r.createdAt)}  ${(r.sizeBytes / 1024).toFixed(0)} KB  ${r.appVersion || "?"}${shots}${cleared}`);
    if (isBeta(r)) {
      console.log(`    ${r.kind}: ${r.summary || ""}`);
      for (const line of betaLines(r)) console.log(`      ${line}`);
    }
    if (r.note) console.log(`    player: "${r.note}"`);
    for (const n of r.notes || []) console.log(`    ${n.author}, ${when(n.createdAt)}: ${n.note}`);
  }
} else if (action === "note" && idOk(arg) && process.argv.slice(4).join(" ").trim()) {
  await call(`/admin/log-reports/${arg}/notes`, "POST", { note: process.argv.slice(4).join(" ").trim() });
  console.log("note added");
} else if (action === "get" && idOk(arg)) {
  const out = resolve(folder || join("log-reports", arg));
  mkdirSync(out, { recursive: true });
  const zip = join(out, "report.zip");
  writeFileSync(zip, Buffer.from(await (await call(`/admin/log-reports/${arg}`)).arrayBuffer()));
  // the player's screenshots, numbered from 1 until there are no more
  const EXT = { "image/png": "png", "image/jpeg": "jpg", "image/webp": "webp" };
  for (let n = 1; n <= 30; n += 1) {                           // a Beta Menu bug send has up to 30
    const shot = await fetch(`${API}/admin/log-reports/${arg}/screenshots/${n}`, { headers: { authorization: `Bearer ${token}` } });
    if (!shot.ok) break;
    writeFileSync(join(out, `screenshot-${n}.${EXT[shot.headers.get("content-type")] || "img"}`), Buffer.from(await shot.arrayBuffer()));
  }
  // a Beta Menu send: its results or bugs next to the logs
  const { reports } = await (await call("/admin/log-reports?all=1")).json();
  const report = reports.find((r) => r.id === arg);
  if (report && isBeta(report)) {
    writeFileSync(join(out, "beta.json"), JSON.stringify({ kind: report.kind, summary: report.summary, ...report.body }, null, 1));
  }
  try {
    execFileSync(tar, ["-xf", zip, "-C", out], { stdio: "ignore" });     // bsdtar unzips (Windows 10+, macOS, Linux)
    console.log(out);
  } catch {
    console.log(`${zip} (unzip it yourself: tar could not)`);
  }
} else if (action === "clear" && (arg === "all" || idOk(arg))) {
  const { cleared } = await (await call(arg === "all" ? "/admin/log-reports" : `/admin/log-reports/${arg}`, "DELETE")).json();
  console.log(`cleared ${cleared}`);
} else {
  console.log('usage: node tools/log-reports.mjs list [--all] [--beta] | get <id> [folder] | note <id> "<text>" | clear <id>|all');
  process.exitCode = 1;
}
} catch (error) {
  // the API's answer, without a stack trace (and no process.exit mid-fetch, which crashes Node on Windows)
  if (!(error instanceof Refused)) throw error;
  console.error(error.message);
  process.exitCode = 1;
}
