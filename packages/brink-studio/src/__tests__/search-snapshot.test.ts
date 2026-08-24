/**
 * Frozen search snapshot — pure model + slice (PR B of
 * docs/search-results-cards-spec.md; ruled 2026-08-24).
 *
 * Covers the pure logic (single-region source diff, span mapping through
 * edits, line-info recompute, edited/stale semantics, the references
 * declaration anchor) and the slice integration: capture on
 * runSearch/showReferences, the compile-seam remap hook
 * (`setCompileResult` → `remapSearchSnapshot`), `refreshSearchSnapshot`
 * re-running the *frozen* origin (query snapshots ignore the input field's
 * current text; references snapshots re-resolve from the edit-mapped
 * declaration anchor), the context-lines knob, and the collapse-state map.
 */

import { describe, expect, it } from "vitest";
import {
  DEFAULT_SEARCH_CONTEXT_LINES,
  DEFAULT_SEARCH_OPTIONS,
  MAX_SEARCH_CONTEXT_LINES,
  captureSnapshot,
  clampContextLines,
  createStudioStore,
  diffSources,
  lineInfoAt,
  mapSpan,
  remapSnapshot,
  searchSources,
  buildSearchPattern,
  type SearchSnapshot,
  type SnapshotOrigin,
} from "@brink/studio-store";

// ── Harness ─────────────────────────────────────────────────────────

/** Build a query snapshot over literal sources, the way the slice does. */
function snapshotOf(
  files: Record<string, string>,
  query = "gold",
  origin?: SnapshotOrigin,
): SearchSnapshot {
  const built = buildSearchPattern(query, DEFAULT_SEARCH_OPTIONS);
  if (!built.ok) throw new Error(built.error);
  const sources = Object.entries(files)
    .map(([path, source]) => ({ path, source }))
    .sort((a, b) => (a.path < b.path ? -1 : 1));
  const result = searchSources(sources, built.pattern);
  return captureSnapshot(
    result,
    origin ?? { kind: "query", query, options: DEFAULT_SEARCH_OPTIONS },
    (p) => files[p] ?? null,
  );
}

interface FakeSession {
  listFiles(): { path: string }[];
  getFileSource(path: string): string | null;
  updateFile(path: string, source: string): void;
  findReferencesAt(
    path: string,
    offset: number,
    includeDeclaration: boolean,
  ): { file: string; start: number; end: number }[];
}

function sliceHarness(files: Record<string, string>) {
  const sources = new Map(Object.entries(files));
  const referenceCalls: Array<{ path: string; offset: number }> = [];
  let referenceAnswer: { file: string; start: number; end: number }[] = [];
  const session: FakeSession = {
    listFiles: () => [...sources.keys()].map((path) => ({ path })),
    getFileSource: (path) => sources.get(path) ?? null,
    updateFile: (path, source) => sources.set(path, source),
    findReferencesAt: (path, offset) => {
      referenceCalls.push({ path, offset });
      return referenceAnswer;
    },
  };
  const store = createStudioStore();
  store.setState({
    _project: { getSession: () => session } as never,
    _documents: { invalidateFile: () => {}, triggerCompile: () => {} } as never,
  });
  return {
    store,
    sources,
    session,
    referenceCalls,
    setReferenceAnswer(locs: { file: string; start: number; end: number }[]) {
      referenceAnswer = locs;
    },
    /** Mutate a file and deliver the compile seam, like an edit debounce. */
    edit(path: string, source: string) {
      sources.set(path, source);
      store.getState().setCompileResult([], { errors: 0, warnings: 0 }, [], null);
    },
  };
}

// ── diffSources ─────────────────────────────────────────────────────

describe("diffSources", () => {
  it("returns null for identical texts", () => {
    expect(diffSources("abc", "abc")).toBeNull();
  });

  it("finds an insertion", () => {
    // "hello world" → "hello brave world": inserted "brave " at 6.
    expect(diffSources("hello world", "hello brave world")).toEqual({
      start: 6,
      oldEnd: 6,
      newEnd: 12,
    });
  });

  it("finds a deletion", () => {
    expect(diffSources("hello brave world", "hello world")).toEqual({
      start: 6,
      oldEnd: 12,
      newEnd: 6,
    });
  });

  it("finds a replacement", () => {
    expect(diffSources("one two three", "one 2 three")).toEqual({
      start: 4,
      oldEnd: 7,
      newEnd: 5,
    });
  });

  it("never lets prefix and suffix overlap on repeated text", () => {
    // "aba" → "ababa": prefix consumes all of the old text, so the suffix
    // scan must be bounded to zero (a naive scan would double-count).
    expect(diffSources("aba", "ababa")).toEqual({ start: 3, oldEnd: 3, newEnd: 5 });
  });
});

// ── mapSpan ─────────────────────────────────────────────────────────

describe("mapSpan", () => {
  const insertion = { start: 10, oldEnd: 10, newEnd: 15 };
  const deletion = { start: 10, oldEnd: 15, newEnd: 10 };

  it("leaves spans before the change alone (insertion exactly at the span end included)", () => {
    expect(mapSpan(insertion, 2, 6)).toEqual({ start: 2, end: 6, touched: false });
    expect(mapSpan(insertion, 6, 10)).toEqual({ start: 6, end: 10, touched: false });
  });

  it("shifts spans after the change (insertion exactly at the span start included)", () => {
    expect(mapSpan(insertion, 20, 24)).toEqual({ start: 25, end: 29, touched: false });
    expect(mapSpan(insertion, 10, 14)).toEqual({ start: 15, end: 19, touched: false });
    expect(mapSpan(deletion, 15, 20)).toEqual({ start: 10, end: 15, touched: false });
  });

  it("expands a span the change lands inside, and reports touched", () => {
    // Change region [10,15)→[10,20) sits inside the span [8,18).
    const grown = { start: 10, oldEnd: 15, newEnd: 20 };
    expect(mapSpan(grown, 8, 18)).toEqual({ start: 8, end: 23, touched: true });
  });

  it("expands a span that the change overlaps partially", () => {
    // Change [10,15)→[10,12) begins inside the span [12,20): the span is
    // pulled back to the region start and its tail shifts.
    const shrunk = { start: 10, oldEnd: 15, newEnd: 12 };
    expect(mapSpan(shrunk, 12, 20)).toEqual({ start: 10, end: 17, touched: true });
  });

  it("clamps a span swallowed whole by a deletion", () => {
    // Deleting [10,15) removes the span [11,14) entirely.
    expect(mapSpan(deletion, 11, 14)).toEqual({ start: 10, end: 10, touched: true });
  });
});

// ── lineInfoAt ──────────────────────────────────────────────────────

describe("lineInfoAt", () => {
  const source = "first line\nsecond gold line\nthird";

  it("computes 1-based line, line text and in-line span", () => {
    const start = source.indexOf("gold");
    expect(lineInfoAt(source, start, start + 4)).toEqual({
      line: 2,
      lineText: "second gold line",
      lineStart: 7,
      lineEnd: 11,
    });
  });

  it("handles offset zero and a source starting with a newline", () => {
    expect(lineInfoAt("\nabc", 0, 0)).toEqual({
      line: 1,
      lineText: "",
      lineStart: 0,
      lineEnd: 0,
    });
    expect(lineInfoAt("abc", 0, 3)).toEqual({
      line: 1,
      lineText: "abc",
      lineStart: 0,
      lineEnd: 3,
    });
  });

  it("clamps a span that crosses the line end to the line", () => {
    expect(lineInfoAt("ab\ncd", 1, 4).lineEnd).toBe(2);
  });
});

// ── captureSnapshot ─────────────────────────────────────────────────

describe("captureSnapshot", () => {
  it("freezes matches with stable ids, clean flags, and the source baseline", () => {
    const snap = snapshotOf({ "a.ink": "gold here\nmore gold" });
    expect(snap.totalMatches).toBe(2);
    const [file] = snap.files;
    expect(file.path).toBe("a.ink");
    expect(file.seenSource).toBe("gold here\nmore gold");
    expect(file.deleted).toBe(false);
    expect(file.matches.map((m) => m.id)).toEqual(["a.ink#0", "a.ink#1"]);
    expect(file.matches.every((m) => !m.edited && !m.stale)).toBe(true);
  });

  it("captures the references declaration anchor with its text", () => {
    const files = { "a.ink": "== barter ==\n-> barter" };
    const snap = captureSnapshot(
      { files: [], totalMatches: 0, capped: false },
      { kind: "references", symbol: "barter" },
      (p) => files[p as keyof typeof files] ?? null,
      { file: "a.ink", start: 3, end: 9 },
    );
    expect(snap.anchor).toMatchObject({
      file: "a.ink",
      start: 3,
      end: 9,
      text: "barter",
      edited: false,
      stale: false,
    });
  });
});

// ── remapSnapshot ───────────────────────────────────────────────────

describe("remapSnapshot", () => {
  it("returns the same object when nothing changed", () => {
    const files = { "a.ink": "some gold" };
    const snap = snapshotOf(files);
    expect(remapSnapshot(snap, (p) => files[p as keyof typeof files] ?? null)).toBe(snap);
  });

  it("shifts spans across an edit before the match without flagging it", () => {
    const snap = snapshotOf({ "a.ink": "intro\ngold coin" });
    const live = "longer intro line\ngold coin";
    const remapped = remapSnapshot(snap, () => live);
    const [match] = remapped.files[0].matches;
    expect(live.slice(match.start, match.end)).toBe("gold");
    expect(match).toMatchObject({ edited: false, stale: false, line: 2, lineText: "gold coin" });
    expect(remapped.files[0].seenSource).toBe(live);
  });

  it("flags a match the edit lands inside as edited and stale", () => {
    const snap = snapshotOf({ "a.ink": "take the gold now" });
    const remapped = remapSnapshot(snap, () => "take the gXld now");
    const [match] = remapped.files[0].matches;
    expect(match.edited).toBe(true);
    expect(match.stale).toBe(true);
  });

  it("keeps a touched match fresh when the new text still matches the query", () => {
    // Case-insensitive default: "gold" → "GOLD" is touched but still a hit.
    const snap = snapshotOf({ "a.ink": "take the gold now" });
    const remapped = remapSnapshot(snap, () => "take the GOLD now");
    const [match] = remapped.files[0].matches;
    expect(match.edited).toBe(true);
    expect(match.stale).toBe(false);
  });

  it("keeps edited sticky across an undo that clears staleness", () => {
    const files = { "a.ink": "take the gold now" };
    const snap = snapshotOf(files);
    const broken = remapSnapshot(snap, () => "take the gXld now");
    const restored = remapSnapshot(broken, () => "take the gold now");
    const [match] = restored.files[0].matches;
    expect(match.stale).toBe(false);
    expect(match.edited).toBe(true);
  });

  it("uses text equality for references-origin staleness", () => {
    const source = "== barter ==\n-> barter";
    const refs = captureSnapshot(
      {
        files: [
          {
            path: "a.ink",
            matches: [
              {
                start: 3,
                end: 9,
                line: 1,
                lineText: "== barter ==",
                lineStart: 3,
                lineEnd: 9,
                text: "barter",
              },
            ],
          },
        ],
        totalMatches: 1,
        capped: false,
      },
      { kind: "references", symbol: "barter" },
      () => source,
    );
    const remapped = remapSnapshot(refs, () => "== Barter ==\n-> barter");
    expect(remapped.files[0].matches[0].stale).toBe(true);
  });

  it("marks every match stale when the file is gone, and recovers when it returns", () => {
    const snap = snapshotOf({ "a.ink": "some gold" });
    const gone = remapSnapshot(snap, () => null);
    expect(gone.files[0].deleted).toBe(true);
    expect(gone.files[0].matches[0]).toMatchObject({ edited: true, stale: true });

    const back = remapSnapshot(gone, () => "some gold");
    expect(back.files[0].deleted).toBe(false);
    expect(back.files[0].matches[0].stale).toBe(false);
  });

  it("maps the references anchor through edits and flags a renamed declaration", () => {
    const files: Record<string, string> = { "a.ink": "== barter ==" };
    const snap = captureSnapshot(
      { files: [], totalMatches: 0, capped: false },
      { kind: "references", symbol: "barter" },
      (p) => files[p] ?? null,
      { file: "a.ink", start: 3, end: 9 },
    );

    files["a.ink"] = "// note\n== barter ==";
    const shifted = remapSnapshot(snap, (p) => files[p] ?? null);
    expect(shifted.anchor).toMatchObject({ start: 11, end: 17, edited: false, stale: false });

    files["a.ink"] = "// note\n== trade ==";
    const renamed = remapSnapshot(shifted, (p) => files[p] ?? null);
    expect(renamed.anchor?.stale).toBe(true);
  });
});

// ── clampContextLines ───────────────────────────────────────────────

describe("clampContextLines", () => {
  it("clamps to the knob's range and floors fractions", () => {
    expect(clampContextLines({ before: -3, after: 99 })).toEqual({
      before: 0,
      after: MAX_SEARCH_CONTEXT_LINES,
    });
    expect(clampContextLines({ before: 1.9, after: Number.NaN })).toEqual({
      before: 1,
      after: 0,
    });
  });
});

// ── Slice integration ───────────────────────────────────────────────

describe("search slice snapshots", () => {
  it("runSearch captures a query-origin snapshot with ids and baselines", () => {
    const { store } = sliceHarness({ "a.ink": "gold\nmore gold" });
    store.getState().setSearchQuery("gold");
    store.getState().runSearch();
    const snap = store.getState().searchResults;
    expect(snap?.origin).toEqual({
      kind: "query",
      query: "gold",
      options: DEFAULT_SEARCH_OPTIONS,
    });
    expect(snap?.files[0].matches.map((m) => m.id)).toEqual(["a.ink#0", "a.ink#1"]);
    expect(snap?.files[0].seenSource).toBe("gold\nmore gold");
  });

  it("showReferences captures a references-origin snapshot with the anchor", () => {
    const { store } = sliceHarness({ "a.ink": "== barter ==\n-> barter" });
    store
      .getState()
      .showReferences(
        "barter",
        [{ file: "a.ink", start: 16, end: 22 }],
        { file: "a.ink", start: 3, end: 9 },
      );
    const snap = store.getState().searchResults;
    expect(snap?.origin).toEqual({ kind: "references", symbol: "barter" });
    expect(snap?.anchor).toMatchObject({ file: "a.ink", start: 3, end: 9, text: "barter" });
    expect(store.getState().searchMode).toEqual({ kind: "references", symbol: "barter" });
  });

  it("remaps the snapshot through the compile seam instead of dropping rows", () => {
    const harness = sliceHarness({ "a.ink": "intro\ngold coin" });
    harness.store.getState().setSearchQuery("gold");
    harness.store.getState().runSearch();

    harness.edit("a.ink", "much longer intro\ngold coin");

    const snap = harness.store.getState().searchResults;
    const match = snap?.files[0]?.matches[0];
    expect(match && "much longer intro\ngold coin".slice(match.start, match.end)).toBe("gold");
    expect(match?.edited).toBe(false);
    expect(snap?.totalMatches).toBe(1);
  });

  it("flags rather than removes a match the edit breaks (frozen snapshot)", () => {
    const harness = sliceHarness({ "a.ink": "take the gold now" });
    harness.store.getState().setSearchQuery("gold");
    harness.store.getState().runSearch();

    harness.edit("a.ink", "take the silver now");

    const snap = harness.store.getState().searchResults;
    expect(snap?.totalMatches).toBe(1);
    expect(snap?.files[0].matches[0]).toMatchObject({ edited: true, stale: true });
  });

  it("refreshSearchSnapshot re-runs the frozen query, not the input field's text", () => {
    const harness = sliceHarness({ "a.ink": "gold and silver" });
    harness.store.getState().setSearchQuery("gold");
    harness.store.getState().runSearch();

    // The user typed a new query but never ran it (references-style hold).
    harness.store.getState().setSearchQuery("silver");
    harness.sources.set("a.ink", "gold gold and silver");
    harness.store.getState().refreshSearchSnapshot();

    const snap = harness.store.getState().searchResults;
    expect(snap?.origin).toMatchObject({ kind: "query", query: "gold" });
    expect(snap?.totalMatches).toBe(2);
  });

  it("refreshSearchSnapshot re-resolves references from the edit-mapped declaration anchor", () => {
    const harness = sliceHarness({ "a.ink": "== barter ==\n-> barter" });
    harness.store
      .getState()
      .showReferences(
        "barter",
        [{ file: "a.ink", start: 16, end: 22 }],
        { file: "a.ink", start: 3, end: 9 },
      );

    // Edit ABOVE the declaration: the original click offset is stale; the
    // mapped anchor is 8 characters further in.
    harness.edit("a.ink", "// hdr\n== barter ==\n-> barter");
    harness.setReferenceAnswer([{ file: "a.ink", start: 23, end: 29 }]);
    harness.store.getState().refreshSearchSnapshot();

    expect(harness.referenceCalls).toEqual([{ path: "a.ink", offset: 10 }]);
    const snap = harness.store.getState().searchResults;
    expect(snap?.files[0].matches[0]).toMatchObject({ start: 23, end: 29 });
    expect(snap?.anchor).toMatchObject({ start: 10, end: 16, text: "barter" });
  });

  it("keeps the snapshot when references re-resolution throws", () => {
    const harness = sliceHarness({ "a.ink": "== barter ==\n-> barter" });
    harness.store
      .getState()
      .showReferences(
        "barter",
        [{ file: "a.ink", start: 16, end: 22 }],
        { file: "a.ink", start: 3, end: 9 },
      );
    const before = harness.store.getState().searchResults;
    harness.session.findReferencesAt = () => {
      throw new Error("mid-edit");
    };
    harness.store.getState().refreshSearchSnapshot();
    expect(harness.store.getState().searchResults).toBe(before);
  });

  it("does nothing on a references refresh without an anchor", () => {
    const harness = sliceHarness({ "a.ink": "-> barter" });
    harness.store.getState().showReferences("barter", [{ file: "a.ink", start: 3, end: 9 }]);
    const before = harness.store.getState().searchResults;
    harness.store.getState().refreshSearchSnapshot();
    expect(harness.store.getState().searchResults).toBe(before);
    expect(harness.referenceCalls).toEqual([]);
  });

  it("clamps the context-lines knob and starts at the ruled default", () => {
    const { store } = sliceHarness({});
    expect(store.getState().searchContextLines).toEqual(DEFAULT_SEARCH_CONTEXT_LINES);
    store.getState().setSearchContextLines({ before: 4, after: 99 });
    expect(store.getState().searchContextLines).toEqual({
      before: 4,
      after: MAX_SEARCH_CONTEXT_LINES,
    });
  });

  it("tracks per-card collapse, clears overrides on a new snapshot, keeps the all-flag", () => {
    const harness = sliceHarness({ "a.ink": "gold" });
    const state = () => harness.store.getState();
    state().setSearchCardCollapsed("a.ink#0", true);
    state().setAllSearchCardsCollapsed(true);
    // Collapse-all resets per-card overrides to the new default.
    expect(state().searchCardCollapsed).toEqual({});
    expect(state().searchAllCollapsed).toBe(true);

    state().setSearchCardCollapsed("a.ink#0", false);
    state().setSearchQuery("gold");
    state().runSearch();
    // New snapshot, new card identities: overrides die, the flag survives.
    expect(state().searchCardCollapsed).toEqual({});
    expect(state().searchAllCollapsed).toBe(true);
  });
});
