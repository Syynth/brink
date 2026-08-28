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
import {
  createStudioStore,
  loadProblemsPrefs,
  saveProblemsPrefs,
  type ProblemsPrefs,
} from "@brink/studio-store";
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
    expect(countBySeverity(ROWS)).toEqual({ error: 1, warning: 2, info: 2, prose: 0 });
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
  // Prose ON here: these cases are about SEVERITY filtering, and leaving
  // the source bucket at its off-by-default would silently drop any prose
  // row a case adds later.
  const all = { error: true, warning: true, info: true, prose: true };

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
    expect(groups[0]?.counts).toEqual({ error: 1, warning: 1, info: 0, prose: 0 });
    expect(groups[2]?.rows).toHaveLength(2);
  });

  it("groups the FILTERED set, so counts reflect what is on screen", () => {
    const visible = filterProblemRows(
      ROWS,
      { error: true, warning: false, info: false, prose: false },
      "",
    );
    const groups = groupProblemRows(visible);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.counts).toEqual({ error: 1, warning: 0, info: 0, prose: 0 });
  });
});

describe("summarizeCounts", () => {
  it("omits empty buckets and singularizes", () => {
    expect(summarizeCounts({ error: 1, warning: 2, info: 0, prose: 0 })).toBe(
      "1 error · 2 warnings",
    );
    expect(summarizeCounts({ error: 0, warning: 0, info: 3, prose: 0 })).toBe("3 info");
    expect(summarizeCounts({ error: 0, warning: 0, info: 0, prose: 0 })).toBe("");
    expect(summarizeCounts({ error: 0, warning: 0, info: 0, prose: 2 })).toBe("2 spelling");
  });
});

// ── Store slice ─────────────────────────────────────────────────────

describe("problems slice", () => {
  it("defaults: every severity shown, GROUPED by file, filter closed", () => {
    const s = createStudioStore().getState();
    // Prose is the exception and is off: spelling findings are opt-in
    // (ruled — the panel "FILTERS THEM OUT BY DEFAULT").
    expect(s.problemsSeverities).toEqual({
      error: true,
      warning: true,
      info: true,
      prose: false,
    });
    expect(s.problemsFilter).toBe("");
    expect(s.problemsFilterOpen).toBe(false);
    // Ruled 2026-08-25: a flat list of every diagnostic reads as noise.
    expect(s.problemsGrouped).toBe(true);
  });

  it("toggles one severity bucket at a time", () => {
    const store = createStudioStore();
    store.getState().toggleProblemSeverity("warning");
    expect(store.getState().problemsSeverities).toEqual({
      error: true,
      warning: false,
      info: true,
      prose: false,
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
    // Grouped is now the default, so the first toggle turns it OFF.
    store.getState().toggleProblemsGrouped();
    expect(store.getState().problemsGrouped).toBe(false);

    store.getState().toggleProblemsFileCollapsed("main.ink");
    expect(store.getState().problemsCollapsedFiles["main.ink"]).toBe(true);
    store.getState().toggleProblemsFileCollapsed("main.ink");
    expect(store.getState().problemsCollapsedFiles["main.ink"]).toBe(false);
  });
});

// ── Persistence (ruled 2026-08-25) ──────────────────────────────────

function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: () => null,
    length: 0,
  } as unknown as Storage;
}

describe("problems preferences persist", () => {
  it("round-trips severities and grouping", () => {
    const storage = memoryStorage();
    const prefs: ProblemsPrefs = {
      severities: { error: true, warning: false, info: false, prose: true },
      grouped: false,
    };
    saveProblemsPrefs(storage, prefs);
    expect(loadProblemsPrefs(storage)).toEqual(prefs);
  });

  it("defaults to shown+grouped when nothing is stored", () => {
    expect(loadProblemsPrefs(memoryStorage())).toEqual({
      severities: { error: true, warning: true, info: true, prose: false },
      grouped: true,
    });
  });

  it("only an explicit false hides a severity — a partial record never does", () => {
    const storage = memoryStorage();
    // A record written by an older build, missing keys entirely.
    storage.setItem("brink-studio.problems.v1", JSON.stringify({ severities: {} }));
    expect(loadProblemsPrefs(storage).severities).toEqual({
      error: true,
      warning: true,
      info: true,
      // The opposite rule for the source bucket: a record written before it
      // existed must not turn spelling rows on for an existing author.
      prose: false,
    });
  });

  it("survives garbage without throwing", () => {
    const storage = memoryStorage();
    storage.setItem("brink-studio.problems.v1", "{not json");
    expect(loadProblemsPrefs(storage).grouped).toBe(true);
  });

  it("toggling a severity or grouping reports through the persistence sink", () => {
    const store = createStudioStore();
    const written: ProblemsPrefs[] = [];
    store.getState().setProblemsPrefsSink((p) => void written.push(p));

    store.getState().toggleProblemSeverity("info");
    store.getState().toggleProblemsGrouped();

    expect(written).toHaveLength(2);
    expect(written[0]?.severities.info).toBe(false);
    expect(written[0]?.grouped).toBe(true);
    expect(written[1]?.grouped).toBe(false);
    // The second write carries the first toggle too — a sink that dropped
    // it would silently un-persist the severity on the next change.
    expect(written[1]?.severities.info).toBe(false);
  });

  it("applyProblemsPrefs restores a saved view at boot", () => {
    const store = createStudioStore();
    store.getState().applyProblemsPrefs({
      severities: { error: true, warning: false, info: false, prose: false },
      grouped: false,
    });
    expect(store.getState().problemsSeverities.warning).toBe(false);
    expect(store.getState().problemsGrouped).toBe(false);
  });
});
