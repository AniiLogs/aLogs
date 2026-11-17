// npm test (node --test): which RC number a build gets (rc-number.mjs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { announcedRc, feedRc, nextRc, recordedRc } from "./rc-number.mjs";

const feedOf = (json, status = 200) => async () => ({ ok: status >= 200 && status < 300, status, json: async () => json });
const offline = async () => { throw new Error("getaddrinfo ENOTFOUND github.com"); };

test("local 24, recorded 25, feed 25 gives 26", () => {
  assert.equal(nextRc({ local: 24, recorded: 25, feed: 25 }), 26);
});

test("feed unreachable still gives 26 from the recorded number", async () => {
  const feed = await announcedRc("0.1.37", { fetchImpl: offline });
  assert.equal(feed.rc, null);
  assert.match(feed.error, /ENOTFOUND/u);
  assert.equal(nextRc({ local: 24, recorded: 25, feed: feed.rc }), 26);
});

test("the highest of the three wins, whichever it is", () => {
  assert.equal(nextRc({ local: 30, recorded: 25, feed: 25 }), 31);
  assert.equal(nextRc({ local: 24, recorded: 25, feed: 40 }), 41);
  assert.equal(nextRc({ local: 0, recorded: 0, feed: null }), 1);
  assert.equal(nextRc({}), 1);
});

test("the feed's RC counts only for the same version", async () => {
  assert.equal(feedRc({ version: "0.1.37+rc.25" }, "0.1.37"), 25);
  assert.equal(feedRc({ version: "0.1.36+rc.33" }, "0.1.37"), null);
  assert.equal(feedRc({ version: "0.1.37" }, "0.1.37"), 0);
  assert.equal(feedRc({}, "0.1.37"), null);
  assert.equal((await announcedRc("0.1.37", { fetchImpl: feedOf({ version: "0.1.37+rc.25", beta: 18 }) })).rc, 25);
  assert.equal((await announcedRc("0.1.38", { fetchImpl: feedOf({ version: "0.1.37+rc.25" }) })).rc, null);
});

test("no feed yet (404) is no number, not an error; other failures are errors", async () => {
  assert.deepEqual(await announcedRc("0.1.37", { fetchImpl: feedOf(null, 404) }), { rc: null });
  const down = await announcedRc("0.1.37", { fetchImpl: feedOf(null, 503) });
  assert.equal(down.rc, null);
  assert.equal(down.error, "HTTP 503");
});

test("betas.json's recorded RC", () => {
  assert.equal(recordedRc({ "0.1.37": 18, rc: { "0.1.37": 25 } }, "0.1.37"), 25);
  assert.equal(recordedRc({ "0.1.37": 18 }, "0.1.37"), 0);
  assert.equal(recordedRc({ rc: { "0.1.36": 33 } }, "0.1.37"), 0);
});
