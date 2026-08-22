// Assemble the updater manifest (`latest.json`) the desktop app polls (D4,
// docs/desktop-shell-spec.md).
//
// Tauri's updater fetches ONE JSON document describing the newest release and
// where each platform's payload lives. `tauri build` with
// `createUpdaterArtifacts: true` emits the payloads and a detached `.sig`
// beside each; this turns a directory of those into the manifest.
//
// The mapping from filename to Tauri platform key is the part worth testing:
// get it wrong and the app either never sees an update (key absent) or is
// handed the wrong architecture's payload. Both fail quietly — the app just
// reports "up to date" forever — which is exactly the class of silent
// failure this repo keeps getting bitten by, so it is a tested function
// rather than inline shell in a workflow.
//
// Logic is exported and the standalone run sits behind the main-guard idiom
// (#2478) that every script in this directory carries.

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/**
 * Tauri platform key for an updater payload filename, or null when the file
 * is not an updater payload (a `.dmg`, a `.deb`, a stray `.sig`).
 *
 * Only the payload extensions Tauri's updater actually consumes are mapped:
 * macOS ships `.app.tar.gz`, Linux `.AppImage`, Windows the NSIS `.zip`.
 * A `.dmg`/`.deb` is an INSTALLER, not an update payload — including one
 * here would produce a manifest the updater cannot apply.
 */
export function platformKeyFor(filename) {
  if (filename.endsWith(".sig")) return null;
  if (filename.endsWith(".app.tar.gz")) {
    return filename.includes("x64") || filename.includes("x86_64")
      ? "darwin-x86_64"
      : "darwin-aarch64";
  }
  if (filename.endsWith(".AppImage")) return "linux-x86_64";
  if (filename.endsWith(".nsis.zip")) return "windows-x86_64";
  return null;
}

/**
 * Build the manifest object from a directory of updater artifacts.
 *
 * Throws when a payload has no adjacent `.sig`: an unsigned entry would be
 * rejected by every client at verification time, so emitting it would ship a
 * manifest that looks complete and updates nobody.
 */
export function buildManifest({ dir, version, tag, repo, notes = "", pubDate, readDir = readdirSync, readFile = (p) => readFileSync(p, "utf8") }) {
  const platforms = {};
  for (const file of readDir(dir).sort()) {
    const key = platformKeyFor(file);
    if (key === null) continue;
    let signature;
    try {
      signature = readFile(join(dir, `${file}.sig`)).trim();
    } catch {
      throw new Error(
        `updater payload ${file} has no adjacent ${file}.sig — every client ` +
          `would reject it, so the manifest is not emitted. Was the build run ` +
          `without TAURI_SIGNING_PRIVATE_KEY?`,
      );
    }
    platforms[key] = {
      signature,
      url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(file)}`,
    };
  }
  if (Object.keys(platforms).length === 0) {
    throw new Error(`no updater payloads found in ${dir} — refusing to emit an empty manifest`);
  }
  return { version, notes, pub_date: pubDate, platforms };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [dir, version, tag, repo, out] = process.argv.slice(2);
  if (!dir || !version || !tag || !repo || !out) {
    console.error("usage: build-update-manifest.mjs <dir> <version> <tag> <repo> <out>");
    process.exit(1);
  }
  const manifest = buildManifest({ dir, version, tag, repo, pubDate: new Date().toISOString() });
  writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`[build-update-manifest] ${out}: ${Object.keys(manifest.platforms).join(", ")}`);
}
