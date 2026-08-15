/**
 * Static enrolment check for the confirm→retire sweep (issue #2480).
 *
 * `save-retire-invariant.test.ts`'s `SAVE_PATHS` array is a hand-maintained
 * driver list, and the only thing that told a save-command author to extend
 * it when adding a new save path was a doc bullet (`docs/embedder-api.md`,
 * "Confirm and retire in ONE synchronous step"). Nothing enforced it: a sixth
 * `session.markFilesSaved`/`markAllSaved` call site could land with no race
 * test at all and every existing suite would stay green.
 *
 * This suite derives the "did you forget to enrol it" check from the
 * production source itself rather than from a second hand-maintained list
 * that could ALSO drift (the alias-map class of gap, #2450/#2464):
 *
 *  1. It walks every `packages/*\/src` tree (skipping `__tests__`, `dist`,
 *     `node_modules`) and finds every file with a real `.markFilesSaved(` /
 *     `.markAllSaved(` call — a source scan, not a name typed by hand. The
 *     result is compared against {@link SCANNED_FILES}: if a NEW file grows
 *     a call site, `SCANNED_FILES` is stale and this fails immediately,
 *     before the per-call-site checks below are even reached.
 *  2. For each of those files it collects the same calls line by line and
 *     requires a `SAVE-PATH` / `SAVE-PATH-EXEMPT` marker comment in the
 *     contiguous comment block directly above each — see the markers in
 *     `persistence.ts` and `file-commands.ts`.
 *  3. Every `SAVE-PATH` marker's ids are cross-checked against
 *     `save-paths.ts`'s `SAVE_PATH_IDS`, the same registry
 *     `save-retire-invariant.test.ts` types its drivers against. A marker
 *     naming an id no driver sweeps (typo, stale rename) fails too.
 *
 * A brand-new save path therefore fails here two different ways depending on
 * what its author did: add the call with no marker → step 2 fails; add the
 * call and a marker but no `SAVE_PATHS` driver → step 3 fails, because the
 * id it names is not in the registry. Only doing both — actually enrolling
 * it — passes.
 *
 * ⚠ Both scan directions share {@link callLines}, so "is this file a
 * candidate" and "which lines in it are call sites" can never disagree about
 * what counts as a call: an asymmetry there would let a file be discovered
 * and then yield zero sites to check, silently passing.
 *
 * Two further checks close gaps flagged by PR #2510's review as uncovered by
 * this suite's original fence (issue #2515):
 *
 *  4. The scan's roots are DERIVED from `pnpm-workspace.yaml`'s `packages:`
 *     globs ({@link import("./workspace-roots.js")}), not a hand-maintained
 *     `packages/` assumption — so a workspace-layout change (a new glob, a
 *     package with sources outside `src/`) either widens the scan to match
 *     or fails loudly, rather than silently narrowing coverage.
 *  5. Every non-exempt `SAVE-PATH` id is required to name exactly ONE call
 *     site across the whole scan. Without this, a brand-new call site could
 *     satisfy step 3 above by reusing an id already claimed by an unrelated,
 *     real call site — the marker would name a real registry entry and pass,
 *     even though nothing enrolled THIS site. This does not prove the
 *     `SAVE_PATHS` driver behind an id exercises this exact call site at
 *     runtime (that needs call-site-level instrumentation, out of this
 *     suite's fence — see #2515) — it closes the narrower, concretely
 *     described loophole: reusing an existing id to skip enrolment.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, resolve } from "node:path";
import { describe, it, expect } from "vitest";
import { SAVE_PATH_IDS } from "./save-paths.js";
import { deriveScanRoots, parseWorkspacePackageGlobs } from "./workspace-roots.js";

const here = dirname(fileURLToPath(import.meta.url));
/** `packages/` — this file lives at `packages/brink-studio/src/__tests__/`. */
const packagesRoot = resolve(here, "../../..");
/** The repo root — one level above `packages/`. */
const repoRoot = resolve(packagesRoot, "..");
const workspaceYamlPath = resolve(repoRoot, "pnpm-workspace.yaml");

/** A CALL of the retire step; the leading `.` excludes declarations. */
const CALL = /\.(markFilesSaved|markAllSaved)\(/;
const MARKER = /^\/\/\s*SAVE-PATH(-EXEMPT)?\s+(markFilesSaved|markAllSaved):\s*(.+)$/;

/**
 * Every real call site in `text`, as 0-based line index + method name. A line
 * that is itself a `//` comment is prose about the call, never a call.
 */
function callLines(text: string): Array<{ index: number; method: string; code: string }> {
  const found: Array<{ index: number; method: string; code: string }> = [];
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith("//")) continue;
    const match = CALL.exec(lines[i]);
    if (match !== null) found.push({ index: i, method: match[1], code: trimmed });
  }
  return found;
}

const SKIP_DIRS = new Set(["__tests__", "dist", "node_modules", ".turbo"]);

/**
 * Every production file today holding a real `.markFilesSaved(` /
 * `.markAllSaved(` call site (grep-verified against `main` on 2026-08-14).
 * This is NOT the source of truth — {@link discoverCallSiteFiles} re-derives
 * it from `packages/*\/src` on every run and the first `it()` below fails the
 * moment the two disagree, so this array going stale is itself caught rather
 * than trusted.
 */
const SCANNED_FILES = [
  resolve(packagesRoot, "ink-editor/src/persistence.ts"),
  resolve(packagesRoot, "brink-studio/src/file-commands.ts"),
];

/**
 * Call sites the scan must find, summing to {@link EXPECTED_CALL_SITES}:
 * `persistence.ts`'s `saveDirty` retire, and `file-commands.ts`'s three
 * (`markSavedAndNotify`, `file.saveAll`'s batch retire, and the no-host-save
 * `markAllSaved`). Asserted exactly, not as "more than zero": a scan that
 * silently matched nothing — or matched only some — would otherwise leave
 * every per-site check below vacuous while still reporting green.
 */
const EXPECTED_CALL_SITES = 4;

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
 * this file's header, point 4, and `workspace-roots.ts`.
 */
function discoverPackageDirs(): string[] {
  const globs = parseWorkspacePackageGlobs(readFileSync(workspaceYamlPath, "utf8"));
  return deriveScanRoots(globs, repoRoot);
}

/** Every `packages/*\/src` file scanned, and the subset holding a call site. */
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
  marker: { exempt: boolean; method: string; ids: string[] } | null;
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
        marker = {
          exempt: match[1] !== undefined,
          method: match[2],
          ids: match[3]
            .split(",")
            .map((id) => id.trim())
            .filter((id) => id.length > 0),
        };
        break;
      }
    }
    return { file, line: index + 1, method, code, marker };
  });
}

describe("every production save path is enrolled in SAVE_PATHS (#2480)", () => {
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
    for (const pkg of ["ink-editor", "brink-studio"]) {
      expect(
        discovered.scanned.some((file) => file.startsWith(join(packagesRoot, pkg, "src"))),
        `the scan never walked packages/${pkg}/src — packagesRoot (${packagesRoot}) is wrong`,
      ).toBe(true);
    }
  });

  it("SCANNED_FILES is exactly the set of packages/*/src files with a call site", () => {
    const expected = [...SCANNED_FILES].sort();
    const label = (files: string[]): string[] =>
      files.map((file) => relative(packagesRoot, file)).sort();

    expect(
      label(discovered.withCallSite.filter((file) => !expected.includes(file))),
      "the source scan is AHEAD of SCANNED_FILES: these files call " +
        ".markFilesSaved(...) / .markAllSaved(...) but this test does not know about them, " +
        "so their call sites are checked by nothing. Add them to SCANNED_FILES and give each " +
        "call site a SAVE-PATH marker (see this file's header)",
    ).toEqual([]);
    expect(
      label(expected.filter((file) => !discovered.withCallSite.includes(file))),
      "SCANNED_FILES is AHEAD of the source scan: these entries no longer hold a call site " +
        "(moved, renamed, or the save path was removed). Drop them from SCANNED_FILES, and " +
        "retire the matching SAVE_PATHS driver in save-retire-invariant.test.ts if its path " +
        "is gone",
    ).toEqual([]);
  });

  const allSites = SCANNED_FILES.flatMap((file) => scanCallSites(file));

  it(`finds exactly ${EXPECTED_CALL_SITES} call sites to check`, () => {
    expect(
      allSites.map((site) => `${relative(packagesRoot, site.file)}:${site.line} ${site.method}`),
      "the number of call sites the scan found changed. If a save path was genuinely added " +
        "or removed, update EXPECTED_CALL_SITES; if not, the scan has stopped matching real " +
        "calls and every per-site check below is now vacuous",
    ).toHaveLength(EXPECTED_CALL_SITES);
  });

  for (const site of allSites) {
    const label = `${relative(packagesRoot, site.file)}:${site.line}`;

    it(`${label} (${site.method}) carries a SAVE-PATH marker`, () => {
      expect(
        site.marker,
        `${label} calls .${site.method}(...) with no "SAVE-PATH" / "SAVE-PATH-EXEMPT" marker ` +
          `comment in the block directly above it: ${site.code}\n` +
          'Add one — either "// SAVE-PATH markFilesSaved: <id in save-paths.ts>" naming the ' +
          "SAVE_PATHS driver(s) that sweep this call, or " +
          '"// SAVE-PATH-EXEMPT markFilesSaved: <reason>" if this site provably never has an ' +
          "await between its confirming read and this call.",
      ).not.toBeNull();
    });
  }

  const enrolledIds = new Set<string>(SAVE_PATH_IDS);

  for (const site of allSites) {
    const label = `${relative(packagesRoot, site.file)}:${site.line}`;
    if (site.marker === null) continue; // already reported by the check above

    const marker = site.marker;
    it(`${label}'s marker matches its call and names real SAVE_PATHS ids`, () => {
      expect(
        marker.method,
        `${label}: marker says "${marker.method}" but the call directly below it is ` +
          `.${site.method}(...) — the marker sits above the wrong call`,
      ).toBe(site.method);

      if (marker.exempt) {
        expect(
          marker.ids.length,
          `${label}: SAVE-PATH-EXEMPT needs a reason after the colon`,
        ).toBeGreaterThan(0);
        return;
      }

      expect(
        marker.ids.length,
        `${label}: SAVE-PATH marker lists no SAVE_PATHS ids`,
      ).toBeGreaterThan(0);
      for (const id of marker.ids) {
        expect(
          enrolledIds.has(id),
          `${label}: marker names save path ${JSON.stringify(id)}, which save-paths.ts's ` +
            "SAVE_PATH_IDS does not have (a typo, or the driver was renamed or removed " +
            "without updating this marker). Nothing sweeps that id",
        ).toBe(true);
      }
    });
  }

  it("every non-exempt SAVE-PATH id names exactly one call site (#2515)", () => {
    // Closes the concrete loophole hole 1 of #2515 describes: today's
    // per-marker checks only verify a marker's id EXISTS in the registry —
    // nothing stops a brand-new call site from reusing an id already
    // claimed by an unrelated, real call site, which would pass every check
    // above with "no new driver behind it". Requiring each id to be claimed
    // by exactly one call site closes that specific reuse: a new call site
    // must either take a genuinely unclaimed id (which then forces a new
    // SAVE_PATHS driver via save-retire-invariant.test.ts's bidirectional
    // check) or be marked SAVE-PATH-EXEMPT instead.
    //
    // This is NOT the full correspondence #2515 also describes — it does
    // not prove the SAVE_PATHS driver behind an id exercises THIS exact
    // call site at runtime, which needs call-site-level instrumentation
    // (an intentionally out-of-scope follow-up; see #2515's PR).
    const idSites = new Map<string, string[]>();
    for (const site of allSites) {
      if (site.marker === null || site.marker.exempt) continue;
      const label = `${relative(packagesRoot, site.file)}:${site.line}`;
      for (const id of site.marker.ids) {
        const sites = idSites.get(id) ?? [];
        sites.push(label);
        idSites.set(id, sites);
      }
    }

    // Non-vacuity: a scan that found zero non-exempt ids would make the
    // loop below check nothing and report green regardless.
    expect(idSites.size, "no non-exempt SAVE-PATH ids were found to check").toBeGreaterThan(0);

    const duplicated = [...idSites.entries()].filter(([, sites]) => sites.length > 1);
    expect(
      duplicated.map(([id, sites]) => `${id}: ${sites.join(", ")}`),
      "these SAVE_PATHS ids are named by more than one call site's marker — one of them is " +
        "reusing an id that is already enrolled elsewhere rather than being genuinely enrolled " +
        "itself. Give the new call site its own id (add it to save-paths.ts and drive it in " +
        "save-retire-invariant.test.ts), or mark it SAVE-PATH-EXEMPT if it provably never has " +
        "an await between its confirming read and the call",
    ).toEqual([]);
  });
});

describe("the scan's roots are derived from pnpm-workspace.yaml, not hand-maintained (#2515)", () => {
  it("pnpm-workspace.yaml's packages: list is exactly what the scan assumes today", () => {
    // An exact-string pin, not "contains packages/*": if this ever changes
    // — a new glob added, packages/* renamed — this must fail and force a
    // conscious look, per the guard this test itself exists to give teeth
    // to (workspace-roots.ts's derivation, not a second hand-typed list).
    const globs = parseWorkspacePackageGlobs(readFileSync(workspaceYamlPath, "utf8"));
    expect(globs).toEqual(["packages/*"]);
  });

  it("the derived roots are exactly today's package directories, not zero and not a guess", () => {
    const roots = discoverPackageDirs();
    const names = roots.map((root) => relative(repoRoot, root)).sort();
    // Non-vacuity: pinned as an exact set (not "length > 0") so a
    // derivation that silently under- or over-matches is caught here,
    // before it ever reaches discoverCallSiteFiles.
    expect(names).toEqual(
      [
        "brink-desktop",
        "brink-studio",
        "ink-editor",
        "ink-operations",
        "studio-shell",
        "studio-store",
        "studio-ui",
        "wasm",
        "wasm-types",
      ].map((pkg) => join("packages", pkg)),
    );
  });
});
