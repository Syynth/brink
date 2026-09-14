// Assemble the OTA web-bundle manifest (`bundle-latest.json`) the desktop app
// polls (docs/desktop-ota-spec.md Stage 2).
//
// The shell fetches ONE JSON document describing the newest web bundle and
// where its archive lives. This turns a built archive plus the checked-in
// descriptor into that document.
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

/**
 * Build the manifest document.
 *
 * `descriptor` is `ota-bundle.json` — hand-maintained, and the source of the
 * two values no build step can compute:
 *
 * - **version** is an independent sequence, deliberately not the app's
 *   (RULED 2026-09-14). Reusing the app version would make two web-only
 *   updates between signed releases impossible: the second would carry the
 *   same version as the first and the shell would report "up to date".
 * - **minShellVersion** is a judgement about the IPC surface, guarded
 *   against going stale by `min_shell_version_is_reconsidered_when_the_ipc_surface_changes`
 *   in `src-tauri/src/lib.rs`.
 *
 * @param {object} options
 * @param {{version: string, minShellVersion: string}} options.descriptor
 *   `ota-bundle.json`, parsed.
 * @param {Uint8Array} options.archive The `.tar.gz` bytes.
 * @param {string} options.filename The archive's published filename.
 * @param {string} options.signature Base64 minisign signature of `archive`.
 * @param {string} options.repo `owner/name`.
 * @param {string} options.tag The release the archive is published under.
 * @param {string} [options.commit] Git SHA, for traceability back to source.
 * @param {() => Date} [options.now] Injected clock, so `pubDate` is testable.
 * @returns {BundleManifest}
 */
export function buildBundleManifest({
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
  if (!(archive instanceof Uint8Array) || archive.length === 0) {
    throw new Error("the archive must be non-empty bytes");
  }
  // A missing signature would produce a manifest the shell refuses on every
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
    minShellVersion: descriptor.minShellVersion,
    url: archiveUrl(repo, tag, filename),
    sha256: sha256Hex(archive),
    signature: signature.trim(),
    pubDate: now().toISOString(),
    ...(commit === undefined ? {} : { commit }),
  };
}

/** Standalone: `node build-bundle-manifest.mjs <descriptor> <archive> <sig> <repo> <tag> <out>` */
function main(argv) {
  const [descriptorPath, archivePath, signaturePath, repo, tag, outPath] = argv;
  const descriptor = JSON.parse(readFileSync(descriptorPath, "utf8"));
  const archive = readFileSync(archivePath);
  const manifest = buildBundleManifest({
    descriptor,
    archive: new Uint8Array(archive),
    filename: archivePath.split("/").at(-1),
    signature: readFileSync(signaturePath, "utf8"),
    repo,
    tag,
    commit: process.env.GITHUB_SHA,
  });
  writeFileSync(outPath, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`wrote ${outPath}`);
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
