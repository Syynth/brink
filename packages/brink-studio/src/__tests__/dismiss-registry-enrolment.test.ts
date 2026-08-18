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
 *     `save-path-enrolment.test.ts` uses) and walks EVERY `packages/*\/src`
 *     tree — not just the packages that happen to own a
 *     `src/dismiss-registry.ts`. A dismissable surface can live in a package
 *     with no registry of its own (e.g. `studio-ui`'s `BinderContextMenu.tsx`
 *     enrols cross-package into `studio-shell`'s registry) — scoping the walk
 *     to registry-owning packages would leave exactly that surface unscanned,
 *     the coverage hole this guard exists to close. Separately, it also
 *     derives which packages own a `src/dismiss-registry.ts` — today exactly
 *     `studio-shell` and `ink-editor` — because a call site is module-enrolled
 *     only if it imports `registerDismissible` from one of THOSE packages'
 *     registries (its own, via a relative import, or another's, via that
 *     package's published name). ⚠ There are TWO independent, uncoordinated
 *     registries (`packages/studio-shell/src/dismiss-registry.ts` and
 *     `packages/ink-editor/src/dismiss-registry.ts`) — Escape only dismisses
 *     surfaces within one package at a time. This guard checks every call
 *     site against whichever registry it actually enrols in; it does not
 *     unify the two registries (out of scope for #2766, noted as a known
 *     limitation).
 *  2. Within every workspace package's `src/` (skipping `__tests__`, `dist`,
 *     `node_modules`), it finds every real
 *     `document.addEventListener("keydown" | "pointerdown", ...)` call — a
 *     source scan, not a hand-typed file list. The result is compared
 *     against {@link EXPECTED_LISTENER_FILES}: a NEW file growing such a
 *     call fails the first `it()` below immediately, before the per-site
 *     checks are reached.
 *  3. A file is module-enrolled if it imports `registerDismissible` — from
 *     its OWN `./dismiss-registry` module, or from another package's
 *     published name (e.g. `studio-ui` importing from `"@brink/studio-shell"`)
 *     — AND calls it. Every listener call site in that file is then covered,
 *     matching how a real surface enrols today (one `registerDismissible()`
 *     call covering that surface's whole open/close lifecycle, not one call
 *     per listener).
 *  4. A file that is NOT module-enrolled must carry a `DISMISS-NET-EXEMPT`
 *     marker comment (mirroring `SAVE-PATH-EXEMPT`) directly above EACH
 *     qualifying listener call, with a reason — for the cases that are
 *     genuinely not "dismiss a transient surface" (an Escape-cancels-drag-
 *     gesture or Escape-restores-maximize handler that manages transient
 *     interaction/layout STATE, not a floating DOM menu/popover/modal; or an
 *     arrow/Enter/shortcut-key navigation handler whose Escape dismissal is
 *     already delegated to a wrapping, already-enrolled surface). A call
 *     site with neither an enrolling module nor an exempt marker fails.
 *
 * Proof this guard actually fails on a real violation (issue #2766's
 * requirement): a `document.addEventListener("keydown", ...)` dismiss-shaped
 * listener with no `registerDismissible()` call and no `DISMISS-NET-EXEMPT`
 * marker was temporarily added to `packages/studio-shell/src/__scratch-violation.ts`
 * (a new, unenrolled file — not an already-enrolled one, since every real
 * listener call site in an already-enrolled file like `overlay.tsx` inherits
 * that file's module-level enrolment and would not go red) during development
 * of this guard; both the "the source scan is AHEAD of EXPECTED_LISTENER_FILES"
 * check and the "every qualifying call site is enrolled or marked exempt"
 * check below went red, naming that exact file:line; the scratch file was
 * then deleted and is not part of this PR's diff. See the PR body for #2766.
 *
 * Corollary of the module-level enrolment rule (point 3): a NEW unenrolled
 * dismiss-shaped listener added to a file that is ALREADY module-enrolled
 * (e.g. `overlay.tsx`, `widget-popover.ts`, `code-actions.ts`,
 * `keybindings.ts`, `widget-modal.ts`) would pass this guard without a
 * marker — module-level enrolment covers every call site in that file, not
 * just the ones present when it was added. That mirrors real enrolment
 * (one `registerDismissible()` call covers a surface's whole lifecycle) and
 * is why the proof above uses a new file rather than adding to an
 * already-enrolled one.
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
 * `packages:` globs (the same derivation `save-path-enrolment.test.ts` uses).
 */
function discoverPackageDirs(): string[] {
  const globs = parseWorkspacePackageGlobs(readFileSync(workspaceYamlPath, "utf8"));
  return deriveScanRoots(globs, repoRoot);
}

/** Of every workspace package, the ones that own a `src/dismiss-registry.ts`. */
function discoverRegistryPackageDirs(allPkgDirs: string[]): string[] {
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

/**
 * The published npm name (`package.json`'s `"name"`) of each registry-owning
 * package, e.g. `"@brink/studio-shell"` — the specifiers a cross-package
 * `registerDismissible` import must resolve to.
 */
function registryPackageNames(registryPkgDirs: string[]): string[] {
  return registryPkgDirs
    .map((dir) => {
      const pkgJsonPath = join(dir, "package.json");
      const parsed = JSON.parse(readFileSync(pkgJsonPath, "utf8")) as { name?: unknown };
      if (typeof parsed.name !== "string" || parsed.name.length === 0) {
        throw new Error(`${pkgJsonPath} has no usable "name" field`);
      }
      return parsed.name;
    })
    .sort();
}

function escapeRegExp(literal: string): string {
  return literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * The module-level enrolment import: `registerDismissible` imported either
 * from the file's OWN `./dismiss-registry` module, or from another
 * registry-owning package's published name (a cross-package enrolment, like
 * `studio-ui` importing from `"@brink/studio-shell"`).
 */
function buildEnrollingImportPattern(registryNames: string[]): RegExp {
  const specifiers = ["\\./dismiss-registry(?:\\.js|\\.ts)?", ...registryNames.map(escapeRegExp)].join("|");
  return new RegExp(`import\\s*\\{[^}]*\\bregisterDismissible\\b[^}]*\\}\\s*from\\s*["'](?:${specifiers})["']`);
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

/** Does `file` enrol at the module level — own or cross-package registry, imported AND called? */
function isModuleEnrolled(file: string, enrollingImportPattern: RegExp): boolean {
  const text = readFileSync(file, "utf8");
  return enrollingImportPattern.test(text) && ENROLLING_CALL.test(text);
}

/**
 * Every production file today holding a real `document`-level `keydown` /
 * `pointerdown` listener call, per package (grep-verified against `main` on
 * 2026-08-18). NOT the source of truth — the `it()` below re-derives this
 * from every workspace package's `src/` on every run and fails the moment
 * the two disagree, so this list going stale is itself caught.
 */
const EXPECTED_LISTENER_FILES: Record<string, string[]> = {
  "studio-shell": ["overlay.tsx", "regions.tsx", "strip-drag.ts", "tab-drag.ts"],
  "ink-editor": ["code-actions.ts", "keybindings.ts", "widget-modal.ts", "widget-popover.ts"],
  "studio-ui": ["BinderContextMenu.tsx", "ElementDropdown.tsx"],
};

describe("every document-level dismiss-shaped listener enrols in its package's dismiss-registry (#2766)", () => {
  const allPkgDirs = discoverPackageDirs();
  const registryPkgDirs = discoverRegistryPackageDirs(allPkgDirs);

  it("finds exactly the two known registry-owning packages (studio-shell, ink-editor)", () => {
    // Non-vacuity + drift pin, in one: a broken derivation that found ZERO
    // packages would make the enrolment check below vacuously accept every
    // cross-package import; a THIRD package growing its own
    // dismiss-registry.ts should force a conscious look at this guard (does
    // it also need scanning as an enrolment target? do the registries need
    // unifying?) rather than being silently absorbed.
    const names = registryPkgDirs.map((dir) => relative(packagesRoot, dir)).sort();
    expect(
      names,
      "the set of packages owning a dismiss-registry.ts changed — if a package genuinely " +
        "gained (or lost) one, this pin needs a conscious update; if not, discoverRegistryPackageDirs " +
        "is broken",
    ).toEqual(["ink-editor", "studio-shell"]);
  });

  const enrollingImportPattern = buildEnrollingImportPattern(registryPackageNames(registryPkgDirs));

  // Flat, cross-package scan — mirrors save-path-enrolment.test.ts's model:
  // walk EVERY packages/*/src, not just the packages that happen to own a
  // dismiss-registry.ts, so a brand-new dismiss-shaped listener in ANY
  // package (e.g. studio-ui's BinderContextMenu.tsx / ElementDropdown.tsx,
  // neither of which owns its own registry) is still caught rather than
  // silently unscanned.
  const discoveredByPkg = new Map<string, string[]>();
  for (const pkgDir of allPkgDirs) {
    const srcDir = join(pkgDir, "src");
    try {
      if (!statSync(srcDir).isDirectory()) continue;
    } catch {
      continue; // no src/ (a config-only package)
    }
    const files = listSourceFiles(srcDir)
      .filter((file) => scanListenerSites(file).length > 0)
      .sort();
    if (files.length > 0) discoveredByPkg.set(relative(packagesRoot, pkgDir), files);
  }

  it("the set of packages holding a document-level keydown/pointerdown listener matches EXPECTED_LISTENER_FILES' keys (the enrolment-capable package set)", () => {
    // Re-pins the ENROLMENT-CAPABLE package set (every package that can hold
    // a dismiss-shaped listener) rather than the narrower registry-owning
    // one — a package with no registry of its own (studio-ui) still owes
    // this guard's coverage via cross-package enrolment.
    expect([...discoveredByPkg.keys()].sort()).toEqual(Object.keys(EXPECTED_LISTENER_FILES).sort());
  });

  for (const [pkgName, discoveredFiles] of discoveredByPkg) {
    const srcDir = join(packagesRoot, pkgName, "src");
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
            "(imported from its own ./dismiss-registry, or from a registry-owning package's " +
            "published name) or, if it genuinely isn't a dismissable transient surface, mark " +
            'each listener call site "// DISMISS-NET-EXEMPT: <reason>"',
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
        const enrolled = isModuleEnrolled(file, enrollingImportPattern);
        const sites = scanListenerSites(file);
        const relFile = relative(packagesRoot, file);

        for (const site of sites) {
          const label = `${relFile}:${site.line}`;
          it(`${label} is enrolled via registerDismissible() or carries a DISMISS-NET-EXEMPT marker`, () => {
            expect(
              enrolled || site.exemptReason !== null,
              `${label} calls a document-level keydown/pointerdown listener (${site.code}) but ` +
                `${relFile} neither imports+calls registerDismissible() from its own ` +
                "./dismiss-registry (or a registry-owning package's published name), nor does " +
                'this call site carry a "// DISMISS-NET-EXEMPT: <reason>" marker directly above ' +
                "it. This is exactly the gap #2766 was filed for: a new dismissable surface (or a " +
                "listener that LOOKS like one) can silently fall back into the unescapable-menu " +
                "failure mode #279 was filed for. Either enrol it (registerDismissible()) or mark " +
                "it exempt with a reason if it manages transient interaction/layout state rather " +
                "than a floating menu/popover/modal surface",
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
