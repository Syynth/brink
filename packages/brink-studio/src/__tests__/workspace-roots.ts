/**
 * Ties `save-path-enrolment.test.ts`'s scan roots to `pnpm-workspace.yaml`
 * (issue #2515, hole 2).
 *
 * Before this module, `discoverCallSiteFiles` (in `save-path-enrolment.test.ts`)
 * hand-derived its scan root as `resolve(here, "../../..")` and walked every
 * entry under it — correct only because `pnpm-workspace.yaml`'s `packages:`
 * glob happens to be `packages/*` today. A future workspace glob (e.g. an
 * added `apps/*` root, or a package whose sources moved outside `packages/`)
 * would widen the real TS workspace with no corresponding change to that
 * scan, so a new save path outside `packages/*\/src` would enrol nowhere and
 * fail nothing — the one hole the #2480 guard could not make loud (PR #2510
 * review, "Scope gaps", item 2).
 *
 * This module makes the scan roots a DERIVATION of `pnpm-workspace.yaml`
 * rather than a second hand-maintained assumption about it, so the two
 * cannot silently drift apart — the same "derive, don't duplicate" shape
 * `save-path-enrolment.test.ts`'s own header already uses for its call-site
 * scan (`SCANNED_FILES` is checked against a live re-scan, never trusted).
 *
 * Deliberately narrow: it understands exactly one glob shape, `"<dir>/*"`
 * (the only shape `pnpm-workspace.yaml` uses today). Anything else —
 * negation (`!packages/x`), an exact path with no `/*`, a mapping form —
 * throws rather than silently ignoring it, so a workspace-layout change this
 * module cannot express fails the suite loudly and asks a human to teach it
 * the new shape, instead of quietly scanning less than the real workspace.
 *
 * ⚠ pnpm-workspace.yaml carries a pointer comment back to this module and to
 * {@link import("./save-path-enrolment.test.js")} — keep both in sync with
 * any glob shape added there.
 */

import { readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const GLOB_ENTRY = /^\s*-\s*(.+?)\s*$/;
/** The only glob shape this module understands: a bare directory + `/*`. */
const SIMPLE_STAR_GLOB = /^[\w./-]+\/\*$/;

function unquote(raw: string): string {
  if (raw.length >= 2) {
    const first = raw[0];
    const last = raw[raw.length - 1];
    if ((first === '"' && last === '"') || (first === "'" && last === "'")) {
      return raw.slice(1, -1);
    }
  }
  return raw;
}

/**
 * Parses `pnpm-workspace.yaml`'s `packages:` list into its glob strings
 * (e.g. `["packages/*"]`). Throws — rather than skipping or guessing — on
 * any entry that is not a simple `"<dir>/*"` glob, and on a missing or
 * empty `packages:` block, so a workspace shape this parser cannot express
 * is a loud failure, not a silent undercount.
 */
export function parseWorkspacePackageGlobs(yamlText: string): string[] {
  const lines = yamlText.split("\n");
  const packagesIdx = lines.findIndex((line) => line.trim() === "packages:");
  if (packagesIdx === -1) {
    throw new Error('pnpm-workspace.yaml has no top-level "packages:" key');
  }

  const globs: string[] = [];
  for (let i = packagesIdx + 1; i < lines.length; i += 1) {
    const line = lines[i];
    if (line.trim() === "" || line.trim().startsWith("#")) continue;
    const match = GLOB_ENTRY.exec(line);
    if (match === null) break; // end of the packages: block (dedented / next key)
    const entry = unquote(match[1]);
    if (!SIMPLE_STAR_GLOB.test(entry)) {
      throw new Error(
        `pnpm-workspace.yaml: unsupported "packages:" entry ${JSON.stringify(match[1])} — ` +
          'save-path-enrolment.test.ts\'s scan-root derivation understands only simple ' +
          '"<dir>/*" globs; a negated, exact-path, or mapping entry needs ' +
          "workspace-roots.ts's parser taught about it explicitly before the scan can trust it",
      );
    }
    globs.push(entry);
  }

  if (globs.length === 0) {
    throw new Error('pnpm-workspace.yaml\'s "packages:" list parsed to zero entries');
  }
  return globs;
}

/**
 * Resolves parsed `"<dir>/*"` globs to the directories they match under
 * `repoRoot`, sorted. A glob whose `<dir>` does not exist contributes no
 * roots rather than throwing — a workspace glob is allowed to name a
 * not-yet-populated directory.
 */
export function deriveScanRoots(globs: string[], repoRoot: string): string[] {
  const roots: string[] = [];
  for (const glob of globs) {
    const dir = glob.slice(0, glob.length - "/*".length);
    const base = resolve(repoRoot, dir);
    let entries: string[];
    try {
      entries = readdirSync(base);
    } catch {
      continue; // the glob's directory does not exist — nothing to scan under it
    }
    for (const entry of entries) {
      const full = join(base, entry);
      try {
        if (statSync(full).isDirectory()) roots.push(full);
      } catch {
        continue; // raced away between readdir and stat — not a package dir either way
      }
    }
  }
  return roots.sort();
}
