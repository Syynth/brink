/**
 * W6/#3299 — the execution-highlight POLICY, over real store state.
 *
 * The extension mechanics are pinned in ink-editor's own suite; the wasm
 * position→file:line road in `crates/brink-web`. What this suite pins is
 * the studio's policy: live vs paused kinds, suppressed-never-stale
 * under degraded, non-live statuses dark, wrong-file dark, and the range
 * riding along for the finer tiers.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore, ALL_CAPABILITIES } from "@brink/studio-store";
import { executionHighlightsFor } from "../execution-highlights";

function stateWith(overrides: {
  paused?: boolean;
  status?: string;
  program?: string | null;
  compiled?: string | null;
  position?: { container_idx: number; offset: number } | null;
  resolved?: { file: string; line: number; range_start: number; range_len: number } | null;
  selectedFrameIdx?: number | null;
  callStack?: { position?: { container_idx: number; offset: number } }[];
  pendingChoices?: { text: string; index: number; def_id?: string }[];
  visitIds?: { def_id: string; count: number }[];
}) {
  const store = createStudioStore();
  store.setState({
    sessionStatus: (overrides.status ?? "running") as never,
    sessionPaused: overrides.paused ?? false,
    programChecksum: overrides.program === undefined ? "abc" : overrides.program,
    compiledChecksum: overrides.compiled === undefined ? "abc" : overrides.compiled,
    selectedFrameIdx: overrides.selectedFrameIdx ?? null,
    debugState:
      overrides.position === null
        ? null
        : ({
            position: overrides.position ?? { container_idx: 3, offset: 7 },
            call_stack: overrides.callStack ?? [],
            pending_choices: overrides.pendingChoices ?? [],
            visit_ids: overrides.visitIds ?? [],
          } as never),
    _provider: {
      capabilities: ALL_CAPABILITIES,
      resolveDebugLine: vi.fn(
        () =>
          overrides.resolved ?? {
            file: "main.ink",
            line: 4,
            range_start: 100,
            range_len: 12,
          },
      ),
    } as never,
  });
  return store.getState();
}

describe("executionHighlightsFor — follow and hover bands (#3437)", () => {
  const src = { file: "main.ink", range_start: 10, range_end: 20 };
  const resolver = (file: string, start: number) =>
    file === "main.ink" ? { line: start === 10 ? 9 : 20, start, end: start + 5 } : null;

  it("bands the newest revealed line as `follow` while playing with follow on", () => {
    const st = stateWith({ status: "running", position: null });
    const out = executionHighlightsFor(
      {
        ...st,
        sessionLines: [{ text: "One", kind: "line" as const, tags: [], source: src }],
        followInEditor: true,
        followPaused: false,
        sessionHoverSource: null,
        _resolveSourceBytes: resolver as never,
      },
      "main.ink",
    );
    expect(out).toEqual([{ line: 10, kind: "follow" }]);
  });

  it("no follow band when off, paused by an edit, at a debugger pause, or for another file", () => {
    const base = {
      sessionLines: [{ text: "One", kind: "line" as const, tags: [], source: src }],
      sessionHoverSource: null,
      _resolveSourceBytes: resolver as never,
    };
    const st = stateWith({ status: "running", position: null });
    expect(executionHighlightsFor({ ...st, ...base, followInEditor: false, followPaused: false }, "main.ink")).toEqual([]);
    expect(executionHighlightsFor({ ...st, ...base, followInEditor: true, followPaused: true }, "main.ink")).toEqual([]);
    expect(executionHighlightsFor({ ...st, ...base, followInEditor: true, followPaused: false }, "other.ink")).toEqual([]);
    const paused = stateWith({ status: "running", paused: true, position: null });
    expect(
      executionHighlightsFor({ ...paused, ...base, followInEditor: true, followPaused: false }, "main.ink").filter((h) => h.kind === "follow"),
    ).toEqual([]);
  });

  it("bands the hovered transcript row's source as `hover`, even when idle", () => {
    const st = stateWith({ status: "none", position: null });
    const out = executionHighlightsFor(
      {
        ...st,
        sessionLines: [],
        followInEditor: true,
        followPaused: false,
        sessionHoverSource: { file: "main.ink", range_start: 30, range_end: 35 },
        _resolveSourceBytes: resolver as never,
      },
      "main.ink",
    );
    expect(out).toEqual([{ line: 21, kind: "hover" }]);
  });

  it("a source spanning several lines bands them all — hover and follow carry `endLine`", () => {
    const spanning = (file: string, start: number, end: number) =>
      file === "main.ink" ? { line: 86, endLine: 88, start, end } : null;
    const st = stateWith({ status: "running", position: null });
    const out = executionHighlightsFor(
      {
        ...st,
        sessionLines: [{ text: "@GRISWOLD: …", kind: "line" as const, tags: [], source: src }],
        followInEditor: true,
        followPaused: false,
        sessionHoverSource: { file: "main.ink", range_start: 30, range_end: 60 },
        _resolveSourceBytes: spanning as never,
      },
      "main.ink",
    );
    // Bars stack (ruled 2026-09-03): follow AND hover both band the lines.
    expect(out).toEqual([
      { line: 87, endLine: 89, kind: "follow" },
      { line: 87, endLine: 89, kind: "hover" },
    ]);
  });

  it("peek bands each forecast source; bars stack on a tinted line instead of deduping", () => {
    // Playing with a resolvable position: line 5 carries the live tint.
    const st = stateWith({ status: "running" });
    const out = executionHighlightsFor(
      {
        ...st,
        sessionLines: [{ text: "One", kind: "line" as const, tags: [], source: src }],
        followInEditor: true,
        followPaused: false,
        sessionHoverSource: null,
        sessionPeek: [
          { file: "main.ink", range_start: 10, range_end: 20 },
          { file: "other.ink", range_start: 10, range_end: 20 },
        ],
        _resolveSourceBytes: ((file: string, start: number) =>
          file === "main.ink" ? { line: 4, endLine: 4, start, end: start + 5 } : null) as never,
      },
      "main.ink",
    );
    expect(out).toEqual([
      { line: 5, kind: "live", rangeStart: 100, rangeLen: 12 },
      { line: 5, kind: "follow" },
      { line: 5, kind: "peek" },
    ]);
  });
});

/** A weave with three sibling choices under one knot (choice point A)
 *  and one unrelated choice under another knot (choice point B). */
function projectionFixture() {
  const container = (
    kind: string,
    startLine: number,
    endLine: number,
    extra: object = {},
  ) => ({
    kind,
    container: true,
    depth: kind === "knot" ? 0 : 1,
    start_line: startLine,
    start_char: 0,
    end_line: endLine,
    end_char: 99,
    handle: startLine,
    ...extra,
  });
  return {
    lines: [],
    spans: [
      container("knot", 0, 9),
      container("choice", 1, 2, { def_id: "$a", sticky: false, weave_depth: 1 }),
      container("choice", 3, 4, { def_id: "$b", sticky: false, weave_depth: 1 }),
      container("choice", 5, 6, { def_id: "$c", sticky: true, weave_depth: 1 }),
      container("knot", 10, 19),
      container("choice", 11, 12, { def_id: "$z", sticky: false, weave_depth: 1 }),
    ],
  } as never;
}

describe("choice-point visualization (W11/#3304)", () => {
  const CHOICES = { position: { container_idx: 3, offset: 7 } };

  it("presented choices light; rejected siblings dim with reasons by elimination", () => {
    const st = stateWith({
      ...CHOICES,
      status: "awaiting-choice",
      pendingChoices: [{ text: "Go", index: 0, def_id: "$c" }],
      visitIds: [{ def_id: "$a", count: 1 }],
    });
    const out = executionHighlightsFor(st, "main.ink", projectionFixture());
    // $c presented (line 6); $a once-only used (line 2); $b condition
    // false (line 4); $z belongs to ANOTHER choice point — untouched.
    expect(out).toEqual([
      { line: 6, kind: "live" },
      { line: 2, kind: "rejected", note: "once-only · used" },
      { line: 4, kind: "rejected", note: "condition false" },
    ]);
  });

  it("without a projection, the choice point falls back to the position band", () => {
    const st = stateWith({
      ...CHOICES,
      status: "awaiting-choice",
      pendingChoices: [{ text: "Go", index: 0, def_id: "$c" }],
    });
    expect(executionHighlightsFor(st, "main.ink", null)).toEqual([
      { line: 5, kind: "live", rangeStart: 100, rangeLen: 12 },
    ]);
  });

  it("paused at the choice point keeps the paused stop band alongside the lit set", () => {
    const st = stateWith({
      ...CHOICES,
      status: "awaiting-choice",
      paused: true,
      pendingChoices: [{ text: "Go", index: 0, def_id: "$c" }],
    });
    const out = executionHighlightsFor(st, "main.ink", projectionFixture());
    expect(out[0]).toEqual({ line: 5, kind: "paused", rangeStart: 100, rangeLen: 12 });
    expect(out).toContainEqual({ line: 6, kind: "live" });
  });

  it("degraded suppresses the choice bands too — suppressed, never stale", () => {
    const st = stateWith({
      ...CHOICES,
      status: "awaiting-choice",
      program: "old",
      compiled: "new",
      pendingChoices: [{ text: "Go", index: 0, def_id: "$c" }],
    });
    expect(executionHighlightsFor(st, "main.ink", projectionFixture())).toEqual([]);
  });
});

describe("executionHighlightsFor (W6/#3299)", () => {
  it("play is stepping: a running session lights the live band, 1-based", () => {
    expect(executionHighlightsFor(stateWith({}), "main.ink")).toEqual([
      { line: 5, kind: "live", rangeStart: 100, rangeLen: 12 },
    ]);
  });

  it("paused turns the band warning-kind", () => {
    expect(executionHighlightsFor(stateWith({ paused: true }), "main.ink")).toEqual([
      { line: 5, kind: "paused", rangeStart: 100, rangeLen: 12 },
    ]);
  });

  it("suppressed, never stale: a degraded session lights nothing", () => {
    expect(
      executionHighlightsFor(stateWith({ program: "old", compiled: "new" }), "main.ink"),
    ).toEqual([]);
  });

  it("only the position's own file lights", () => {
    expect(executionHighlightsFor(stateWith({}), "other.brink")).toEqual([]);
  });

  it("ended / errored / no-session states are dark", () => {
    for (const status of ["none", "ended", "error"]) {
      expect(executionHighlightsFor(stateWith({ status }), "main.ink")).toEqual([]);
    }
  });

  it("a selected non-top frame adds the accent frame band while paused (W8/#3301)", () => {
    const st = stateWith({
      paused: true,
      selectedFrameIdx: 1,
      callStack: [
        { position: { container_idx: 3, offset: 7 } },
        { position: { container_idx: 9, offset: 0 } },
      ],
    });
    // Per-position resolver: the top position is line 4, the frame line 20.
    (st._provider as unknown as { resolveDebugLine: unknown }).resolveDebugLine = (
      c: number,
    ) =>
      c === 9
        ? { file: "main.ink", line: 20, range_start: 700, range_len: 9 }
        : { file: "main.ink", line: 4, range_start: 100, range_len: 12 };
    expect(executionHighlightsFor(st, "main.ink")).toEqual([
      { line: 5, kind: "paused", rangeStart: 100, rangeLen: 12 },
      { line: 21, kind: "frame", rangeStart: 700, rangeLen: 9 },
    ]);
    // Not paused: the frame band never draws (selection is a paused-mode
    // affordance).
    const live = stateWith({
      selectedFrameIdx: 1,
      callStack: [
        { position: { container_idx: 3, offset: 7 } },
        { position: { container_idx: 9, offset: 0 } },
      ],
    });
    expect(executionHighlightsFor(live, "main.ink").map((h) => h.kind)).toEqual(["live"]);
  });

  it("no runtime position (or no debug info to resolve it) is dark", () => {
    expect(executionHighlightsFor(stateWith({ position: null }), "main.ink")).toEqual([]);
    const st = stateWith({});
    (st._provider as unknown as { resolveDebugLine: unknown }).resolveDebugLine = () => null;
    expect(executionHighlightsFor(st, "main.ink")).toEqual([]);
  });
});
