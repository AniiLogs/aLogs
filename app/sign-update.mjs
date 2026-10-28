// Signs an update file for the app's updater (tauri-plugin-updater) with the version its feed announces.
//
// `tauri build` signs the installer with the app's plain version ("0.1.28"), but the feeds announce each build as
// "0.1.28+rc.17" (update-feed.mjs) so builds of one version can be told apart, and the updater refuses an update
// whose signed version differs from the announced one ("The update was signed for version 0.1.28 but the update
// endpoint announced version 0.1.28+rc.17"). So each build's installer is signed again here, as minisign (what Tauri
// uses) with "version:<version>+rc.<n>" in the signed trusted comment, using the same key (%USERPROFILE%\.tauri).
//
// Minisign, prehashed: the file's BLAKE2b-512 is signed with Ed25519; a second Ed25519 signature covers that
// signature plus the trusted comment. The key file is rsign's encrypted secret key (scrypt + XOR), here with an
// empty password. Node's own crypto does it all; nothing to install.
import { createHash, createPrivateKey, createPublicKey, scryptSync, sign, verify } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const b64url = (buf) => Buffer.from(buf).toString("base64url");

// libsodium's crypto_pwhash_scryptsalsa208sha256 parameters from opslimit/memlimit (pickparams)
function scryptParams(opslimit, memlimit) {
  const ops = Math.max(opslimit, 32768);
  const r = 8;
  let maxN, p;
  if (ops < memlimit / 32) {
    p = 1;
    maxN = ops / (r * 4);
  } else {
    maxN = memlimit / (r * 128);
  }
  let logN = 1;
  for (; logN < 63; logN++) if (2 ** logN > maxN / 2) break;
  if (!(ops < memlimit / 32)) {
    const maxrp = Math.min(Math.floor(ops / 4 / 2 ** logN), 0x3fffffff);
    p = Math.floor(maxrp / r);
  }
  return { N: 2 ** logN, r, p };
}

/** The Ed25519 key in an rsign/Tauri secret key file (base64 of "untrusted comment: …\n<base64 key>"). */
export function loadSecretKey(keyFile, password = "") {
  const text = Buffer.from(readFileSync(keyFile, "utf8").trim(), "base64").toString("utf8");
  const raw = Buffer.from(text.split(/\r?\n/u)[1].trim(), "base64");
  if (raw.length !== 158 || raw.subarray(0, 2).toString() !== "Ed") throw new Error("not a minisign secret key");
  const kdf = raw.subarray(2, 4).toString();
  const salt = raw.subarray(6, 38);
  const opslimit = Number(raw.readBigUInt64LE(38));
  const memlimit = Number(raw.readBigUInt64LE(46));
  let keynum = Buffer.from(raw.subarray(54, 158));                  // key id (8) + secret key (64) + checksum (32)
  if (kdf === "Sc") {
    const { N, r, p } = scryptParams(opslimit, memlimit);
    const stream = scryptSync(password, salt, keynum.length, { N, r, p, maxmem: 128 * N * r * p + 1024 * 1024 });
    keynum = Buffer.from(keynum.map((b, i) => b ^ stream[i]));
  } else if (kdf !== "\0\0") {
    throw new Error(`unknown key encryption ${JSON.stringify(kdf)}`);
  }
  const keyId = keynum.subarray(0, 8);
  const secret = keynum.subarray(8, 72);                            // Ed25519 seed (32) + public key (32)
  const key = createPrivateKey({ key: { kty: "OKP", crv: "Ed25519", d: b64url(secret.subarray(0, 32)), x: b64url(secret.subarray(32)) }, format: "jwk" });
  // a wrong password gives a key whose public half doesn't match the stored one
  const derived = createPublicKey(key).export({ format: "jwk" }).x;
  if (derived !== b64url(secret.subarray(32))) throw new Error("could not unlock the signing key (wrong password?)");
  return { key, keyId, publicKey: secret.subarray(32) };
}

/** Writes `<file>.sig` (Tauri's format: base64 of the minisign signature) with `version` in its trusted comment. */
export function signUpdate(file, version, keyFile) {
  const { key, keyId } = loadSecretKey(keyFile);
  const hash = createHash("blake2b512").update(readFileSync(file)).digest();
  const signature = sign(null, hash, key);
  const trusted = `timestamp:${Math.floor(Date.now() / 1000)}\tfile:${basename(file)}\tversion:${version}`;
  const global = sign(null, Buffer.concat([signature, Buffer.from(trusted)]), key);
  const text = `untrusted comment: signature from tauri secret key\n${Buffer.concat([Buffer.from("ED"), keyId, signature]).toString("base64")}\n`
    + `trusted comment: ${trusted}\n${global.toString("base64")}\n`;
  writeFileSync(`${file}.sig`, Buffer.from(text).toString("base64"));
  return trusted;
}

/** Checks `<file>.sig` against the app's public key (tauri.conf.json's updater pubkey): what the updater will do. */
export function verifyUpdate(file, pubkeyB64) {
  const pub = Buffer.from(Buffer.from(pubkeyB64, "base64").toString("utf8").split(/\r?\n/u)[1].trim(), "base64");
  const lines = Buffer.from(readFileSync(`${file}.sig`, "utf8").trim(), "base64").toString("utf8").split(/\r?\n/u);
  const sigRaw = Buffer.from(lines[1], "base64");
  const trusted = lines[2].replace(/^trusted comment: /u, "");
  const global = Buffer.from(lines[3], "base64");
  if (!sigRaw.subarray(2, 10).equals(pub.subarray(2, 10))) throw new Error("signed with a different key");
  const pk = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: b64url(pub.subarray(10)) }, format: "jwk" });
  const hash = createHash("blake2b512").update(readFileSync(file)).digest();
  const ok = verify(null, hash, pk, sigRaw.subarray(10)) && verify(null, Buffer.concat([sigRaw.subarray(10), Buffer.from(trusted)]), pk, global);
  if (!ok) throw new Error("signature does not verify");
  return trusted;
}
