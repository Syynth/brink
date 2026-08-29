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

import { readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * @typedef {{ signature: string, url: string }} PlatformEntry
 * @typedef {{ version: string, notes: string, pub_date: string,
 *   platforms: Record<string, PlatformEntry> }} UpdateManifest
 */

/**
 * Tauri platform key for an updater payload filename, or null when the file
 * is not an updater payload (a `.dmg`, a `.deb`, a stray `.sig`).
 *
 * Only the payload extensions Tauri's updater actually consumes are mapped:
 * macOS ships `.app.tar.gz`, Linux `.AppImage`, Windows the NSIS `.zip`.
 * A `.dmg`/`.deb` is an INSTALLER, not an update payload — including one
 * here would produce a manifest the updater cannot apply.
 *
 * ⚠ The macOS payload is named `Brink Studio.app.tar.gz` — no architecture
 * marker at all (observed from a real `tauri build`, not assumed). That is
 * why the arch test below DEFAULTS to aarch64 rather than reading the name:
 * it is correct only while the macOS matrix is ARM-only. Adding an Intel mac
 * lane would have BOTH lanes emit that same bare filename, and the release
 * job's `download-artifact` runs with `merge-multiple: true` — so one would
 * silently overwrite the other and the manifest would ship one arch's binary
 * under both keys. Give the lanes distinct artifact names before adding one.
 *
 * @param {string} filename
 * @returns {string | null}
 */
export function platformKeyFor(filename) {
  if (filename.endsWith(".sig")) return null;
  if (filename.endsWith(".app.tar.gz")) {
    return filename.includes("x64") || filename.includes("x86_64")
      ? "darwin-x86_64"
      : "darwin-aarch64";
  }
  if (filename.endsWith(".AppImage")) return "linux-x86_64";
  // Windows: tauri v2's NSIS updater payload is the SETUP EXE ITSELF, not a
  // `.nsis.zip` (that was v1). Observed from the first Windows build this
  // repo has ever run — run 32588025582 emitted
  // `Brink Studio_0.1.0_x64-setup.exe` plus a `.sig`, and nothing else.
  //
  // `.nsis.zip` is still accepted so a config or toolchain that emits the
  // older shape does not silently lose Windows. That is the whole hazard
  // here: an unmatched payload is not an error, it is an ABSENT platform
  // key, and an absent key means every Windows client polls forever and is
  // told it is up to date. `assertPlatformsCovered` below is what turns that
  // silence into a failure.
  if (filename.endsWith(".nsis.zip") || filename.endsWith(".exe")) {
    return filename.includes("arm64") || filename.includes("aarch64")
      ? "windows-aarch64"
      : "windows-x86_64";
  }
  return null;
}

/**
 * Fail when the manifest is missing a platform whose artifacts are present.
 *
 * The mapping's failure mode is silence: an unrecognised payload filename
 * yields no key, the manifest is still valid JSON, the release still
 * publishes, and every client on that platform is told it is up to date
 * forever. That is exactly what `.nsis.zip` vs `-setup.exe` would have
 * caused. So rather than trusting the mapping, infer what SHOULD be there
 * from the signed payloads actually on disk and demand they all landed.
 *
 * @param {Record<string, PlatformEntry>} platforms
 * @param {string[]} files
 */
export function assertPlatformsCovered(platforms, files) {
  const signed = new Set(
    files.filter((f) => f.endsWith(".sig")).map((f) => f.slice(0, -".sig".length)),
  );
  // An installer that is not an update payload (.dmg has no .sig; .deb has
  // one but cannot self-update under /usr) is not expected in the manifest.
  const expectedButMissing = [...signed].filter(
    (f) => !f.endsWith(".deb") && platformKeyFor(f) === null,
  );
  if (expectedButMissing.length > 0) {
    throw new Error(
      `these signed updater payloads matched no platform key, so the platforms ` +
        `they belong to would be MISSING from latest.json and their users would ` +
        `never be offered an update: ${expectedButMissing.join(", ")}. Add the ` +
        `filename shape to platformKeyFor rather than shipping a manifest that ` +
        `silently covers fewer platforms than were built.`,
    );
  }
}

/**
 * Build the manifest object from a directory of updater artifacts.
 *
 * Throws when a payload has no adjacent `.sig`: an unsigned entry would be
 * rejected by every client at verification time, so emitting it would ship a
 * manifest that looks complete and updates nobody.
 *
 * `readDir`/`readFile` are injected so the mapping is testable without a
 * real bundle directory; they are typed narrowly (rather than inheriting
 * `readdirSync`'s overload set) so a consumer's stub typechecks.
 *
 * @param {object} options
 * @param {string} options.dir
 * @param {string} options.version
 * @param {string} options.tag
 * @param {string} options.repo
 * @param {string} [options.notes]
 * @param {string} [options.pubDate]
 * @param {(dir: string) => string[]} [options.readDir]
 * @param {(path: string) => string} [options.readFile]
 * @returns {UpdateManifest}
 */
export function buildManifest({ dir, version, tag, repo, notes = "", pubDate, readDir = readdirSync, readFile = (p) => readFileSync(p, "utf8") }) {
  /** @type {Record<string, PlatformEntry>} */
  const platforms = {};
  const entries = readDir(dir).sort();
  for (const file of entries) {
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
  assertPlatformsCovered(platforms, entries);
  return { version, notes, pub_date: pubDate, platforms };
}

// Compared as REAL paths. `import.meta.url` is symlink-resolved by Node
// while `process.argv[1]` is not, so on macOS — where `/var` is a symlink
// to `/private/var` — a script run from anywhere under `$TMPDIR` compared
// unequal and this guard silently did not fire. That is the worst shape a
// safety check can fail in: `tauri.conf.json`'s `beforeBundleCommand` runs
// this file directly, and an inert guard ships whatever it was meant to
// stop. Wrapped because `realpathSync` throws on a path that no longer
// exists.
const invokedDirectly = (() => {
  if (!process.argv[1]) return false;
  try {
    return realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
  } catch {
    return false;
  }
})();
if (invokedDirectly) {
  const [dir, version, tag, repo, out] = process.argv.slice(2);
  if (!dir || !version || !tag || !repo || !out) {
    console.error("usage: build-update-manifest.mjs <dir> <version> <tag> <repo> <out>");
    process.exit(1);
  }
  const manifest = buildManifest({ dir, version, tag, repo, pubDate: new Date().toISOString() });
  writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`[build-update-manifest] ${out}: ${Object.keys(manifest.platforms).join(", ")}`);
}
