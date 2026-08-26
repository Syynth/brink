/**
 * Problems panel controls (beta feedback 2026-08-25: "sorted by file,
 * filtered, and toggles for individual severity levels").
 *
 * Two halves: the pure filter/group model exported by ProblemsView, and
 * the store slice that the chrome-header actions and the panel body share
 * (they live in different React subtrees, so a store is the only channel).
 *
 * The defaults matter as much as the behavior: they must reproduce today's
 * panel exactly — every severity on, ungrouped, no filter — so shipping
 * the controls changes nothing until a control is touched.
 */

import { describe, expect, it } from "vitest";
import {
  countBySeverity,
  filterProblemRows,
  groupProblemRows,
  matchesProblemFilter,
  severityBucket,
  summarizeCounts,
  type ProblemRow,
} from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { Diagnostic } from "@brink/wasm-types";

function row(
  file: string,
  message: string,
  severity: Diagnostic["severity"],
  line = 1,
): ProblemRow {
  return {
    diagnostic: { file, message, severity, start: 0, end: 1 } as Diagnostic,
    location: `${file}:${line}:1`,
  };
}

const ROWS: ProblemRow[] = [
  row("clues/case_file.ink", "Unresolved divert target barter_menu", "Error", 74),
  row("clues/case_file.ink", "unreachable code after divert", "Warning", 122),
  row("menus/game_menu.ink", "unreachable code after divert", "Warning", 19),
  row("main.ink", "TODO: rewrite this scene", "Info", 12),
  row("main.ink", "consider naming this knot", "Hint", 30),
];

// ── Pure model ──────────────────────────────────────────────────────

describe("severityBucket", () => {
  it("folds Info and Hint into one advisory bucket", () => {
    expect(ROWS.map((r) => severityBucket(r.diagnostic))).toEqual([
      "error",
      "warning",
      "warning",
      "info",
      "info",
    ]);
  });
});

describe("countBySeverity", () => {
  it("counts every bucket over the unfiltered list", () => {
    expect(countBySeverity(ROWS)).toEqual({ error: 1, warning: 2, info: 2 });
  });
});

describe("matchesProblemFilter", () => {
  it("matches message or location, case-insensitively, and passes on empty", () => {
    const r = ROWS[0]!;
    expect(matchesProblemFilter(r, "")).toBe(true);
    expect(matchesProblemFilter(r, "  ")).toBe(true);
    expect(matchesProblemFilter(r, "BARTER")).toBe(true);
    expect(matchesProblemFilter(r, "case_file")).toBe(true);
    expect(matchesProblemFilter(r, "nonsense")).toBe(false);
  });
});

describe("filterProblemRows", () => {
  const all = { error: true, warning: true, info: true };

  it("passes everything through with defaults (today's behavior)", () => {
    expect(filterProblemRows(ROWS, all, "")).toHaveLength(ROWS.length);
  });

  it("drops a muted severity bucket, Hint along with Info", () => {
    const noInfo = filterProblemRows(ROWS, { ...all, info: false }, "");
    expect(noInfo).toHaveLength(3);
    expect(noInfo.some((r) => r.diagnostic.severity === "Hint")).toBe(false);
  });

  it("composes severity toggles with the text filter", () => {
    const out = filterProblemRows(ROWS, { ...all, error: false }, "divert");
    expect(out.map((r) => r.diagnostic.file)).toEqual([
      "clues/case_file.ink",
      "menus/game_menu.ink",
    ]);
  });

  it("preserves incoming order (already canonical: file, offset, errors first)", () => {
    const out = filterProblemRows(ROWS, all, "");
    expect(out).toEqual([...ROWS]);
  });
});

describe("groupProblemRows", () => {
  it("groups by file in first-appearance order with per-bucket counts", () => {
    const groups = groupProblemRows(ROWS);
    expect(groups.map((g) => g.file)).toEqual([
      "clues/case_file.ink",
      "menus/game_menu.ink",
      "main.ink",
    ]);
    expect(groups[0]?.counts).toEqual({ error: 1, warning: 1, info: 0 });
    expect(groups[2]?.rows).toHaveLength(2);
  });

  it("groups the FILTERED set, so counts reflect what is on screen", () => {
    const visible = filterProblemRows(ROWS, { error: true, warning: false, info: false }, "");
    const groups = groupProblemRows(visible);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.counts).toEqual({ error: 1, warning: 0, info: 0 });
  });
});

describe("summarizeCounts", () => {
  it("omits empty buckets and singularizes", () => {
    expect(summarizeCounts({ error: 1, warning: 2, info: 0 })).toBe("1 error · 2 warnings");
    expect(summarizeCounts({ error: 0, warning: 0, info: 3 })).toBe("3 info");
    expect(summarizeCounts({ error: 0, warning: 0, info: 0 })).toBe("");
  });
});

// ── Store slice ─────────────────────────────────────────────────────

describe("problems slice", () => {
  it("defaults reproduce today's panel: all severities, flat, no filter", () => {
    const s = createStudioStore().getState();
    expect(s.problemsSeverities).toEqual({ error: true, warning: true, info: true });
    expect(s.problemsFilter).toBe("");
    expect(s.problemsFilterOpen).toBe(false);
    expect(s.problemsGrouped).toBe(false);
  });

  it("toggles one severity bucket at a time", () => {
    const store = createStudioStore();
    store.getState().toggleProblemSeverity("warning");
    expect(store.getState().problemsSeverities).toEqual({
      error: true,
      warning: false,
      info: true,
    });
    store.getState().toggleProblemSeverity("warning");
    expect(store.getState().problemsSeverities.warning).toBe(true);
  });

  it("closing the filter clears the query — a hidden filter must not hide rows", () => {
    const store = createStudioStore();
    store.getState().toggleProblemsFilter();
    store.getState().setProblemsFilter("divert");
    expect(store.getState().problemsFilter).toBe("divert");

    store.getState().toggleProblemsFilter();
    expect(store.getState().problemsFilterOpen).toBe(false);
    expect(store.getState().problemsFilter).toBe("");
  });

  it("tracks grouping and per-file collapse independently", () => {
    const store = createStudioStore();
    store.getState().toggleProblemsGrouped();
    expect(store.getState().problemsGrouped).toBe(true);

    store.getState().toggleProblemsFileCollapsed("main.ink");
    expect(store.getState().problemsCollapsedFiles["main.ink"]).toBe(true);
    store.getState().toggleProblemsFileCollapsed("main.ink");
    expect(store.getState().problemsCollapsedFiles["main.ink"]).toBe(false);
  });
});
