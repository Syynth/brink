/**
 * Editable search results buffer tests (issue #322, Track V, design D).
 *
 * Two halves:
 *
 *  - The pure model (`buildResultsRows` / `mapRowEditToSource`): rows mirror
 *    the search result (file headers + match lines), and an edited match line
 *    maps back to a source `ReplacementEdit` over the *whole source line* —
 *    with the stale / multi-line / no-op guards the locked design requires.
 *  - The CM6 surface (`SearchResultsBuffer`): renders the synthetic document,
 *    keeps headers / prefixes read-only while match-line source text is
 *    editable, routes committed match-row edits to `onSourceEdit`, and — per
 *    the CM6 teardown contract — leaves no DOM behind after `destroy()`.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  buildResultsRows,
  mapRowEditToSource,
  SearchResultsBuffer,
  searchSources,
  buildSearchPattern,
  DEFAULT_SEARCH_OPTIONS,
  DEFAULT_COMMIT_DELAY_MS,
  SEARCH_RESULT_CAP,
  type ProjectSearchResult,
  type ResultRow,
  type SearchQueryOptions,
} from "@brink/studio-store";
import { runScopeHandlers, type EditorView } from "@codemirror/view";

function options(over: Partial<SearchQueryOptions> = {}): SearchQueryOptions {
  return { ...DEFAULT_SEARCH_OPTIONS, ...over };
}

function search(
  files: ReadonlyArray<{ path: string; source: string }>,
  query: string,
  over: Partial<SearchQueryOptions> = {},
  cap?: number,
): ProjectSearchResult {
  const built = buildSearchPattern(query, options(over));
  if (!built.ok) throw new Error(built.error);
  return searchSources(files, built.pattern, cap);
}

function matchRow(row: ResultRow): Extract<ResultRow, { kind: "match" }> {
  if (row.kind !== "match") throw new Error(`expected match row, got ${row.kind}`);
  return row;
}

// ── buildResultsRows ─────────────────────────────────────────────────

describe("buildResultsRows", () => {
  const files = [
    { path: "a.ink", source: "The lights dim.\nA figure steps into the light.\n" },
    { path: "b.ink", source: "the end\n" },
  ];

  it("renders a header per file and a match line per match, in order", () => {
    const model = buildResultsRows(search(files, "the"));
    const lines = model.text.split("\n");
    expect(lines[0]).toBe("a.ink (2)");
    expect(lines[1]).toBe("  1: The lights dim.");
    expect(lines[2]).toBe("  2: A figure steps into the light.");
    expect(lines[3]).toBe(""); // blank separator between files
    expect(lines[4]).toBe("b.ink (1)");
    expect(lines[5]).toBe("  1: the end");
  });

  it("row table lines up with the buffer lines (header | match | blank)", () => {
    const model = buildResultsRows(search(files, "the"));
    expect(model.rows.map((r) => r.kind)).toEqual([
      "header",
      "match",
      "match",
      "blank",
      "header",
      "match",
    ]);
    const first = matchRow(model.rows[1]);
    expect(first.path).toBe("a.ink");
    // sourceCol is the width of the "  N: " prefix, so the source text at that
    // column reproduces the original line.
    const line1 = model.text.split("\n")[1];
    expect(line1.slice(first.sourceCol)).toBe("The lights dim.");
  });

  it("is empty for an empty result", () => {
    const model = buildResultsRows({ files: [], totalMatches: 0, capped: false });
    expect(model.text).toBe("");
    expect(model.rows).toEqual([]);
  });

  it("preserves the capped result (rows only reflect what searchSources kept)", () => {
    const big = { path: "big.ink", source: "x ".repeat(SEARCH_RESULT_CAP + 50) };
    const result = search([big], "x");
    expect(result.capped).toBe(true);
    const model = buildResultsRows(result);
    // header + exactly cap match lines, no more.
    const matchLines = model.rows.filter((r) => r.kind === "match").length;
    expect(matchLines).toBe(SEARCH_RESULT_CAP);
  });
});

// ── mapRowEditToSource ───────────────────────────────────────────────

describe("mapRowEditToSource", () => {
  const source = "The lights dim.\nA figure steps into the light.\n";

  it("maps an edited match line to a whole-source-line replacement", () => {
    const model = buildResultsRows(search([{ path: "a.ink", source }], "figure"));
    const row = matchRow(model.rows[1]);
    // The buffer line is "  2: A figure steps into the light."; user rewrites
    // the source portion.
    const edit = mapRowEditToSource(row, "  2: A shadow steps into the light.", source);
    expect(edit).not.toBeNull();
    // The edit replaces the whole 2nd source line's span.
    expect(source.slice(edit!.start, edit!.end)).toBe(
      "A figure steps into the light.",
    );
    expect(edit!.text).toBe("A shadow steps into the light.");
    // Applying it yields the intended source.
    const applied =
      source.slice(0, edit!.start) + edit!.text + source.slice(edit!.end);
    expect(applied).toBe("The lights dim.\nA shadow steps into the light.\n");
  });

  it("returns null for a no-op edit (line unchanged)", () => {
    const model = buildResultsRows(search([{ path: "a.ink", source }], "figure"));
    const row = matchRow(model.rows[1]);
    expect(
      mapRowEditToSource(row, "  2: A figure steps into the light.", source),
    ).toBeNull();
  });

  it("skips a stale row (live source no longer matches the recorded line)", () => {
    const model = buildResultsRows(search([{ path: "a.ink", source }], "figure"));
    const row = matchRow(model.rows[1]);
    // File changed underneath — the recorded line is gone.
    const changed = "Totally different first line.\nAnd a different second.\n";
    expect(
      mapRowEditToSource(row, "  2: A shadow steps into the light.", changed),
    ).toBeNull();
  });

  it("skips a multi-line match (recorded lineText is only a prefix of the span)", () => {
    // A regex match spanning a newline: match.text has the newline, so
    // end-start != lineText.length and the row is not line-rewritable.
    const src = "alpha\nbeta gamma";
    const result = search([{ path: "a.ink", source: src }], "alpha[\\s\\S]beta", {
      regex: true,
    });
    const model = buildResultsRows(result);
    const row = matchRow(model.rows[1]);
    expect(row.match.text).toContain("\n"); // the match spans a newline
    expect(mapRowEditToSource(row, "  1: EDITED", src)).toBeNull();
  });

  it("clamps an over-deletion into the prefix to an empty source line", () => {
    const model = buildResultsRows(search([{ path: "a.ink", source }], "figure"));
    const row = matchRow(model.rows[1]);
    // User deleted the whole line including the prefix.
    const edit = mapRowEditToSource(row, "", source);
    expect(edit).not.toBeNull();
    expect(edit!.text).toBe("");
    expect(source.slice(edit!.start, edit!.end)).toBe(
      "A figure steps into the light.",
    );
  });
});

// ── SearchResultsBuffer (CM6 surface) ────────────────────────────────

function makeBuffer(
  result: ProjectSearchResult,
  sources: Record<string, string>,
  commitDelayMs = 0,
) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const onSourceEdit = vi.fn();
  const onReveal = vi.fn();
  const buffer = new SearchResultsBuffer(host, result, {
    getSource: (path) => sources[path] ?? null,
    onSourceEdit,
    onReveal,
    // Tests commit synchronously by default (single atomic transaction); the
    // debounce path is exercised explicitly with fake timers below.
    commitDelayMs,
  });
  return { host, buffer, onSourceEdit, onReveal };
}

/** Reach into the private EditorView for direct dispatch in tests. */
function viewOf(buffer: SearchResultsBuffer): EditorView {
  return (buffer as unknown as { view: EditorView }).view;
}

/** Run the "editor" keymap scope for a key, as CM6 would on a real keydown. */
function runKey(view: EditorView, key: string): boolean {
  const event = new KeyboardEvent("keydown", { key });
  return runScopeHandlers(view, event, "editor");
}

beforeEach(() => {
  document.body.replaceChildren();
});

describe("SearchResultsBuffer", () => {
  const source = "The lights dim.\nA figure steps into the light.\n";

  it("mounts a CM6 editor whose document mirrors the results", () => {
    const { host, buffer } = makeBuffer(search([{ path: "a.ink", source }], "the"), {
      "a.ink": source,
    });
    const editor = host.querySelector(".cm-editor");
    expect(editor).not.toBeNull();
    expect(host.textContent).toContain("a.ink (2)");
    expect(host.textContent).toContain("A figure steps into the light.");
    buffer.destroy();
  });

  it("routes a committed match-row edit to onSourceEdit", () => {
    const { buffer, onSourceEdit } = makeBuffer(
      search([{ path: "a.ink", source }], "figure"),
      { "a.ink": source },
    );
    // Buffer line 2 (0-based line index 1) is the match line; overwrite the
    // word "figure" with "shadow" in the source portion. Compute the doc
    // offset of "figure" within the synthetic buffer.
    const view = viewOf(buffer);
    const docText = view.state.doc.toString();
    const at = docText.indexOf("figure");
    expect(at).toBeGreaterThan(0);
    view.dispatch({ changes: { from: at, to: at + "figure".length, insert: "shadow" } });
    expect(onSourceEdit).toHaveBeenCalledTimes(1);
    const [path, edit] = onSourceEdit.mock.calls[0];
    expect(path).toBe("a.ink");
    expect(edit.text).toBe("A shadow steps into the light.");
    buffer.destroy();
  });

  it("rejects edits to a header line (read-only)", () => {
    const { buffer, onSourceEdit } = makeBuffer(
      search([{ path: "a.ink", source }], "figure"),
      { "a.ink": source },
    );
    const view = viewOf(buffer);
    const before = view.state.doc.toString();
    // Try to insert at offset 0 (inside the header line).
    view.dispatch({ changes: { from: 0, to: 0, insert: "XXX" } });
    expect(view.state.doc.toString()).toBe(before); // change filtered out
    expect(onSourceEdit).not.toHaveBeenCalled();
    buffer.destroy();
  });

  it("setResult swaps the document without routing a source edit", () => {
    const { buffer, onSourceEdit } = makeBuffer(
      search([{ path: "a.ink", source }], "figure"),
      { "a.ink": source },
    );
    const other = "the beginning\n";
    buffer.setResult(search([{ path: "b.ink", source: other }], "the"));
    const view = viewOf(buffer);
    expect(view.state.doc.toString()).toContain("b.ink (1)");
    expect(onSourceEdit).not.toHaveBeenCalled();
    buffer.destroy();
  });

  it("destroy() removes every node it mounted (CM6 teardown contract)", () => {
    const { host, buffer } = makeBuffer(search([{ path: "a.ink", source }], "the"), {
      "a.ink": source,
    });
    expect(host.querySelector(".cm-editor")).not.toBeNull();
    buffer.destroy();
    expect(host.querySelector(".cm-editor")).toBeNull();
    expect(host.childElementCount).toBe(0);
  });

  // ── Issue 1: read-only contract vs inserted newlines ────────────────

  it("rejects inserting a newline mid-match-line (row table must not desync)", () => {
    const { buffer, onSourceEdit } = makeBuffer(
      search([{ path: "a.ink", source }], "figure"),
      { "a.ink": source },
    );
    const view = viewOf(buffer);
    const before = view.state.doc.toString();
    const beforeLines = view.state.doc.lines;
    // Enter mid-match-line: insert a bare "\n" inside the editable source region.
    const docText = view.state.doc.toString();
    const at = docText.indexOf("figure");
    view.dispatch({ changes: { from: at, to: at, insert: "\n" } });
    // Filtered out entirely — no split, doc + line count unchanged.
    expect(view.state.doc.toString()).toBe(before);
    expect(view.state.doc.lines).toBe(beforeLines);
    expect(onSourceEdit).not.toHaveBeenCalled();
    buffer.destroy();
  });

  it("rejects a multi-line paste over a match-line word (no source corruption)", () => {
    const { buffer, onSourceEdit } = makeBuffer(
      search([{ path: "a.ink", source }], "figure"),
      { "a.ink": source },
    );
    const view = viewOf(buffer);
    const before = view.state.doc.toString();
    const docText = view.state.doc.toString();
    const at = docText.indexOf("figure");
    // Paste containing a newline over "figure".
    view.dispatch({
      changes: { from: at, to: at + "figure".length, insert: "INJECT\nEDNEWLINE" },
    });
    expect(view.state.doc.toString()).toBe(before); // rejected wholesale
    expect(onSourceEdit).not.toHaveBeenCalled();
    buffer.destroy();
  });

  // ── Issues 2 & 3: caret survival + no per-keystroke reset ────────────

  it("preserves the caret across a same-content setResult (no yank to 0)", () => {
    const { buffer } = makeBuffer(search([{ path: "a.ink", source }], "figure"), {
      "a.ink": source,
    });
    const view = viewOf(buffer);
    const at = view.state.doc.toString().indexOf("figure");
    view.dispatch({ selection: { anchor: at } });
    // Host re-runs search after a commit and pushes the same result back.
    buffer.setResult(search([{ path: "a.ink", source }], "figure"));
    // Same content ⇒ untouched doc ⇒ caret stays put (not collapsed to 0).
    expect(view.state.selection.main.head).toBe(at);
    buffer.destroy();
  });

  it("clamps (does not zero) the caret when a changed setResult shrinks the doc", () => {
    const { buffer } = makeBuffer(search([{ path: "a.ink", source }], "the"), {
      "a.ink": source,
    });
    const view = viewOf(buffer);
    // Put caret near the end of the buffer.
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    // A new (shorter) result replaces it.
    buffer.setResult(search([{ path: "a.ink", source: "the end\n" }], "the"));
    const head = view.state.selection.main.head;
    expect(head).toBeGreaterThan(0); // not yanked to a read-only header
    expect(head).toBeLessThanOrEqual(view.state.doc.length); // clamped in-bounds
    buffer.destroy();
  });

  it("debounces: no source write until the idle window elapses, then one write", () => {
    vi.useFakeTimers();
    try {
      const { buffer, onSourceEdit } = makeBuffer(
        search([{ path: "a.ink", source }], "figure"),
        { "a.ink": source },
        DEFAULT_COMMIT_DELAY_MS,
      );
      const view = viewOf(buffer);
      const docText = view.state.doc.toString();
      const at = docText.indexOf("figure");
      // Type "shadow" one char at a time (six separate transactions).
      view.dispatch({ changes: { from: at, to: at + "figure".length, insert: "s" } });
      view.dispatch({ changes: { from: at + 1, to: at + 1, insert: "h" } });
      view.dispatch({ changes: { from: at + 2, to: at + 2, insert: "a" } });
      view.dispatch({ changes: { from: at + 3, to: at + 3, insert: "d" } });
      view.dispatch({ changes: { from: at + 4, to: at + 4, insert: "o" } });
      view.dispatch({ changes: { from: at + 5, to: at + 5, insert: "w" } });
      // Nothing committed yet — no compile-per-keystroke.
      expect(onSourceEdit).not.toHaveBeenCalled();
      vi.advanceTimersByTime(DEFAULT_COMMIT_DELAY_MS);
      // Exactly one coherent write with the whole replacement.
      expect(onSourceEdit).toHaveBeenCalledTimes(1);
      expect(onSourceEdit.mock.calls[0][1].text).toBe(
        "A shadow steps into the light.",
      );
      buffer.destroy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("flushes a pending edit on destroy() (no lost write)", () => {
    vi.useFakeTimers();
    try {
      const { buffer, onSourceEdit } = makeBuffer(
        search([{ path: "a.ink", source }], "figure"),
        { "a.ink": source },
        DEFAULT_COMMIT_DELAY_MS,
      );
      const view = viewOf(buffer);
      const at = view.state.doc.toString().indexOf("figure");
      view.dispatch({ changes: { from: at, to: at + "figure".length, insert: "shadow" } });
      expect(onSourceEdit).not.toHaveBeenCalled(); // still pending
      buffer.destroy(); // teardown must flush
      expect(onSourceEdit).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  // ── Issue 4: keyboard reveal ────────────────────────────────────────

  it("reveals a match on Enter from the match line (keyboard-reachable)", () => {
    const { buffer, onReveal } = makeBuffer(
      search([{ path: "a.ink", source }], "figure"),
      { "a.ink": source },
    );
    const view = viewOf(buffer);
    // Caret on the match line (line 2).
    const lineStart = view.state.doc.line(2).from;
    view.dispatch({ selection: { anchor: lineStart + 5 } });
    // Simulate the Enter keybinding firing, as CM6 does from a keydown event.
    const handled = runKey(view, "Enter");
    expect(handled).toBe(true);
    expect(onReveal).toHaveBeenCalledTimes(1);
    expect(onReveal.mock.calls[0][0]).toBe("a.ink");
    buffer.destroy();
  });

  it("does not reveal on Enter from a header line (falls through)", () => {
    const { buffer, onReveal } = makeBuffer(
      search([{ path: "a.ink", source }], "figure"),
      { "a.ink": source },
    );
    const view = viewOf(buffer);
    view.dispatch({ selection: { anchor: 0 } }); // header line
    const handled = runKey(view, "Enter");
    expect(handled).toBe(false);
    expect(onReveal).not.toHaveBeenCalled();
    buffer.destroy();
  });
});
