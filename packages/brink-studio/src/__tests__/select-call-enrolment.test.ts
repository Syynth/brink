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
 *
 * ── Sibling selection APIs (#2571 gap 2) ──────────────────────────────
 *
 * `.select()` / `.setSelectionRange(` are not the only ways to clobber a
 * user's edit. `input.selectionStart = 0; input.selectionEnd = n` reaches the
 * same end state one property at a time; `document.execCommand("selectAll")`
 * does it through the legacy editing command; and on a `contenteditable` the
 * Selection/Range API (`getSelection()` + `selectNodeContents(` /
 * `setBaseAndExtent(` / `addRange(`) does it with no input element involved
 * at all. A violator reaching for any of those enrolled nowhere.
 *
 * #2571 recorded "verified zero instances on main as of PR #2565" and asked
 * whether to widen now or wait for a real instance. Re-verified by grep on
 * 2026-08-16 (`\.(selectionStart|selectionEnd)\s*=[^=]`, `execCommand`,
 * `getSelection\(`, `createRange\(`, `selectNodeContents`, `setBaseAndExtent`
 * over every workspace package's `src` tree): still zero — the only
 * `contenteditable` hits in the tree are CodeMirror's own `contentDOM`
 * attribute, read (never written) by `document-sessions.ts` and two suites.
 *
 * Widened now rather than deferred, and the emptiness is the reason both
 * ways: the case for deferring is false positives, and with zero matches
 * there is no false positive to pay for and no marker churn to write — the
 * per-site checks below still see exactly the same five call sites, which
 * `EXPECTED_CALL_SITES` pins. Deferring would instead bank on a future author
 * both knowing the invariant exists and choosing the one spelling the scan
 * happens to match. If a legitimate future use does trip the widened scan
 * (a read-only `getSelection()` for a caret coordinate, say), the
 * `SELECT-INVARIANT-EXEMPT` hatch is the answer — and it is now proven usable
 * rather than assumed (see the second `describe` in this file).
 */

import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
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

/**
 * Every spelling of "programmatically move or widen a text selection" this
 * invariant enrols — the two originals plus the sibling APIs from #2571 gap
 * 2 (see this file's header). Order matters only for which name a match
 * reports; the alternatives are mutually exclusive in practice.
 *
 * `\s*=[^=]` on the two property assignments is what keeps them from matching
 * a comparison (`a.selectionStart === b`, `!==`) — this invariant is about
 * WRITES.
 */
const CALL_PATTERNS: ReadonlyArray<{ method: string; pattern: RegExp }> = [
  { method: "select", pattern: /\.select\(\)/ },
  { method: "setSelectionRange", pattern: /\.setSelectionRange\(/ },
  { method: "selectionStart=", pattern: /\.selectionStart\s*=[^=]/ },
  { method: "selectionEnd=", pattern: /\.selectionEnd\s*=[^=]/ },
  { method: "execCommand", pattern: /\bexecCommand\(/ },
  { method: "getSelection", pattern: /\bgetSelection\(\)/ },
  { method: "createRange", pattern: /\bcreateRange\(\)/ },
  { method: "selectNodeContents", pattern: /\.selectNodeContents\(/ },
  { method: "setBaseAndExtent", pattern: /\.setBaseAndExtent\(/ },
  { method: "addRange", pattern: /\.addRange\(/ },
];
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
    const hit = CALL_PATTERNS.find(({ pattern }) => pattern.test(lines[i]));
    if (hit !== undefined) found.push({ index: i, method: hit.method, code: trimmed });
  }
  return found;
}

const SKIP_DIRS = new Set(["__tests__", "dist", "node_modules", ".turbo"]);

/**
 * Every production file today holding a real call site matched by {@link
 * CALL_PATTERNS} (grep-verified against `main` on 2026-08-16). This is NOT
 * the source of truth — {@link discoverCallSiteFiles} re-derives it from
 * every workspace `src/` root on every run, and the first `it()` below
 * fails the moment the two disagree, so this array going stale is itself
 * caught rather than trusted.
 */
const SCANNED_FILES = [
  resolve(packagesRoot, "ink-editor/src/goto-definition.ts"),
  resolve(packagesRoot, "ink-editor/src/inline-name-input.ts"),
  resolve(packagesRoot, "studio-ui/src/Binder.tsx"),
  resolve(packagesRoot, "studio-ui/src/SearchView.tsx"),
  resolve(packagesRoot, "studio-ui/src/SymbolRenamePrompt.tsx"),
];

/**
 * Call sites the scan must find, summing to {@link EXPECTED_CALL_SITES}:
 * `goto-definition.ts`'s emulated multi-cursor (CM6 EditorSelection
 * `addRange`, #3110), `inline-name-input.ts`'s guarded `select()`, `Binder.tsx`'s two
 * `setSelectionRange` calls (rename pre-select, new-file cursor-to-end),
 * `SearchView.tsx`'s `select()`, and `SymbolRenamePrompt.tsx`'s guarded
 * `select()`. Asserted exactly, not as "more than zero": a scan that
 * silently matched nothing — or matched only some — would otherwise leave
 * every per-site check below vacuous while still reporting green.
 */
const EXPECTED_CALL_SITES = 6;

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

/**
 * Validates one call site's marker per the invariant rules, and is the
 * single source of truth for those rules: both the per-site `it()`s below
 * and this file's own synthetic-exempt-marker regression test call it
 * directly, so the regression test exercises the exact logic a real
 * author's marker goes through rather than a hand-rolled re-implementation
 * that could silently drift from it.
 *
 * An EXEMPT marker's id is deliberately NOT required to be in
 * `SELECT_CALL_IDS` — the registry tracks call sites this invariant
 * *enrols*, and an exempt site is by definition not one of those (see this
 * file's header and `select-calls.ts`). Requiring exempt ids to also appear
 * in `SELECT_CALL_IDS` would deadlock the escape hatch: the "every
 * non-exempt id names exactly one call site" check further down would then
 * need an exempt id to be claimed by a *non-exempt* marker, which is
 * impossible by construction — exactly the bug the #2565 review caught.
 */
function validateMarker(
  marker: NonNullable<CallSite["marker"]>,
  enrolledIds: ReadonlySet<string>,
  label: string,
): void {
  if (marker.exempt) {
    expect(
      marker.justification.length,
      `${label}: SELECT-INVARIANT-EXEMPT marker has no justification text after the id`,
    ).toBeGreaterThan(0);
    return;
  }
  expect(
    enrolledIds.has(marker.id),
    `${label}: marker names id ${JSON.stringify(marker.id)}, which select-calls.ts's ` +
      "SELECT_CALL_IDS does not have (a typo, or the id was renamed or removed without " +
      "updating this marker)",
  ).toBe(true);
  expect(
    marker.justification.length,
    `${label}: SELECT-INVARIANT marker has no justification text after the id`,
  ).toBeGreaterThan(0);
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
      "the source scan is AHEAD of SCANNED_FILES: these files move a text selection " +
        "programmatically (see CALL_PATTERNS — .select(), .setSelectionRange(...), a " +
        ".selectionStart/.selectionEnd write, execCommand, or the Selection/Range API) but " +
        "this test does not know about them, so their call sites are checked by nothing. Add " +
        "them to SCANNED_FILES and give each call site a SELECT-INVARIANT marker (see this " +
        "file's header)",
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
      validateMarker(marker, enrolledIds, label);
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

describe("SELECT-INVARIANT-EXEMPT is actually usable (regression test for the #2565 review fix)", () => {
  // Before this fix, `validateMarker` (née the inline per-site check) ran
  // `enrolledIds.has(marker.id)` unconditionally, so an exempt marker whose
  // id was correctly kept OUT of SELECT_CALL_IDS still failed here — and
  // the "every non-exempt id names exactly one call site" check could never
  // rescue it, because that check only ever looks at non-exempt markers.
  // The escape hatch this file's header and select-calls.ts document was
  // therefore unusable: the first author who followed the instruction
  // printed in three failure messages and both new file headers would hit
  // a deadlock, not green. This plants a real exempt marker in a real file
  // on disk and runs it through the exact scan + validation path a
  // production call site goes through, so a regression here fails loudly
  // instead of only being caught by re-reading the logic.
  it("an exempt marker whose id is NOT in SELECT_CALL_IDS still validates", () => {
    const dir = mkdtempSync(join(tmpdir(), "select-invariant-exempt-"));
    const file = join(dir, "synthetic.ts");
    try {
      writeFileSync(
        file,
        [
          "function synthetic(input: HTMLInputElement) {",
          "  // SELECT-INVARIANT-EXEMPT synthetic.notInRegistry: synthetic call site for this",
          "  // regression test only — not a real production text input.",
          "  input.select();",
          "}",
          "",
        ].join("\n"),
      );

      // synthetic.notInRegistry is deliberately absent from SELECT_CALL_IDS
      // — that absence is the point of this test.
      expect(SELECT_CALL_IDS as readonly string[]).not.toContain("synthetic.notInRegistry");

      const sites = scanCallSites(file);
      expect(sites).toHaveLength(1);
      const marker = sites[0].marker;
      if (marker === null) throw new Error("expected a marker; scanCallSites found none");
      expect(marker.exempt).toBe(true);
      expect(marker.id).toBe("synthetic.notInRegistry");

      // The regression: this must NOT throw, even though marker.id is not
      // enrolled in SELECT_CALL_IDS.
      expect(() => validateMarker(marker, new Set(SELECT_CALL_IDS), "synthetic.ts:4")).not.toThrow();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("a non-exempt marker whose id is NOT in SELECT_CALL_IDS still fails (control case)", () => {
    // The inverse of the test above: proves validateMarker's exempt branch
    // is actually doing selective work, not just always passing.
    const dir = mkdtempSync(join(tmpdir(), "select-invariant-non-exempt-"));
    const file = join(dir, "synthetic.ts");
    try {
      writeFileSync(
        file,
        [
          "function synthetic(input: HTMLInputElement) {",
          "  // SELECT-INVARIANT synthetic.alsoNotInRegistry: deliberately unenrolled control case.",
          "  input.select();",
          "}",
          "",
        ].join("\n"),
      );

      const sites = scanCallSites(file);
      expect(sites).toHaveLength(1);
      const marker = sites[0].marker;
      if (marker === null) throw new Error("expected a marker; scanCallSites found none");
      expect(marker.exempt).toBe(false);

      expect(() => validateMarker(marker, new Set(SELECT_CALL_IDS), "synthetic.ts:3")).toThrow();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("the sibling selection APIs are really matched (#2571 gap 2)", () => {
  // Widening CALL_PATTERNS costs nothing precisely BECAUSE it matches nothing
  // on `main` today (see this file's header) — which is also what makes a
  // typo in one of the new patterns invisible: the scan would keep finding
  // the same five real call sites and stay green forever while the sibling
  // API it claims to cover walked straight through. These cases put a line of
  // each spelling in front of `callLines` and require a hit.
  const VIOLATORS: ReadonlyArray<{ method: string; line: string }> = [
    { method: "select", line: "  input.select();" },
    { method: "setSelectionRange", line: "  input.setSelectionRange(0, 4);" },
    { method: "selectionStart=", line: "  input.selectionStart = 0;" },
    { method: "selectionEnd=", line: "  input.selectionEnd = name.length;" },
    { method: "execCommand", line: '  document.execCommand("selectAll");' },
    { method: "getSelection", line: "  const sel = window.getSelection();" },
    { method: "createRange", line: "  const range = document.createRange();" },
    { method: "selectNodeContents", line: "  range.selectNodeContents(host);" },
    { method: "setBaseAndExtent", line: "  sel.setBaseAndExtent(host, 0, host, 1);" },
    { method: "addRange", line: "  sel.addRange(range);" },
  ];

  it("every pattern in CALL_PATTERNS has a violator case here", () => {
    // Otherwise a pattern added later would silently have no coverage.
    expect(VIOLATORS.map((v) => v.method).sort()).toEqual(CALL_PATTERNS.map((p) => p.method).sort());
  });

  for (const { method, line } of VIOLATORS) {
    it(`${method} is seen as a call site`, () => {
      const found = callLines(`function violate() {\n${line}\n}\n`);
      expect(
        found.map((f) => f.method),
        `${line.trim()} matched nothing`,
      ).toEqual([method]);
    });
  }

  it("a comparison is not a call site (the property patterns match WRITES only)", () => {
    // `\s*=[^=]` is the whole reason these two patterns are safe to add; a
    // `=` that starts `==`/`===` is a read, and reads clobber nothing.
    const reads = [
      "  if (input.selectionStart === input.selectionEnd) return;",
      "  if (input.selectionEnd !== end) return;",
      "  const at = input.selectionStart == null ? 0 : input.selectionStart;",
    ].join("\n");
    expect(callLines(reads)).toEqual([]);
  });

  it("a commented-out call is not a call site", () => {
    // Preservation guard for the `trimmed.startsWith("//")` filter: markers
    // and prose about these APIs sit directly above real call sites, and a
    // scan that counted them would double every site.
    expect(callLines('  // document.execCommand("selectAll") would clobber it\n')).toEqual([]);
  });
});
