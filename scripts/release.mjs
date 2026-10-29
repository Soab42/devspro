#!/usr/bin/env node
// Release steps for DevsPro's in-app updater. See docs/PACKAGING.md, "Releasing an update".
//
//   node scripts/release.mjs version 0.2.0   bump every version field, date CHANGELOG's Unreleased section
//   node scripts/release.mjs feed            sign this machine's bundles, write site/latest.json + changelog.json
//   node scripts/release.mjs changelog       only rewrite site/changelog.json
//   node scripts/release.mjs publish         create (or add to) the GitHub release v<version>
//
// `feed` runs on each platform's build machine: Linux adds the .deb and .rpm, Windows the
// setup .exe. Entries for the same version are merged, so the order doesn't matter.
// Nothing here pushes; site/latest.json reaches installed copies when it's pushed to main.

import { createHash, createPublicKey, verify } from "node:crypto";
import { existsSync, readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const REPO = "Soab42/devspro";
const BUNDLE = join(ROOT, "src-tauri/target/release/bundle");
const PUBLIC_KEY_FILE = join(ROOT, "src-tauri/updater.pub");
const FEED_FILE = join(ROOT, "site/latest.json");
const CHANGELOG_JSON = join(ROOT, "site/changelog.json");
const CHANGELOG_MD = join(ROOT, "CHANGELOG.md");
// Run with node itself rather than through npx: npx is a .cmd shim on Windows,
// which only a shell can start, and nothing here should go through a shell.
const TAURI_CLI = join(ROOT, "node_modules/@tauri-apps/cli/tauri.js");

const read = (p) => readFileSync(p, "utf8");
const readJson = (p) => JSON.parse(read(p));
const writeJson = (p, value) => writeFileSync(p, `${JSON.stringify(value, null, 2)}\n`);

function fail(message) {
  console.error(`release: ${message}`);
  process.exit(1);
}

/** No shell, so file names and the like are never interpreted. */
function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: "inherit", ...opts });
  if (r.status !== 0) fail(`${basename(cmd)} ${args[0] === TAURI_CLI ? args.slice(1, 3).join(" ") : args[0]} failed`);
  return r;
}

/* ----------------------------------------------------------------- versions -- */

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$/;

function compareVersions(a, b) {
  const pa = a.match(SEMVER).slice(1, 4).map(Number);
  const pb = b.match(SEMVER).slice(1, 4).map(Number);
  for (let i = 0; i < 3; i++) if (pa[i] !== pb[i]) return pa[i] - pb[i];
  return 0;
}

function cargoVersion() {
  const m = read(join(ROOT, "src-tauri/Cargo.toml")).match(/\[package\][^[]*?\nversion = "([^"]+)"/);
  return m?.[1];
}

/** The one version everything agrees on, or a failure naming what doesn't. */
function currentVersion() {
  const found = {
    "package.json": readJson(join(ROOT, "package.json")).version,
    "src-tauri/Cargo.toml": cargoVersion(),
    "src-tauri/tauri.conf.json": readJson(join(ROOT, "src-tauri/tauri.conf.json")).version,
  };
  const versions = new Set(Object.values(found));
  if (versions.size !== 1) {
    fail(`the version fields disagree: ${Object.entries(found).map(([f, v]) => `${f}=${v}`).join(", ")}`);
  }
  return [...versions][0];
}

function bumpVersion(next) {
  if (!SEMVER.test(next ?? "")) fail("usage: release.mjs version <x.y.z>");
  const current = currentVersion();
  if (compareVersions(next, current) <= 0) fail(`${next} is not newer than ${current}`);

  const pkgPath = join(ROOT, "package.json");
  const pkg = readJson(pkgPath);
  pkg.version = next;
  writeJson(pkgPath, pkg);

  const lockPath = join(ROOT, "package-lock.json");
  if (existsSync(lockPath)) {
    const lock = readJson(lockPath);
    lock.version = next;
    if (lock.packages?.[""]) lock.packages[""].version = next;
    writeJson(lockPath, lock);
  }

  // Edited in place: re-serialising would reflow the file's inline arrays.
  const confPath = join(ROOT, "src-tauri/tauri.conf.json");
  writeFileSync(confPath, read(confPath).replace(/^(  "version": ")[^"]+"/m, `$1${next}"`));
  if (readJson(confPath).version !== next) fail("couldn't update the version in src-tauri/tauri.conf.json");

  const cargoPath = join(ROOT, "src-tauri/Cargo.toml");
  writeFileSync(cargoPath, read(cargoPath).replace(/(\[package\][^[]*?\nversion = ")[^"]+"/, `$1${next}"`));
  const cargoLock = join(ROOT, "src-tauri/Cargo.lock");
  if (existsSync(cargoLock)) {
    writeFileSync(cargoLock, read(cargoLock).replace(/(name = "devspro"\nversion = ")[^"]+"/, `$1${next}"`));
  }

  const md = read(CHANGELOG_MD);
  const today = new Date().toISOString().slice(0, 10);
  // [ \t], not \s: \s would also take the blank line after the heading.
  if (/^## Unreleased[ \t]*$/m.test(md)) {
    writeFileSync(CHANGELOG_MD, md.replace(/^## Unreleased[ \t]*$/m, `## ${next} — ${today}`));
  } else {
    console.warn(`release: CHANGELOG.md has no "## Unreleased" section; add notes under "## ${next} — ${today}" by hand`);
  }
  console.log(`Version ${current} → ${next}. Next: npm run package, then npm run release:feed.`);
}

/* ---------------------------------------------------------------- changelog -- */

const HEADING = /^## (Unreleased|v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?))(?:\s+[—–-]\s+(\d{4}-\d{2}-\d{2}))?\s*$/;

/** CHANGELOG.md's releases, newest first, each with its raw body and its groups. */
function parseChangelog(md) {
  const releases = [];
  let release = null;
  let group = null;
  for (const line of md.split(/\r?\n/)) {
    const h = line.match(HEADING);
    if (h) {
      release = { version: h[2] ?? "Unreleased", date: h[3] ?? null, groups: [], body: [] };
      releases.push(release);
      group = null;
      continue;
    }
    if (!release) continue;
    release.body.push(line);
    const trimmed = line.trim();
    if (trimmed.startsWith("### ")) {
      group = { title: trimmed.slice(4).trim(), items: [] };
      release.groups.push(group);
    } else if (/^[-*] /.test(trimmed)) {
      if (!group) {
        group = { title: "Changes", items: [] };
        release.groups.push(group);
      }
      group.items.push(trimmed.slice(2).trim());
    } else if (trimmed && group?.items.length) {
      group.items[group.items.length - 1] += ` ${trimmed}`;
    }
  }
  return releases.map((r) => ({ ...r, body: r.body.join("\n").trim() }));
}

function writeChangelogJson() {
  const releases = parseChangelog(read(CHANGELOG_MD))
    .filter((r) => r.groups.some((g) => g.items.length))
    .map(({ version, date, groups }) => ({ version, date, groups: groups.filter((g) => g.items.length) }));
  writeJson(CHANGELOG_JSON, { releases });
  console.log(`Wrote site/changelog.json (${releases.length} releases).`);
}

/* ------------------------------------------------------------------ signing -- */

/** `tauri signer`'s base64 wrapping around minisign's text format. */
const unwrap = (b64) => Buffer.from(b64.trim(), "base64").toString("utf8").split(/\r?\n/);

/**
 * Checks a signature exactly as minisign-verify does in the app and the helper: the key
 * id, the Ed25519 signature over the file's BLAKE2b-512 hash, and the signature over the
 * trusted comment. A key that doesn't match updater.pub fails here, not on users' machines.
 */
function verifySignature(file, signatureB64) {
  const keyBytes = Buffer.from(unwrap(read(PUBLIC_KEY_FILE))[1], "base64");
  const [, sigLine, trustedLine, globalLine] = unwrap(signatureB64);
  const sigBytes = Buffer.from(sigLine, "base64");
  if (keyBytes.length !== 42 || sigBytes.length !== 74) return "malformed key or signature";
  if (sigBytes.subarray(0, 2).toString() !== "ED") return "not a prehashed (ED) signature";
  if (!sigBytes.subarray(2, 10).equals(keyBytes.subarray(2, 10))) return "signed by a different key than src-tauri/updater.pub";
  const key = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: keyBytes.subarray(10).toString("base64url") }, format: "jwk" });
  const digest = createHash("blake2b512").update(readFileSync(file)).digest();
  if (!verify(null, digest, key, sigBytes.subarray(10))) return "the signature doesn't match the file";
  const trusted = Buffer.from(trustedLine.replace(/^trusted comment: /, ""), "utf8");
  if (!verify(null, Buffer.concat([sigBytes.subarray(10), trusted]), key, Buffer.from(globalLine, "base64"))) {
    return "the trusted comment's signature doesn't match";
  }
  return null;
}

function sign(file) {
  const keyPath = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH || join(homedir(), ".tauri", "devspro-updater.key");
  if (!existsSync(keyPath)) fail(`no signing key at ${keyPath} (set TAURI_SIGNING_PRIVATE_KEY_PATH)`);
  if (!existsSync(TAURI_CLI)) fail("the Tauri CLI isn't installed — run npm install");
  // The password stays in the environment, where the signer reads it: on the
  // command line it would show in the process list. Set even when empty, or
  // the signer stops to ask for one.
  const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "" };
  run(process.execPath, [TAURI_CLI, "signer", "sign", "-f", keyPath, file], { stdio: ["ignore", "ignore", "inherit"], env });
  const signature = read(`${file}.sig`).trim();
  const problem = verifySignature(file, signature);
  if (problem) fail(`${basename(file)}: ${problem}`);
  return signature;
}

/* --------------------------------------------------------------------- feed -- */

/** This machine's bundles for `version`, by feed platform key. */
function bundles(version) {
  const candidates = {
    "linux-x86_64-deb": join(BUNDLE, "deb", `DevsPro_${version}_amd64.deb`),
    "linux-x86_64-rpm": join(BUNDLE, "rpm", `DevsPro-${version}-1.x86_64.rpm`),
    "windows-x86_64-nsis": join(BUNDLE, "nsis", `DevsPro_${version}_x64-setup.exe`),
  };
  return Object.fromEntries(Object.entries(candidates).filter(([, file]) => existsSync(file)));
}

const assetUrl = (version, file) => `https://github.com/${REPO}/releases/download/v${version}/${basename(file)}`;

function writeFeed() {
  const version = currentVersion();
  const found = bundles(version);
  if (Object.keys(found).length === 0) fail(`no bundles for ${version} in ${BUNDLE} — run npm run package first`);

  const release = parseChangelog(read(CHANGELOG_MD)).find((r) => r.version === version);
  if (!release?.body) fail(`CHANGELOG.md has no notes under "## ${version} — <date>" (run release.mjs version first)`);

  const previous = existsSync(FEED_FILE) ? readJson(FEED_FILE) : null;
  const sameVersion = previous?.version === version;
  const platforms = sameVersion ? { ...previous.platforms } : {};
  for (const [key, file] of Object.entries(found)) {
    platforms[key] = { signature: sign(file), url: assetUrl(version, file) };
    console.log(`Signed and verified ${basename(file)} → ${key}`);
  }
  writeJson(FEED_FILE, {
    version,
    notes: release.body,
    pub_date: sameVersion && previous.pub_date ? previous.pub_date : new Date().toISOString().replace(/\.\d+Z$/, "Z"),
    platforms,
  });
  console.log(`Wrote site/latest.json for ${version}: ${Object.keys(platforms).join(", ")}.`);
  writeChangelogJson();
  console.log("Next: npm run release:publish, then commit and push site/ — installed copies see the update once Pages deploys it.");
}

/* ------------------------------------------------------------------ publish -- */

function publish() {
  const version = currentVersion();
  const files = Object.values(bundles(version));
  if (files.length === 0) fail(`no bundles for ${version} — run npm run package first`);
  const missing = files.filter((f) => !existsSync(`${f}.sig`));
  if (missing.length) fail(`not signed yet: ${missing.map((f) => basename(f)).join(", ")} — run npm run release:feed first`);
  const assets = files.flatMap((f) => [f, `${f}.sig`]);
  const tag = `v${version}`;
  const exists = spawnSync("gh", ["release", "view", tag, "-R", REPO], { stdio: "ignore" }).status === 0;
  if (exists) {
    run("gh", ["release", "upload", tag, ...assets, "--clobber", "-R", REPO]);
  } else {
    const notes = join(mkdtempSync(join(tmpdir(), "devspro-release-")), "notes.md");
    writeFileSync(notes, parseChangelog(read(CHANGELOG_MD)).find((r) => r.version === version)?.body ?? "");
    run("gh", ["release", "create", tag, ...assets, "--title", `DevsPro ${version}`, "--notes-file", notes, "-R", REPO]);
  }
  console.log(`Release ${tag} has ${files.map((f) => basename(f)).join(", ")}. Now commit and push site/latest.json.`);
}

/* --------------------------------------------------------------------- main -- */

const [command, arg] = process.argv.slice(2);
const commands = { version: () => bumpVersion(arg), feed: writeFeed, changelog: writeChangelogJson, publish };
if (!commands[command]) fail("usage: release.mjs <version x.y.z | feed | changelog | publish>");
commands[command]();
