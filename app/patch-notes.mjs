// Writes the About page's patch notes from CHANGELOG.md (the newest five versions) into the app's
// crates/alogs-app/ui/patch-notes.js. Runs before every build (npm run build, build:rc), so the notes in the app always
// match the ones on the GitHub Release.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import * as paths from "./paths.mjs";

const KEEP = 5;
const text = readFileSync(join(paths.appDir, "CHANGELOG.md"), "utf8").replace(/\r\n/gu, "\n");
const versions = [];
let block = null;
for (const line of text.split("\n")) {
  const version = /^## (\d+\.\d+\.\d+)\s*$/u.exec(line);
  if (version) { versions.push({ version: version[1], intro: [], sections: [] }); block = null; continue; }
  const current = versions.at(-1);
  if (!current) continue;                                          // the file's own heading and how-to
  const heading = /^### (.+)$/u.exec(line);
  if (heading) { block = { heading: heading[1].trim(), items: [] }; current.sections.push(block); continue; }
  if (line.startsWith("- ")) { block?.items.push(line.slice(2).trim()); continue; }
  if (!line.trim()) continue;
  // a wrapped continuation of the last item, or an intro paragraph before the first heading
  if (block?.items.length && /^\s/u.test(line)) block.items[block.items.length - 1] += ` ${line.trim()}`;
  else if (!block) current.intro.push(line.trim());
}

const notes = versions.slice(0, KEEP).map((v) => ({ ...v, intro: v.intro.join(" ") }));
const js = `// Generated from CHANGELOG.md by patch-notes.mjs - edit the changelog, not this file.\nwindow.PATCH_NOTES = ${JSON.stringify(notes, null, 1)};\n`;
writeFileSync(join(paths.crate, "ui", "patch-notes.js"), js);
console.log(`patch notes: ${notes.map((v) => v.version).join(", ")}`);
