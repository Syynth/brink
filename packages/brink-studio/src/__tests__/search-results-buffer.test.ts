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
  SEARCH_RESULT_CAP,
  type ProjectSearchResult,
  type ResultRow,
  type SearchQueryOptions,
} from "@brink/studio-store";

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
) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const onSourceEdit = vi.fn();
  const onReveal = vi.fn();
  const buffer = new SearchResultsBuffer(host, result, {
    getSource: (path) => sources[path] ?? null,
    onSourceEdit,
    onReveal,
  });
  return { host, buffer, onSourceEdit, onReveal };
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
    const view = (buffer as unknown as { view: import("@codemirror/view").EditorView }).view;
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
    const view = (buffer as unknown as { view: import("@codemirror/view").EditorView }).view;
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
    const view = (buffer as unknown as { view: import("@codemirror/view").EditorView }).view;
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
});
