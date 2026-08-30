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
 *
 * ── #2846 follow-ups to the above (three of its four points; point 3, the
 * two-registries-uncoordinated design question, is out of scope for this
 * guard and left to a maintainer ruling) ──
 *
 *  - **Point 1 — the LISTENER pattern was `document`-only.** `dismiss-registry.ts`
 *    ITSELF attaches its net listener on `window` (see that file's
 *    "LISTENER ORDERING" note), so "attach the way the registry does" was
 *    the single most plausible next-surface shape and it evaded this scan
 *    entirely — unguarded AND unflagged. {@link LISTENER}'s doc comment
 *    records the exact widening chosen (target: `document`/`window`/
 *    `ownerDocument`; event: `keydown`/`keyup`/`pointerdown`) and why it
 *    stops there. Proof this widening actually fails on a real violation:
 *    a `window.addEventListener("keydown", ...)` dismiss-shaped listener
 *    (the exact shape named above) was added to a NEW, unenrolled file,
 *    `packages/studio-shell/src/__scratch-violation-2846.ts` — not
 *    `overlay.tsx` or any other already-enrolled file, for the same reason
 *    the original #2766 proof avoided one (see the corollary above: a call
 *    site in an already-enrolled file inherits that file's module-level
 *    enrolment and cannot go red on its own). Running this suite against
 *    that file found FOUR failures, not one: the scratch file itself (both
 *    "source scan is AHEAD of EXPECTED_LISTENER_FILES" and "call site is
 *    enrolled or exempt"), plus — because the widened target now also
 *    matches `window.addEventListener("keydown", ...)` in the two REAL
 *    `dismiss-registry.ts` files' own `installGlobalDismissNet()` — those
 *    two call sites failed the same "enrolled or exempt" check for the
 *    first time. The scratch file was then deleted (not part of this PR's
 *    diff); the two real call sites needed an actual fix — see the next
 *    point.
 *  - **Point 1's fallout — the registries' own net listeners.** Both
 *    `dismiss-registry.ts` files now carry a `DISMISS-NET-EXEMPT` marker on
 *    their own `window.addEventListener("keydown", ...)` call (the widening
 *    made it a discovered site — see the comment on `EXPECTED_LISTENER_FILES`
 *    below), each with its own behavioural-backing test file
 *    (`dismiss-registry-net-listener.test.ts` in both `ink-editor` and this
 *    package, per point 2 below).
 *  - **Point 2 — exempt markers asserted a claim nothing checked.** The
 *    `SAVE-PATH` precedent this guard was modelled on
 *    (`docs/studio-shell-spec.md` §7.7.1, #2571) requires a marker's
 *    justification to be PROVEN, not just present. Every `DISMISS-NET-EXEMPT`
 *    marker in the workspace — the three pre-existing ones
 *    (`tab-drag.ts`, `strip-drag.ts`, `regions.tsx`) plus `ElementDropdown.tsx`
 *    (present before #2846 but not named in its issue body) plus the two new
 *    ones from point 1's fallout — now has a dedicated behavioural test
 *    proving its claim against the real production module, not a
 *    reimplementation: see `dismiss-net-exempt-claims.test.ts` for the
 *    first four and `dismiss-registry-net-listener.test.ts` (both packages)
 *    for the net-listener two.
 *  - **Point 4 — a JSDoc-quoted example counted as a real call.**
 *    `scanListenerSites` used to skip `//`-prefixed lines only, so a block
 *    comment (`/** ... *\/`) mentioning the listener shape in prose (e.g.
 *    documenting the pattern this very guard looks for) counted as a real
 *    call site with no way to mark it exempt — the exempt-marker walk-up
 *    only ever recognized a `//` comment directly above a call. Fixed by
 *    {@link blankBlockComments}: block-comment spans are blanked (not
 *    removed — line numbers stay stable) before the LISTENER scan runs, so
 *    only real, uncommented calls are found; the raw lines are still used
 *    for the exempt-marker walk-up and the reported source text. See the
 *    "scanListenerSites ignores a listener call quoted inside a block
 *    comment" describe block below for the fixture-backed proof (including
 *    the false-positive reproduced by temporarily scanning raw lines
 *    instead of {@link blankBlockComments}'s output, during development of
 *    this fix).
 */

import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { deriveScanRoots, parseWorkspacePackageGlobs } from "./workspace-roots.js";

const here = dirname(fileURLToPath(import.meta.url));
/** `packages/` — this file lives at `packages/brink-studio/src/__tests__/`. */
const packagesRoot = resolve(here, "../../..");
/** The repo root — one level above `packages/`. */
const repoRoot = resolve(packagesRoot, "..");
const workspaceYamlPath = resolve(repoRoot, "pnpm-workspace.yaml");

const REGISTRY_FILE = "dismiss-registry.ts";

/**
 * A real dismiss-shaped listener call (not a declaration) — widened by
 * #2846 along both axes it names, symmetrically:
 *
 *  - target: `document` (the original #2766 scope) plus `window` — the
 *    registry's OWN net listener (`dismiss-registry.ts`) attaches on
 *    `window`, bubble phase (see that file's "LISTENER ORDERING" note), so
 *    "attach the way the registry does" was the single most plausible
 *    unguarded next shape and it evaded the scan entirely — plus
 *    `ownerDocument`, common in portal/iframe-aware components reaching for
 *    the document that actually owns their DOM node rather than the host
 *    page's `document`.
 *  - event: `keydown` / `pointerdown` (the original scope) plus `keyup` —
 *    the same dismiss shape, one key event later.
 *
 * Deliberately NOT widened further (e.g. `pointerup`, `touchstart`, a bare
 * `addEventListener` on an arbitrary element): #2846 warns explicitly that
 * over-widening trades a coverage hole for marker-noise that erodes the
 * `DISMISS-NET-EXEMPT` convention (#2766's own boilerplate-nobody-reads
 * failure mode) — a global hotkey or focus-tracking listener that happens
 * to reuse `document`/`window`/`ownerDocument` + one of these three events
 * is exactly the false-positive shape the issue names, and going further
 * (matching every event, or every listener target) would multiply that
 * cost for no discovered real-world dismiss-shaped listener beyond the two
 * this widening actually surfaced (both registries' own net listener,
 * #2846 point 1 — see the `DISMISS-NET-EXEMPT` markers added to
 * `dismiss-registry.ts` in both packages). Revisit the width if a real
 * listener of one of the excluded shapes ever appears.
 */
const LISTENER = /\b(?:document|window|ownerDocument)\.addEventListener\(\s*(["'])(keydown|keyup|pointerdown)\1/;
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

/**
 * Blank out every `/* … *\/` block-comment span, keeping the exact line
 * count (each replaced character becomes a space, `\n` left alone) so line
 * numbers computed from the result still line up with the original file.
 * #2846 point 4: `scanListenerSites` used to skip `//` lines only, so a
 * JSDoc line quoting the listener shape as an example (e.g. this very
 * function's own doc comment, if it named the pattern) counted as a real
 * call site with no way to mark it exempt short of rewording the prose —
 * the exempt-marker walk-up only recognizes a `//` comment directly above a
 * call, and a block comment's `*` continuation lines do not qualify as one.
 * Scanning this blanked copy instead — rather than the raw source — for
 * call sites (while still using the RAW lines for the exempt-marker walk
 * and the reported `code` text) means a block comment can name the pattern
 * freely; only a real, uncommented call is ever flagged.
 */
function blankBlockComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, (block) => block.replace(/[^\n]/g, " "));
}

/** Every real listener call site in `file`, paired with any exempt marker above it. */
function scanListenerSites(file: string): ListenerSite[] {
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  const codeLines = blankBlockComments(text).split("\n");
  const sites: ListenerSite[] = [];
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith("//")) continue; // prose about a call, not a call
    if (!LISTENER.test(codeLines[i])) continue; // block-comment prose is not a call either

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
  // "dismiss-registry.ts" itself joined this list under #2846's widened
  // LISTENER pattern (target now includes `window`, not just `document`):
  // its own net-install listener (`window.addEventListener("keydown", ...)`)
  // is now a discovered call site in BOTH packages, and — being the net,
  // not a surface that dismisses INTO the net — carries its own
  // DISMISS-NET-EXEMPT marker rather than an enrolment (see that file).
  "studio-shell": [
    "dismiss-registry.ts",
    "overlay.tsx",
    "regions.tsx",
    "strip-drag.ts",
    "tab-drag.ts",
  ],
  "ink-editor": [
    "code-actions.ts",
    "dismiss-registry.ts",
    "keybindings.ts",
    "widget-modal.ts",
    "widget-popover.ts",
  ],
  // KeymapSettings' recording gesture owns the entire keyboard while a
  // binding is being captured (capture-phase window listener), so its
  // Escape cannot route through the net — the same key must be swallowed
  // before every other handler. DISMISS-NET-EXEMPT at the site.
  "studio-ui": ["BinderContextMenu.tsx", "ElementDropdown.tsx", "KeymapSettings.tsx"],
  // The desktop shell's New Project dialog (#3012) — a modal on the
  // landing screen, OUTSIDE any studio mount, so there is no live
  // dismiss-registry to enrol in; its Escape listener carries a
  // DISMISS-NET-EXEMPT marker instead (see that file for the lifecycle).
  "brink-desktop": ["new-project-dialog.ts"],
};

describe("scanListenerSites ignores a listener call quoted inside a block comment (#2846 point 4)", () => {
  let scratchDir = "";

  beforeEach(() => {
    scratchDir = mkdtempSync(join(tmpdir(), "dismiss-net-exempt-scan-"));
  });

  afterEach(() => {
    rmSync(scratchDir, { recursive: true, force: true });
  });

  it("a JSDoc example naming the exact call shape is not counted as a real call site", () => {
    const file = join(scratchDir, "jsdoc-only.ts");
    writeFileSync(
      file,
      [
        "/**",
        " * Example usage (do not copy verbatim, see the real call below):",
        ' * document.addEventListener("keydown", handler, true);',
        " */",
        "export function noop(): void {}",
        "",
      ].join("\n"),
    );

    expect(scanListenerSites(file)).toEqual([]);
  });

  it("a real call site AFTER a JSDoc block naming the same shape is still found, at the right line", () => {
    const file = join(scratchDir, "jsdoc-then-real.ts");
    const lines = [
      "/**",
      " * See the pattern below:",
      ' * document.addEventListener("keydown", handler, true);',
      " */",
      "export function attach(handler: (e: KeyboardEvent) => void): void {",
      '  document.addEventListener("keydown", handler, true);',
      "}",
      "",
    ];
    writeFileSync(file, lines.join("\n"));

    const sites = scanListenerSites(file);
    expect(sites).toHaveLength(1);
    // 1-based line of the REAL call — the 6th line, not the 3rd (JSDoc).
    expect(sites[0].line).toBe(6);
    expect(sites[0].exemptReason).toBeNull();
  });

  it("a genuinely unmarked call directly beneath an UNRELATED block comment (no exempt marker inside it) still fails enrolment — block comments never satisfy the exempt walk-up", () => {
    const file = join(scratchDir, "block-comment-then-unmarked-call.ts");
    const lines = [
      "/**",
      " * Some unrelated explanation of what this does.",
      " */",
      'document.addEventListener("keydown", () => {}, true);',
      "",
    ];
    writeFileSync(file, lines.join("\n"));

    const sites = scanListenerSites(file);
    expect(sites).toHaveLength(1);
    // The walk-up only recognizes a `//` exempt marker directly above —
    // this call is real and unmarked, matching today's behaviour for a
    // JSDoc block that does NOT carry the marker (only #2846's fix to the
    // false-POSITIVE case above changed; a genuine miss must still fail).
    expect(sites[0].exemptReason).toBeNull();
  });
});

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
