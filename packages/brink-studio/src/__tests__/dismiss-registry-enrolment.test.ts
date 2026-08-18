/**
 * Static enrolment check for the global "Escape dismisses every registered
 * transient surface" safety net (#279, PR #2760): a mechanical guard that a
 * NEW dismissable surface actually enrols in `dismiss-registry.ts` (issue
 * #2766).
 *
 * PR #2760 wired the 10 then-known context-menu/popover/modal surfaces into
 * `registerDismissible()`. Nothing stopped surface #11 from shipping its own
 * `document`-level `keydown`/`pointerdown` dismiss listener without also
 * calling `registerDismissible()` — silently falling back into the
 * unescapable-menu failure mode #279 was filed for, invisibly (no test
 * failure, no diagnostic — just a menu a future user cannot Escape out of if
 * its own listener is ever orphaned).
 *
 * This follows the same shape as `save-path-enrolment.test.ts` (derive the
 * "did you forget to enrol it" check from production source, not a second
 * hand-maintained list that could itself drift):
 *
 *  1. It derives its scan roots from `pnpm-workspace.yaml` (reusing
 *     {@link import("./workspace-roots.js")}, the same derivation
 *     `save-path-enrolment.test.ts` uses) and keeps only the package roots
 *     that own a `src/dismiss-registry.ts` — today that is exactly
 *     `studio-shell` and `ink-editor`. ⚠ There are TWO independent,
 *     uncoordinated registries (`packages/studio-shell/src/dismiss-registry.ts`
 *     and `packages/ink-editor/src/dismiss-registry.ts`) — Escape only
 *     dismisses surfaces within one package at a time. This guard checks
 *     each package against its OWN registry; it does not unify them (out of
 *     scope for #2766, noted as a known limitation).
 *  2. Within each registry-owning package's `src/` (skipping `__tests__`,
 *     `dist`, `node_modules`), it finds every real
 *     `document.addEventListener("keydown" | "pointerdown", ...)` call — a
 *     source scan, not a hand-typed file list. The result is compared
 *     against {@link EXPECTED_LISTENER_FILES}: a NEW file growing such a
 *     call fails the first `it()` below immediately, before the per-site
 *     checks are reached.
 *  3. A file that imports `registerDismissible` from its OWN
 *     `./dismiss-registry` module AND calls it is enrolled at the MODULE
 *     level — every listener call site in that file is covered, matching
 *     how a real surface enrols today (one `registerDismissible()` call
 *     covering that surface's whole open/close lifecycle, not one call per
 *     listener).
 *  4. A file that is NOT module-enrolled must carry a `DISMISS-NET-EXEMPT`
 *     marker comment (mirroring `SAVE-PATH-EXEMPT`) directly above EACH
 *     qualifying listener call, with a reason — for the cases that are
 *     genuinely not "dismiss a transient surface" (an Escape-cancels-drag- gesture
 *     or Escape-restores-maximize handler that manages transient
 *     interaction/layout STATE, not a floating DOM menu/popover/modal). A
 *     call site with neither an enrolling module nor an exempt marker fails.
 *
 * Proof this guard actually fails on a real violation (issue #2766's
 * requirement): a `document.addEventListener("keydown", ...)` dismiss-shaped
 * listener with no `registerDismissible()` call and no `DISMISS-NET-EXEMPT`
 * marker was temporarily added to `packages/studio-shell/src/overlay.tsx`
 * during development of this guard; the "every qualifying call site is
 * enrolled or marked exempt" check below went red, naming that exact
 * file:line; the listener was then removed. See the PR body for #2766.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, resolve } from "node:path";
import { describe, it, expect } from "vitest";
import { deriveScanRoots, parseWorkspacePackageGlobs } from "./workspace-roots.js";

const here = dirname(fileURLToPath(import.meta.url));
/** `packages/` — this file lives at `packages/brink-studio/src/__tests__/`. */
const packagesRoot = resolve(here, "../../..");
/** The repo root — one level above `packages/`. */
const repoRoot = resolve(packagesRoot, "..");
const workspaceYamlPath = resolve(repoRoot, "pnpm-workspace.yaml");

const REGISTRY_FILE = "dismiss-registry.ts";

/** A real `document`-level dismiss-shaped listener call (not a declaration). */
const LISTENER = /document\.addEventListener\(\s*(["'])(keydown|pointerdown)\1/;
const EXEMPT_MARKER = /^\/\/\s*DISMISS-NET-EXEMPT:\s*(.+)$/;
/** The module-level enrolment import, scoped to the package's OWN registry. */
const ENROLLING_IMPORT =
  /import\s*\{[^}]*\bregisterDismissible\b[^}]*\}\s*from\s*["']\.\/dismiss-registry(?:\.js|\.ts)?["']/;
const ENROLLING_CALL = /\bregisterDismissible\s*\(/;

const SKIP_DIRS = new Set(["__tests__", "dist", "node_modules", ".turbo"]);

/** Recursively list `.ts`/`.tsx` files under `dir`, skipping {@link SKIP_DIRS}. */
function listSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    if (SKIP_DIRS.has(entry)) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...listSourceFiles(full));
    } else if (/\.tsx?$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

/**
 * Every workspace package directory, derived from `pnpm-workspace.yaml`'s
 * `packages:` globs (the same derivation `save-path-enrolment.test.ts` uses),
 * narrowed to the ones that own a `src/dismiss-registry.ts` — i.e. the
 * packages this guard's enrolment rule actually applies to.
 */
function discoverRegistryPackageDirs(): string[] {
  const globs = parseWorkspacePackageGlobs(readFileSync(workspaceYamlPath, "utf8"));
  const allPkgDirs = deriveScanRoots(globs, repoRoot);
  return allPkgDirs
    .filter((pkgDir) => {
      try {
        return statSync(join(pkgDir, "src", REGISTRY_FILE)).isFile();
      } catch {
        return false;
      }
    })
    .sort();
}

interface ListenerSite {
  file: string;
  /** 1-based, for messages a reader can jump to. */
  line: number;
  code: string;
  exemptReason: string | null;
}

/** Every real listener call site in `file`, paired with any exempt marker above it. */
function scanListenerSites(file: string): ListenerSite[] {
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  const sites: ListenerSite[] = [];
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith("//")) continue; // prose about a call, not a call
    if (!LISTENER.test(lines[i])) continue;

    // Walk up through the contiguous comment block directly above the call
    // (no blank or code line in between) looking for the exempt marker.
    let exemptReason: string | null = null;
    for (let j = i - 1; j >= 0; j -= 1) {
      const above = lines[j].trim();
      if (!above.startsWith("//")) break;
      const match = EXEMPT_MARKER.exec(above);
      if (match !== null) {
        exemptReason = match[1];
        break;
      }
    }
    sites.push({ file, line: i + 1, code: trimmed, exemptReason });
  }
  return sites;
}

/** Does `file` enrol at the module level — its OWN registry, imported AND called? */
function isModuleEnrolled(file: string): boolean {
  const text = readFileSync(file, "utf8");
  return ENROLLING_IMPORT.test(text) && ENROLLING_CALL.test(text);
}

/**
 * Every production file today holding a real `document`-level `keydown` /
 * `pointerdown` listener call, per registry-owning package (grep-verified
 * against `main` on 2026-08-18). NOT the source of truth — the `it()` below
 * re-derives this from each package's `src/` on every run and fails the
 * moment the two disagree, so this list going stale is itself caught.
 */
const EXPECTED_LISTENER_FILES: Record<string, string[]> = {
  "studio-shell": ["overlay.tsx", "regions.tsx", "strip-drag.ts", "tab-drag.ts"],
  "ink-editor": ["code-actions.ts", "keybindings.ts", "widget-modal.ts", "widget-popover.ts"],
};

describe("every document-level dismiss-shaped listener enrols in its package's dismiss-registry (#2766)", () => {
  const registryPkgDirs = discoverRegistryPackageDirs();

  it("finds exactly the two known registry-owning packages (studio-shell, ink-editor)", () => {
    // Non-vacuity + drift pin, in one: a broken derivation that found ZERO
    // packages would make every check below vacuously pass; a THIRD package
    // growing its own dismiss-registry.ts should force a conscious look at
    // this guard (does it also need scanning? do the two registries need
    // unifying?) rather than being silently skipped.
    const names = registryPkgDirs.map((dir) => relative(packagesRoot, dir)).sort();
    expect(
      names,
      "the set of packages owning a dismiss-registry.ts changed — if a package genuinely " +
        "gained (or lost) one, this pin needs a conscious update; if not, discoverRegistryPackageDirs " +
        "is broken",
    ).toEqual(["ink-editor", "studio-shell"]);
  });

  for (const pkgDir of registryPkgDirs) {
    const pkgName = relative(packagesRoot, pkgDir);
    const srcDir = join(pkgDir, "src");
    const discoveredFiles = listSourceFiles(srcDir)
      .filter((file) => scanListenerSites(file).length > 0)
      .sort();
    const expectedFiles = (EXPECTED_LISTENER_FILES[pkgName] ?? [])
      .map((name) => join(srcDir, name))
      .sort();

    describe(`${pkgName}`, () => {
      it("the set of files with a document-level keydown/pointerdown listener matches EXPECTED_LISTENER_FILES", () => {
        const label = (files: string[]): string[] =>
          files.map((file) => relative(packagesRoot, file)).sort();

        expect(
          label(discoveredFiles.filter((file) => !expectedFiles.includes(file))),
          `the source scan is AHEAD of EXPECTED_LISTENER_FILES["${pkgName}"]: these files call ` +
            "document.addEventListener(\"keydown\"|\"pointerdown\", ...) but this test does not " +
            "know about them, so they are checked by nothing below. Add them to " +
            "EXPECTED_LISTENER_FILES, then either enrol the surface via registerDismissible() " +
            "(imported from its own ./dismiss-registry) or, if it genuinely isn't a dismissable " +
            'transient surface, mark each listener call site "// DISMISS-NET-EXEMPT: <reason>"',
        ).toEqual([]);
        expect(
          label(expectedFiles.filter((file) => !discoveredFiles.includes(file))),
          `EXPECTED_LISTENER_FILES["${pkgName}"] is AHEAD of the source scan: these entries no ` +
            "longer hold a document-level keydown/pointerdown listener (moved, renamed, or " +
            "removed). Drop them from EXPECTED_LISTENER_FILES",
        ).toEqual([]);
      });

      const allSites = discoveredFiles.flatMap((file) => scanListenerSites(file));

      it("finds at least one listener call site to check (a scan finding nothing would pass every check below)", () => {
        expect(allSites.length).toBeGreaterThan(0);
      });

      for (const file of discoveredFiles) {
        const enrolled = isModuleEnrolled(file);
        const sites = scanListenerSites(file);
        const relFile = relative(packagesRoot, file);

        for (const site of sites) {
          const label = `${relFile}:${site.line}`;
          it(`${label} is enrolled via registerDismissible() or carries a DISMISS-NET-EXEMPT marker`, () => {
            expect(
              enrolled || site.exemptReason !== null,
              `${label} calls a document-level keydown/pointerdown listener (${site.code}) but ` +
                `${relFile} neither imports+calls registerDismissible() from its own ` +
                "./dismiss-registry, nor does this call site carry a " +
                '"// DISMISS-NET-EXEMPT: <reason>" marker directly above it. This is exactly the ' +
                "gap #2766 was filed for: a new dismissable surface (or a listener that LOOKS " +
                "like one) can silently fall back into the unescapable-menu failure mode #279 " +
                "was filed for. Either enrol it (registerDismissible()) or mark it exempt with a " +
                "reason if it manages transient interaction/layout state rather than a floating " +
                "menu/popover/modal surface",
            ).toBe(true);
          });

          const exemptReason = site.exemptReason;
          if (exemptReason !== null) {
            it(`${label}'s DISMISS-NET-EXEMPT marker carries a non-empty reason`, () => {
              expect(exemptReason.trim().length).toBeGreaterThan(0);
            });
          }
        }
      }
    });
  }
});
