// Repo-wide guard: no literal NUL byte in any TS/TSX workspace source file
// (#2737, follow-up from #2558/#2733).
//
// The gap. `packages/ink-editor/src/rename.ts` shipped a cache-key separator
// built as a literal NUL byte (`` `${a}\x00${b}` ``, chosen so the composite
// key could never collide with either half alone) — #2558/#2733 fixed it.
// PR #2733's own review then found the IDENTICAL defect still live one
// package over, in `packages/studio-shell/src/document.ts`'s
// `documentKey()`, and nothing stopped a third instance from needing its own
// retro cycle to surface (#2737).
//
// Why hand review keeps missing this. A literal NUL byte makes `file`
// classify the file as `data`, and it makes `grep`/`rg` (without `-a`)
// classify it BINARY — `grep -n "documentKey" document.ts` prints
// "binary file matches" and exits 0, showing no line, no match, no hit.
// Every ordinary repo-wide text sweep (grep, rg, an editor's find-in-files)
// silently skips a NUL-bearing file rather than flagging it. The failure is
// invisible by construction; only a byte-level check that reads files as
// bytes — never as a grep pattern — can see it. This mirrors the exact
// lesson `scripts/check-grammar-drift.mjs`'s header draws for a different
// defect class: hand enumeration of "which files have this problem" failed
// four rounds running there; this repo does not get a fifth chance on a
// defect that is silent by construction on top of that.
//
// Scope: `packages/*/src` (case-insensitive-safe: `TypeScript` name
// convention throughout this repo is `.ts`/`.tsx`), matching the issue's own
// framing ("no NUL bytes in any workspace `src/` tree") and the established
// precedent of `no-test-file-imports.test.ts` (#2516), which scans the same
// `packages/*/src` tree for a different byte-string defect class. A
// repo-wide sweep over ALL git-tracked files (verified by hand while
// building this guard) additionally finds legitimate NUL-bearing binaries
// that must NOT be flagged: `.inkb` bytecode fuzz seeds under
// `crates/*/fuzz/seeds/`, a `.png` icon
// (`packages/brink-desktop/src-tauri/icons/icon.png`), and three `.inkl`
// fixture files under `tests/tier3/lists/tower-of-hanoi/`. None of those
// live under `packages/*/src`, so scoping there is exact for the real repo
// today — verified, not assumed (`git ls-files 'packages/*/src/*'` today
// carries only `.ts`/`.tsx`/`.css`/`.rs`/`.txt` extensions, no binary asset
// type, so scanning every file under that scope rather than filtering by
// extension carries no known false-positive risk).
//
// This lives in scripts/ (run by `pnpm test:scripts`, CI's `frontend` job,
// the non-recursive `scripts/*.test.mjs` glob) rather than as a vitest test
// inside a package, so it rides CI automatically the same way
// check-pnpm-pin.mjs and check-grammar-drift.mjs do — no separate wiring
// needed, and it catches the defect before `pnpm install` even runs.
//
// Exported as pure functions over an already-read buffer (or a directory
// scan) so check-no-nul-bytes.test.mjs can drive them with synthetic planted
// NUL bytes; the CLI at the bottom applies them to the real repo.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");
export const PACKAGES_DIR = "packages";

/**
 * List `packages/<name>` directories (repo-relative), sorted for
 * determinism. Mirrors `no-test-file-imports.test.ts`'s `listPackages`.
 *
 * @param {string} packagesDir absolute path to the packages/ directory
 * @returns {string[]} directory names (not full paths)
 */
export function listPackages(packagesDir) {
  let entries;
  try {
    entries = readdirSync(packagesDir);
  } catch {
    return [];
  }
  return entries
    .filter((name) => {
      if (name.startsWith(".")) return false;
      try {
        return statSync(join(packagesDir, name)).isDirectory();
      } catch {
        return false;
      }
    })
    .sort();
}

/**
 * Every file under `srcDir`, walked recursively, any extension — this is a
 * byte-level property check, not a syntactic one, so it deliberately does
 * NOT filter by extension (see file header: verified no binary asset type
 * lives under a package's src/ tree today).
 *
 * @param {string} srcDir absolute path
 * @returns {string[]} absolute file paths, sorted
 */
export function listFilesRecursive(srcDir) {
  const found = [];

  const walk = (dir) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const child = join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(child);
      } else if (entry.isFile()) {
        found.push(child);
      }
    }
  };

  walk(srcDir);
  return found.sort();
}

/**
 * The byte-level check itself: does `buffer` contain a literal NUL (0x00)
 * byte, and if so at what offset? Operates on a `Buffer`, never on a decoded
 * string or a regex over text — a JS string built from `readFileSync(path,
 * "utf8")` can lose or mangle a raw NUL depending on the source encoding, so
 * this always reads as bytes.
 *
 * @param {Buffer} buffer
 * @returns {{hasNul: boolean, offset: number}} offset is -1 when absent
 */
export function findFirstNulByte(buffer) {
  const offset = buffer.indexOf(0);
  return { hasNul: offset !== -1, offset };
}

/**
 * @typedef {{ path: string, offset: number }} NulOffense
 */

/**
 * Scan every file under `packages/*\/src` for a literal NUL byte.
 *
 * @param {{ repoRoot?: string }} [options]
 * @returns {{ ok: boolean, offenses: NulOffense[], filesScanned: number }}
 */
export function checkNoNulBytes({ repoRoot = REPO_ROOT } = {}) {
  const packagesDir = join(repoRoot, PACKAGES_DIR);
  const offenses = [];
  let filesScanned = 0;

  for (const pkg of listPackages(packagesDir)) {
    const srcDir = join(packagesDir, pkg, "src");
    for (const file of listFilesRecursive(srcDir)) {
      filesScanned += 1;
      const buffer = readFileSync(file);
      const { hasNul, offset } = findFirstNulByte(buffer);
      if (hasNul) {
        offenses.push({ path: file.slice(repoRoot.length + 1), offset });
      }
    }
  }

  return { ok: offenses.length === 0, offenses, filesScanned };
}

const invokedDirectly =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const result = checkNoNulBytes();
  if (result.ok) {
    console.log(
      `ok - no literal NUL bytes found in any packages/*/src file (${result.filesScanned} files scanned).`,
    );
  } else {
    console.error("NUL-byte cache-key guard FAILED (#2737):");
    for (const offense of result.offenses) {
      console.error(
        `  - ${offense.path}: literal NUL byte at offset ${offense.offset}. A grep/rg sweep without ` +
          `-a silently skips this file (it classifies as binary) — see scripts/check-no-nul-bytes.mjs's ` +
          `header. If this is source code building a composite in-memory cache key, use a printable, ` +
          `collision-free separator instead (e.g. JSON.stringify([...fields]) for a fixed-arity ` +
          `composite key, the fix shape #2733 used). If this is a legitimately binary asset (an image, ` +
          `font, or other non-text file) checked in under packages/*/src, either move it out of src/ ` +
          `(this guard scans only packages/*/src) or extend this guard's scope/exclusions to allow it — ` +
          `do not add a printable separator to a binary format that isn't yours to edit.`,
      );
    }
    process.exitCode = 1;
  }
}
