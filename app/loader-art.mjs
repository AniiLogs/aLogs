// Builds the updating window's loader art from ui/img/alogs-loader.svg (the same art as the site's): the app's
// security policy (tauri.conf.json csp) lets no page fetch the SVG and no inline <style> or style="" apply, so the
// SVG's styles go to ui/alogs-loader.css (each buff's style="--n:K" becomes class n-K) and its markup into
// ui/alogs-loader-art.js. Run after changing the SVG:  node loader-art.mjs
import { readFileSync, writeFileSync } from "node:fs";
const svg = readFileSync(new URL("ui/img/alogs-loader.svg", import.meta.url), "utf8");
const css = (svg.match(/<style>([\s\S]*?)<\/style>/) || [])[1];
if (!css) throw new Error("alogs-loader.svg has no <style>");
const ns = new Set();
const markup = svg.replace(/<style>[\s\S]*?<\/style>\s*/, "").replace(/<\?xml[^>]*>\s*/, "")
  .replace(/class="([^"]*)" style="--n:(\d+)"/g, (_, cls, n) => { ns.add(n); return `class="${cls} n-${n}"`; });
if (/style="/.test(markup)) throw new Error("alogs-loader.svg has a style attribute this doesn't handle");
const head = "/* Generated from img/alogs-loader.svg by loader-art.mjs - edit the SVG, then run it. */\n";
writeFileSync(new URL("ui/alogs-loader.css", import.meta.url),
  head + css.trim() + "\n" + [...ns].sort().map((n) => `.alogs-loader .n-${n}{--n:${n}}`).join("\n") + "\n");
writeFileSync(new URL("ui/alogs-loader-art.js", import.meta.url),
  "// Generated from img/alogs-loader.svg by loader-art.mjs - edit the SVG, then run it.\nwindow.ALOGS_LOADER_SVG = " + JSON.stringify(markup.trim()) + ";\n");
console.log(`loader art: ${markup.length} bytes of markup, ${ns.size} buffs`);
