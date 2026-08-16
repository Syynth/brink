/**
 * Static enrolment check for §7.7.1's text-input select-call invariant
 * (issue #2542): "never `select()` text the user typed"
 * (`docs/studio-shell-spec.md` §7.7.1 rule 2).
 *
 * Before this suite, the only thing that would catch a violating call site —
 * a new text input whose `select()`/`setSelectionRange()` fires from a
 * deferred callback without checking the field still holds what was seeded
 * — was `search-view-focus.test.tsx`, which only exercises `SearchView`. A
 * new call site anywhere else in `studio-ui`, `studio-shell`, or
 * `ink-editor` shipped with no signal at all; `inline-name-input.ts` was
 * exactly that gap until #2548 fixed it.
 *
 * This suite is structurally a sibling of `save-path-enrolment.test.ts`
 * (issues #2480, #2510, #2515) and deliberately inherits its shape rather
 * than re-deriving it:
 *
 *  1. It derives its scan roots from `pnpm-workspace.yaml` via
 *     {@link import("./workspace-roots.js")} — the same module
 *     `save-path-enrolment.test.ts` uses — rather than hand-typing
 *     "studio-ui, studio-shell, ink-editor". The issue names those three
 *     packages as the known risk surface, but scanning only them would
 *     repeat #2515's "scan roots not derived from pnpm-workspace.yaml" hole:
 *     a call site added to a fourth package (or a package renamed) would
 *     silently enrol nowhere. Scanning every derived root costs nothing (the
 *     regex below has zero false positives across the whole workspace today
 *     — see the header note on the zero-argument `.select()` filter) and
 *     closes that hole by construction instead of by promise.
 *  2. It walks every `<root>/src` tree (skipping `__tests__`, `dist`,
 *     `node_modules`, `.turbo`) for real `.select()` / `.setSelectionRange(`
 *     call sites — a source scan, not a hand-typed list — and compares the
 *     result against {@link SCANNED_FILES}: a new file growing a call site
 *     makes {@link SCANNED_FILES} stale and fails immediately, before any
 *     per-call-site check is reached.
 *  3. Every call site must carry a `SELECT-INVARIANT` / `SELECT-INVARIANT-
 *     EXEMPT` marker comment directly above it, naming an id from
 *     `select-calls.ts`'s `SELECT_CALL_IDS`.
 *  4. Every non-exempt id is required to name exactly one call site — the
 *     #2515 "id reuse" loophole, closed here from the start rather than
 *     retrofitted after a wave finds it.
 *  5. The expected call-site count is asserted exactly (not ">0") so a scan
 *     that stops matching real calls does not silently make every
 *     per-site check below vacuous.
 *
 * ── Why `.select()` is matched zero-argument only ──────────────────────
 *
 * `.select(` alone is not a safe grep across this codebase: `studio-shell`'s
 * theme store and `brink-desktop`'s API surface both expose an unrelated
 * `select(x)` (`themes.select(theme.id)`, `api.select((s) => s.diagnostics)`,
 * `code-actions.ts`'s `this.select(action)`) that has nothing to do with
 * `HTMLInputElement.select()`/`HTMLTextAreaElement.select()`. The DOM API
 * this invariant is about takes no arguments, so `\.select\(\)` (empty
 * parens) is not a narrowing for convenience — it is the exact boundary
 * between "text-input select" and "an unrelated method that happens to be
 * named select". `.setSelectionRange(` has no such collision in this
 * codebase today.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, resolve } from "node:path";
import { describe, it, expect } from "vitest";
import { SELECT_CALL_IDS } from "./select-calls.js";
import { deriveScanRoots, parseWorkspacePackageGlobs } from "./workspace-roots.js";

const here = dirname(fileURLToPath(import.meta.url));
/** `packages/` — this file lives at `packages/brink-studio/src/__tests__/`. */
const packagesRoot = resolve(here, "../../..");
/** The repo root — one level above `packages/`. */
const repoRoot = resolve(packagesRoot, "..");
const workspaceYamlPath = resolve(repoRoot, "pnpm-workspace.yaml");

/** A CALL of the invariant: zero-arg `.select()`, or `.setSelectionRange(`. */
const CALL = /\.(select)\(\)|\.(setSelectionRange)\(/;
const MARKER = /^\/\/\s*SELECT-INVARIANT(-EXEMPT)?\s+(\S+):\s*(.+)$/;

/**
 * Every real call site in `text`, as 0-based line index + method name. A
 * line that is itself a `//` comment is prose about the call, never a call.
 */
function callLines(text: string): Array<{ index: number; method: string; code: string }> {
  const found: Array<{ index: number; method: string; code: string }> = [];
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith("//")) continue;
    const match = CALL.exec(lines[i]);
    if (match !== null) found.push({ index: i, method: match[1] ?? match[2], code: trimmed });
  }
  return found;
}

const SKIP_DIRS = new Set(["__tests__", "dist", "node_modules", ".turbo"]);

/**
 * Every production file today holding a real `.select()` (zero-argument) /
 * `.setSelectionRange(` call site (grep-verified against `main` on
 * 2026-08-16). This is NOT the source of truth — {@link discoverCallSiteFiles}
 * re-derives it from every workspace `src/` root on every run, and the first
 * `it()` below fails the moment the two disagree, so this array going stale
 * is itself caught rather than trusted.
 */
const SCANNED_FILES = [
  resolve(packagesRoot, "ink-editor/src/inline-name-input.ts"),
  resolve(packagesRoot, "studio-ui/src/Binder.tsx"),
  resolve(packagesRoot, "studio-ui/src/SearchView.tsx"),
  resolve(packagesRoot, "studio-ui/src/SymbolRenamePrompt.tsx"),
];

/**
 * Call sites the scan must find, summing to {@link EXPECTED_CALL_SITES}:
 * `inline-name-input.ts`'s guarded `select()`, `Binder.tsx`'s two
 * `setSelectionRange` calls (rename pre-select, new-file cursor-to-end),
 * `SearchView.tsx`'s `select()`, and `SymbolRenamePrompt.tsx`'s guarded
 * `select()`. Asserted exactly, not as "more than zero": a scan that
 * silently matched nothing — or matched only some — would otherwise leave
 * every per-site check below vacuous while still reporting green.
 */
const EXPECTED_CALL_SITES = 5;

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
 * Every package directory the scan should walk, derived from
 * `pnpm-workspace.yaml`'s `packages:` globs rather than hand-typed — see
 * this file's header, point 1, and `workspace-roots.ts`.
 */
function discoverPackageDirs(): string[] {
  const globs = parseWorkspacePackageGlobs(readFileSync(workspaceYamlPath, "utf8"));
  return deriveScanRoots(globs, repoRoot);
}

/** Every `<root>/src` file scanned, and the subset holding a call site. */
function discoverCallSiteFiles(): { scanned: string[]; withCallSite: string[] } {
  const scanned: string[] = [];
  const withCallSite: string[] = [];
  for (const pkgDir of discoverPackageDirs()) {
    const src = join(pkgDir, "src");
    try {
      if (!statSync(src).isDirectory()) continue;
    } catch {
      continue; // no src/ (a config-only package)
    }
    for (const file of listSourceFiles(src)) {
      scanned.push(file);
      if (callLines(readFileSync(file, "utf8")).length > 0) withCallSite.push(file);
    }
  }
  return { scanned: scanned.sort(), withCallSite: withCallSite.sort() };
}

interface CallSite {
  file: string;
  /** 1-based, for messages a reader can jump to. */
  line: number;
  method: string;
  code: string;
  marker: { exempt: boolean; id: string; justification: string } | null;
}

/** Every real call site in `file`, paired with the marker (if any) above it. */
function scanCallSites(file: string): CallSite[] {
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  return callLines(text).map(({ index, method, code }) => {
    // Walk up through the contiguous comment block directly above the call
    // (no blank or code line in between) looking for a marker anywhere in
    // it — markers sit alongside pre-existing explanatory comments.
    let marker: CallSite["marker"] = null;
    for (let j = index - 1; j >= 0; j -= 1) {
      const above = lines[j].trim();
      if (!above.startsWith("//")) break;
      const match = MARKER.exec(above);
      if (match !== null) {
        marker = { exempt: match[1] !== undefined, id: match[2], justification: match[3].trim() };
        break;
      }
    }
    return { file, line: index + 1, method, code, marker };
  });
}

describe("every §7.7.1 select-call site is enrolled (#2542)", () => {
  const discovered = discoverCallSiteFiles();

  it("the scan reaches real source (a scan finding nothing would pass every check below)", () => {
    // packagesRoot resolving somewhere unexpected, or listSourceFiles
    // filtering everything out, would make this suite a no-op that reports
    // green forever. Pin both ends: the walk visited a plausible tree, and
    // the packages that hold the call sites were among the ones walked.
    expect(
      discovered.scanned.length,
      `only ${discovered.scanned.length} files walked under ${packagesRoot}`,
    ).toBeGreaterThan(100);
    for (const pkg of ["ink-editor", "studio-ui", "studio-shell"]) {
      expect(
        discovered.scanned.some((file) => file.startsWith(join(packagesRoot, pkg, "src"))),
        `the scan never walked packages/${pkg}/src — packagesRoot (${packagesRoot}) is wrong`,
      ).toBe(true);
    }
  });

  it("SCANNED_FILES is exactly the set of src files with a call site", () => {
    const expected = [...SCANNED_FILES].sort();
    const label = (files: string[]): string[] =>
      files.map((file) => relative(packagesRoot, file)).sort();

    expect(
      label(discovered.withCallSite.filter((file) => !expected.includes(file))),
      "the source scan is AHEAD of SCANNED_FILES: these files call " +
        ".select()/.setSelectionRange(...) but this test does not know about them, so their " +
        "call sites are checked by nothing. Add them to SCANNED_FILES and give each call site " +
        "a SELECT-INVARIANT marker (see this file's header)",
    ).toEqual([]);
    expect(
      label(expected.filter((file) => !discovered.withCallSite.includes(file))),
      "SCANNED_FILES is AHEAD of the source scan: these entries no longer hold a call site " +
        "(moved, renamed, or the call was removed). Drop them from SCANNED_FILES and retire " +
        "the matching id(s) from select-calls.ts",
    ).toEqual([]);
  });

  const allSites = SCANNED_FILES.flatMap((file) => scanCallSites(file));

  it(`finds exactly ${EXPECTED_CALL_SITES} call sites to check`, () => {
    expect(
      allSites.map((site) => `${relative(packagesRoot, site.file)}:${site.line} ${site.method}`),
      "the number of call sites the scan found changed. If a call site was genuinely added or " +
        "removed, update EXPECTED_CALL_SITES; if not, the scan has stopped matching real calls " +
        "and every per-site check below is now vacuous",
    ).toHaveLength(EXPECTED_CALL_SITES);
  });

  for (const site of allSites) {
    const label = `${relative(packagesRoot, site.file)}:${site.line}`;

    it(`${label} (${site.method}) carries a SELECT-INVARIANT marker`, () => {
      expect(
        site.marker,
        `${label} calls .${site.method}(...) with no "SELECT-INVARIANT" / ` +
          `"SELECT-INVARIANT-EXEMPT" marker comment in the block directly above it: ${site.code}\n` +
          'Add one — either "// SELECT-INVARIANT <id in select-calls.ts>: <why this satisfies ' +
          'docs/studio-shell-spec.md §7.7.1 rule 2>", or "// SELECT-INVARIANT-EXEMPT <id>: ' +
          '<reason>" if this call is provably unrelated to a seeded text input.',
      ).not.toBeNull();
    });
  }

  const enrolledIds = new Set<string>(SELECT_CALL_IDS);

  for (const site of allSites) {
    const label = `${relative(packagesRoot, site.file)}:${site.line}`;
    if (site.marker === null) continue; // already reported by the check above

    const marker = site.marker;
    it(`${label}'s marker names a real select-calls.ts id and a non-empty justification`, () => {
      expect(
        enrolledIds.has(marker.id),
        `${label}: marker names id ${JSON.stringify(marker.id)}, which select-calls.ts's ` +
          "SELECT_CALL_IDS does not have (a typo, or the id was renamed or removed without " +
          "updating this marker)",
      ).toBe(true);
      expect(
        marker.justification.length,
        `${label}: SELECT-INVARIANT${marker.exempt ? "-EXEMPT" : ""} marker has no ` +
          "justification text after the id",
      ).toBeGreaterThan(0);
    });
  }

  it("every non-exempt SELECT-INVARIANT id names exactly one call site (inherited from #2515)", () => {
    // The same reuse loophole #2515 found in the SAVE_PATHS guard, closed
    // here from the start: without this, a brand-new call site could claim
    // an id already used by an unrelated, real call site and pass every
    // check above with no id of its own genuinely enrolling it.
    const idSites = new Map<string, string[]>();
    for (const site of allSites) {
      if (site.marker === null || site.marker.exempt) continue;
      const label = `${relative(packagesRoot, site.file)}:${site.line}`;
      const sites = idSites.get(site.marker.id) ?? [];
      sites.push(label);
      idSites.set(site.marker.id, sites);
    }

    // Non-vacuity: a scan that found zero non-exempt ids would make the
    // checks below vacuous while still reporting green.
    expect(idSites.size, "no non-exempt SELECT-INVARIANT ids were found to check").toBeGreaterThan(
      0,
    );

    const duplicated = [...idSites.entries()].filter(([, sites]) => sites.length > 1);
    expect(
      duplicated.map(([id, sites]) => `${id}: ${sites.join(", ")}`),
      "these select-calls.ts ids are named by more than one call site's marker — give the new " +
        "call site its own id in select-calls.ts, or mark it SELECT-INVARIANT-EXEMPT if it is " +
        "provably unrelated to a seeded text input",
    ).toEqual([]);

    // The other direction: an id in the registry claimed by zero call sites
    // is a free id a future violator could cite as its sole "justification"
    // without actually being enrolled by this scan.
    expect(
      SELECT_CALL_IDS.filter((id) => !idSites.has(id)),
      "these select-calls.ts ids are in the registry but named by no non-exempt call-site " +
        "marker — either the marker that claimed the id was removed (restore it), or the call " +
        "site is gone and the id should be retired from select-calls.ts",
    ).toEqual([]);
  });
});
