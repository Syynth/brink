/**
 * Static enrolment check for the off-paint-path invariant #2767 audits
 * (issue #2767, "studio: audit remaining synchronous wasm calls made inline
 * from React event handlers with no pending UI").
 *
 * ## The defect class
 *
 * A synchronous `session.*` wasm call made inline from a React event handler
 * with no synchronously-committed pending state before the call: because the
 * call has no yield point, it blocks the main thread — and therefore paint —
 * so the user gets a frozen UI with zero feedback. #722 fixed this for the
 * inline (F2) rename widget; PR #2761 fixed it again for the modal rename
 * prompt, because #722's fix never generalized past "rename". #2767 is the
 * audit that found the shape recurs a third time in `dispatchSymbolAction`'s
 * `moveStitch`/`promoteStitch`/`demoteKnot` branches — the three ops whose
 * Rust op runs the full-project breakage gate (`gate_with_source`,
 * `crates/internal/brink-ide/src/structural_result.rs`), the same cost class
 * as rename's collision check — and fixed those three the same way
 * (`runGatedStructuralOp` in `symbolMenuActions.ts`: commit a LOCAL
 * busy-state affordance synchronously — `structuralOpPending`, rendered by
 * the status bar's `StructuralOpSegment`, spec §7.7.4 — then defer the wasm
 * call via `scheduleIdleWork` and trust the op's own refusal for staleness;
 * no shell notification and no `session.generation` re-check, both tried in
 * an earlier draft and removed in review — see §7.7.4's "Third enrolment"
 * paragraph for why).
 *
 * ## Deliberately narrow scope
 *
 * This guard enrols exactly the three method names #2767 found and fixed —
 * `moveStitch`, `promoteStitch`, `demoteKnot` — not every wasm call the
 * studio makes, and not even every call that runs the same breakage gate.
 * That is a stated, not accidental, narrowing:
 *
 *  - `renameSymbol`/`renameSymbolAt` (rename) and `extractToKnot`/
 *    `extractToFunction` (extract) already carry the off-paint-path
 *    treatment, but through call sites that don't look like a bare
 *    `.methodName(` at all — `renameSymbolAt` is invoked through
 *    `InlineNameInput`'s `query` callback indirection, and
 *    `SymbolRenamePrompt.tsx`'s `run()` wraps its own `performSymbolRename`
 *    call in a hand-written `busy` state + `scheduleIdleWork`, not this
 *    file's marker convention. Enrolling their literal method names here
 *    would require retrofitting markers onto already-correct code with a
 *    different (and already-tested) remedy shape, for no new coverage.
 *  - `renameFile`/`renameDir` (`crates/internal/brink-ide/src/file_rename.rs`
 *    / `dir_rename.rs`) run the SAME gate and are called synchronously from
 *    `project-session.ts`'s `ProjectSession.renameFile` with no pending
 *    state — a same-shape site #2767's audit found but did not fix (see the
 *    issue's tracking comment). Adding them here would make this guard red
 *    on a known, tracked, un-fixed gap instead of on a NEW regression, which
 *    is a worse signal than leaving them out and naming the gap explicitly
 *    (this comment, and the #2767 issue thread).
 *
 * A new site matching this file's narrow scope — a bare `.moveStitch(`,
 * `.promoteStitch(`, or `.demoteKnot(` call added anywhere in the workspace
 * — fails this suite immediately, before any marker check, because
 * {@link SCANNED_FILES} goes stale. That is the guard's actual job: stop a
 * FOURTH occurrence of this exact three-method shape, not police every wasm
 * call in the codebase.
 *
 * ## What a false negative looks like
 *
 * This is a marker-presence scan, not a control-flow analysis — it cannot
 * verify that a `PAINT-PATH-DEFERRED` call site is ACTUALLY wrapped in
 * `scheduleIdleWork` with a synchronous pending-state commit ahead of it, or
 * that a `PAINT-PATH-EXEMPT` call site is genuinely cheap. An author who
 * writes a truthful-looking marker comment over code that does not actually
 * implement the remedy (or a bogus EXEMPT reason) passes this scan. Real
 * verification of the remedy's behavior lives in
 * `symbol-structural-ops.test.ts`'s "run off the paint path" describe block
 * (synchronous pending busy-state assertion, two concurrent-edit cases
 * proving a queued op is no longer dropped on an unrelated change, and the
 * reorder-stays-synchronous control case) — this file only guarantees that
 * REVIEW attention was paid at the call site, the same guarantee
 * `select-call-enrolment.test.ts` gives for its own invariant.
 *
 * ## Structural sibling
 *
 * Deliberately modeled on `select-call-enrolment.test.ts` (#2542, itself
 * modeled on `save-path-enrolment.test.ts`): derive scan roots from
 * `pnpm-workspace.yaml` (not a hand-typed package list, #2515's hole),
 * re-scan every run rather than trusting a snapshot, and require a marker
 * comment directly above each real call site naming which of
 * DEFERRED/EXEMPT it is plus a non-trivial reason.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { describe, it, expect } from "vitest";
import { deriveScanRoots, parseWorkspacePackageGlobs } from "./workspace-roots.js";

const here = dirname(fileURLToPath(import.meta.url));
/** `packages/` — this file lives at `packages/brink-studio/src/__tests__/`. */
const packagesRoot = resolve(here, "../../..");
const repoRoot = resolve(packagesRoot, "..");
const workspaceYamlPath = resolve(repoRoot, "pnpm-workspace.yaml");

/**
 * The three call-site names #2767 found and fixed — see this file's header
 * for why the scope stops here rather than covering every gated wasm op.
 */
const CALL_PATTERNS: ReadonlyArray<{ method: string; pattern: RegExp }> = [
  { method: "moveStitch", pattern: /\.moveStitch\(/ },
  { method: "promoteStitch", pattern: /\.promoteStitch\(/ },
  { method: "demoteKnot", pattern: /\.demoteKnot\(/ },
];

const MARKER = /^\/\/\s*PAINT-PATH-(DEFERRED|EXEMPT)\s+(\S+):\s*(.+)$/;
const SKIP_DIRS = new Set(["__tests__", "__mocks__", "dist", "node_modules", ".turbo"]);

/** Every real call site in `text`, as 0-based line index + method name. A
 *  line that is itself a `//` comment is prose about the call, or a method
 *  DEFINITION line quoted in a comment, never a call. */
function callLines(text: string): Array<{ index: number; method: string }> {
  const found: Array<{ index: number; method: string }> = [];
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith("//")) continue;
    const hit = CALL_PATTERNS.find(({ pattern }) => pattern.test(lines[i]));
    if (hit !== undefined) found.push({ index: i, method: hit.method });
  }
  return found;
}

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

/**
 * Every file today holding a real call site matched by {@link CALL_PATTERNS}
 * (grep-verified against this branch — see #2767's fix in
 * `symbolMenuActions.ts`). NOT the source of truth: {@link discoverCallSiteFiles}
 * re-derives it from every workspace `src/` root on every run, and the first
 * `it()` below fails the moment the two disagree.
 */
const SCANNED_FILES = [resolve(packagesRoot, "studio-ui/src/symbolMenuActions.ts")];

/** `moveStitch` + `promoteStitch` + `demoteKnot`, one call site each, all in
 *  `symbolMenuActions.ts`'s `dispatchSymbolAction`. Asserted exactly, not as
 *  "more than zero" — a scan that stops matching real calls must not leave
 *  every per-site check below vacuously green. */
const EXPECTED_CALL_SITES = 3;

interface CallSite {
  file: string;
  line: number; // 1-based, for a message a reader can jump to
  method: string;
  marker: { exempt: boolean; id: string; justification: string } | null;
}

/** Every real call site in `file`, paired with the marker (if any) directly
 *  above it — walking up through the contiguous `//` comment block with no
 *  blank/code line in between, same as `select-call-enrolment.test.ts`. */
function scanCallSites(file: string): CallSite[] {
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  return callLines(text).map(({ index, method }) => {
    let marker: CallSite["marker"] = null;
    for (let j = index - 1; j >= 0; j -= 1) {
      const above = lines[j].trim();
      if (!above.startsWith("//")) break;
      const match = MARKER.exec(above);
      if (match !== null) {
        marker = { exempt: match[1] === "EXEMPT", id: match[2], justification: match[3].trim() };
        break;
      }
    }
    return { file, line: index + 1, method, marker };
  });
}

describe("moveStitch/promoteStitch/demoteKnot call sites carry a paint-path marker (#2767)", () => {
  it("SCANNED_FILES matches a fresh workspace-wide scan", () => {
    const { withCallSite } = discoverCallSiteFiles();
    expect(withCallSite).toEqual([...SCANNED_FILES].sort());
  });

  it("finds exactly the expected number of call sites", () => {
    const total = SCANNED_FILES.reduce((sum, file) => sum + scanCallSites(file).length, 0);
    expect(total).toBe(EXPECTED_CALL_SITES);
  });

  for (const file of SCANNED_FILES) {
    const rel = file.slice(repoRoot.length + 1);
    describe(rel, () => {
      const sites = scanCallSites(file);
      it("has at least one call site (SCANNED_FILES sanity)", () => {
        expect(sites.length).toBeGreaterThan(0);
      });
      for (const site of sites) {
        it(`${site.method}() at line ${site.line} carries a non-exempt PAINT-PATH-DEFERRED marker with a real reason`, () => {
          expect(
            site.marker,
            `${rel}:${site.line} calls .${site.method}( with no PAINT-PATH-DEFERRED/-EXEMPT ` +
              "marker directly above it — see this file's header for the convention, and " +
              "symbolMenuActions.ts's runGatedStructuralOp for the established remedy.",
          ).not.toBeNull();
          // Every known call site today is DEFERRED (wrapped by
          // runGatedStructuralOp), not EXEMPT — there is no legitimately cheap
          // call to these three specific methods (the pure ops that back them
          // always run the gate; see structural_result.rs). An EXEMPT marker
          // here would mean either a mis-scoped call this file should not be
          // matching, or a real regression pretending otherwise — so, unlike
          // select-call-enrolment.test.ts, this suite does not carry a
          // "prove the exempt hatch is usable" case: there is nothing today
          // for it to legitimately exempt.
          expect(site.marker?.exempt).toBe(false);
          expect(site.marker?.justification.length ?? 0).toBeGreaterThan(10);
        });
      }
    });
  }

  it("every non-exempt id is used by exactly one call site", () => {
    const ids = SCANNED_FILES.flatMap((file) =>
      scanCallSites(file)
        .filter((s) => s.marker !== null && !s.marker.exempt)
        .map((s) => s.marker!.id),
    );
    const counts = new Map<string, number>();
    for (const id of ids) counts.set(id, (counts.get(id) ?? 0) + 1);
    for (const [id, count] of counts) {
      expect(count, `PAINT-PATH-DEFERRED id "${id}" is reused by ${count} call sites`).toBe(1);
    }
    expect([...counts.keys()].sort()).toEqual(["demote-knot", "move-stitch", "promote-stitch"]);
  });
});

describe("the marker parser itself (regression coverage for scanCallSites' logic)", () => {
  it("finds a marker on the line directly above a call, skipping trailing comment prose", () => {
    const text = [
      "function f(session) {",
      "  return runGatedStructuralOp(state, session, description, () =>",
      "    // PAINT-PATH-DEFERRED move-stitch: gated, deferred by runGatedStructuralOp above",
      "    // (trailing prose line, no marker here)",
      "    session.moveStitch(a, b, c, d),",
      "  );",
      "}",
    ].join("\n");
    const sites = callLines(text);
    expect(sites).toHaveLength(1);
  });

  it("does not match a bare method DEFINITION (no leading dot)", () => {
    const text = ["class EditorSessionHandle {", "  moveStitch(path, knot) {}", "}"].join("\n");
    expect(callLines(text)).toHaveLength(0);
  });

  it("does not match a call line that is itself commented out", () => {
    const text = ["// session.moveStitch(a, b, c, d);"].join("\n");
    expect(callLines(text)).toHaveLength(0);
  });

  it("treats an EXEMPT marker as exempt with its own justification", () => {
    const text = [
      "  // PAINT-PATH-EXEMPT scratch-id: a hypothetical reason long enough to pass",
      "  session.moveStitch(a, b, c, d);",
    ].join("\n");
    const [{ index }] = callLines(text);
    expect(index).toBe(1);
  });
});
