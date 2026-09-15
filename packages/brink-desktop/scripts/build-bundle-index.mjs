// Assemble the OTA web-bundle index (`bundle-index.json`) the desktop app
// polls (docs/desktop-ota-spec.md Stage 4).
//
// ONE document lists every published bundle, each entry carrying its own
// version, channel, url, hash, signature and minShellVersion. That single
// fetch then answers all three questions a policy can ask: the newest entry
// on my channel, the list a picker renders, and the url a pinned version
// resolves to.
//
// It REPLACES Stage 2's `bundle-latest.json`, which could answer only the
// first. Appending rather than overwriting is what makes the other two
// possible, so this script reads the current index and merges into it.
//
// ⚠ What this does NOT do is sign. The signature is produced by the Tauri
// signer with TAURI_SIGNING_PRIVATE_KEY (one keypair across both channels,
// RULED 2026-09-14) and passed in — keeping the key out of this script means
// the logic below is exercisable by `node --test` with no secret present,
// which is the only reason it can be tested at all.
//
// Logic is exported and the standalone run sits behind the main-guard idiom
// (#2478) every script in this directory carries.

import { createHash } from "node:crypto";
import { readFileSync, realpathSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

/**
 * @typedef {{ version: string, minShellVersion: string, url: string,
 *   sha256: string, signature: string, pubDate: string, commit?: string }} BundleManifest
 */

/** Lowercase hex SHA-256, the encoding `bundle_update::hash_matches` compares. */
export function sha256Hex(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/**
 * Where a bundle archive is published.
 *
 * The archive lives on its own `bundle-v*` release while the MANIFEST is
 * republished to the `desktop-latest` alias — the same split `latest.json`
 * already uses, so the app polls one stable URL and each archive keeps an
 * immutable one.
 */
export function archiveUrl(repo, tag, filename) {
  return `https://github.com/${repo}/releases/download/${tag}/${filename}`;
}

/** Channels an entry may declare. */
export const CHANNELS = ["stable", "beta"];

/**
 * How many entries the published index keeps.
 *
 * Bounded per the repo's standing guard against unbounded growth, and the
 * bound belongs HERE rather than in the client: a shell that trimmed on read
 * would still have paid to download the whole thing. Fifty is far more
 * history than a picker shows and still a ~15 KB document.
 */
export const MAX_INDEX_ENTRIES = 50;

/**
 * Build one index entry from a built archive.
 *
 * `descriptor` is `ota-bundle.json` — hand-maintained, and the source of the
 * values no build step can compute:
 *
 * - **version** is an independent sequence, deliberately not the app's
 *   (RULED 2026-09-14). Reusing the app version would make two web-only
 *   updates between signed releases impossible: the second would carry the
 *   same version as the first and the shell would report "up to date".
 * - **minShellVersion** is a judgement about the IPC surface, guarded
 *   against going stale by `min_shell_version_is_reconsidered_when_the_ipc_surface_changes`
 *   in `src-tauri/src/lib.rs`.
 * - **channel** decides who is offered this bundle at all. Validated against
 *   a closed list rather than passed through: a typo'd channel would parse
 *   on the Rust side as the serde default (stable) and ship a beta to every
 *   stable install.
 *
 * @param {object} options
 * @param {{version: string, minShellVersion: string, channel?: string | null}} options.descriptor
 * @param {Uint8Array} options.archive The `.tar.gz` bytes.
 * @param {string} options.filename The archive's published filename.
 * @param {string} options.signature Base64 minisign signature of `archive`.
 * @param {string} options.repo `owner/name`.
 * @param {string} options.tag The release the archive is published under.
 * @param {string} [options.commit] Git SHA, for traceability back to source.
 * @param {() => Date} [options.now] Injected clock, so `pubDate` is testable.
 */
export function buildIndexEntry({
  descriptor,
  archive,
  filename,
  signature,
  repo,
  tag,
  commit,
  now = () => new Date(),
}) {
  for (const field of ["version", "minShellVersion"]) {
    if (typeof descriptor?.[field] !== "string" || descriptor[field] === "") {
      throw new Error(`ota-bundle.json must declare a non-empty ${field}`);
    }
  }
  const channel = descriptor.channel ?? "stable";
  if (!CHANNELS.includes(channel)) {
    throw new Error(
      `ota-bundle.json declares channel ${JSON.stringify(channel)}; expected one of ` +
        `${CHANNELS.join(", ")}. An unrecognised channel would read as the default on the ` +
        `shell side and ship this bundle to every stable install.`,
    );
  }
  if (!(archive instanceof Uint8Array) || archive.length === 0) {
    throw new Error("the archive must be non-empty bytes");
  }
  // A missing signature would produce an entry the shell refuses on every
  // install — silently, since a refusal looks like any other failed check.
  // Better to fail the release job.
  if (typeof signature !== "string" || signature.trim() === "") {
    throw new Error(
      "the archive signature is empty — TAURI_SIGNING_PRIVATE_KEY was probably not set, " +
        "and an unsigned bundle is refused by every install",
    );
  }

  return {
    version: descriptor.version,
    channel,
    minShellVersion: descriptor.minShellVersion,
    url: archiveUrl(repo, tag, filename),
    sha256: sha256Hex(archive),
    signature: signature.trim(),
    pubDate: now().toISOString(),
    ...(commit === undefined ? {} : { commit }),
  };
}

/**
 * Merge `entry` into the published index, newest first.
 *
 * Three behaviours worth stating, because each is a bug if it goes the other
 * way:
 *
 * - **Re-publishing a version REPLACES its entry** rather than appending a
 *   second. Two entries for one version would make "latest" depend on scan
 *   order, and the shell resolves the first match.
 * - **A malformed existing document is treated as empty rather than
 *   inherited.** Merging into something unparseable would either throw on
 *   every future release or silently carry the damage forward; starting
 *   fresh loses history, which is recoverable, instead of the channel, which
 *   is not.
 * - **The cap drops the OLDEST PUBLISHED, not the lowest version.** History
 *   is about what shipped when, and a hotfix to an old line should not evict
 *   a newer entry.
 */
export function mergeIntoIndex(existing, entry, { maxEntries = MAX_INDEX_ENTRIES } = {}) {
  const previous = Array.isArray(existing?.entries) ? existing.entries : [];
  const kept = previous.filter(
    (candidate) => candidate && candidate.version !== entry.version,
  );
  return { entries: [entry, ...kept].slice(0, maxEntries) };
}

/**
 * Standalone:
 * `node build-bundle-index.mjs <descriptor> <archive> <sig> <repo> <tag> <existing|-> <out>`
 *
 * `<existing>` is the currently-published index, or `-` when there is none
 * yet (the first release, or a fetch that 404'd). Passing the path rather
 * than fetching here keeps this script network-free and therefore testable.
 */
function main(argv) {
  const [descriptorPath, archivePath, signaturePath, repo, tag, existingPath, outPath] = argv;
  const descriptor = JSON.parse(readFileSync(descriptorPath, "utf8"));
  const archive = readFileSync(archivePath);
  const entry = buildIndexEntry({
    descriptor,
    archive: new Uint8Array(archive),
    filename: archivePath.split("/").at(-1),
    signature: readFileSync(signaturePath, "utf8"),
    repo,
    tag,
    commit: process.env.GITHUB_SHA,
  });

  let existing = { entries: [] };
  if (existingPath && existingPath !== "-") {
    try {
      existing = JSON.parse(readFileSync(existingPath, "utf8"));
    } catch (error) {
      // Loud, but not fatal — see mergeIntoIndex's note on why starting
      // fresh beats inheriting damage.
      console.warn(`[build-bundle-index] could not read ${existingPath}: ${error.message}`);
    }
  }

  const index = mergeIntoIndex(existing, entry);
  writeFileSync(outPath, `${JSON.stringify(index, null, 2)}\n`);
  console.log(`wrote ${outPath} (${index.entries.length} entries)`);
}

// The main-guard (#2478), in the form every script in this directory uses.
// It compares REAL paths: comparing `import.meta.url` to
// `pathToFileURL(argv[1])` directly fails on macOS, where `/var` is a
// symlink to `/private/var`, so the guard silently does not fire. The
// `!process.argv[1]` arm is load-bearing — with no script path at all
// (`node -e`, which is how the inert-on-import test loads this module)
// `realpathSync(undefined)` throws, and a guard without it makes importing
// the module fail instead of doing nothing.
const invokedDirectly = (() => {
  if (!process.argv[1]) return false;
  try {
    return realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
  } catch {
    return false;
  }
})();
if (invokedDirectly) {
  main(process.argv.slice(2));
}
