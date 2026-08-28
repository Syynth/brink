/**
 * Prose checking, editor half (#3209).
 *
 * Two things here have silent failure modes and are therefore what these
 * tests are about:
 *
 * 1. **Span subtraction.** Content spans alone would hand `{gold}` to a spell
 *    checker. The result is not an error — it is a squiggle under a variable
 *    name, which an author reads as "the checker is stupid" and switches off.
 * 2. **Two producers, one set.** `setDiagnostics` replaces. If the compile and
 *    the prose check each called it, whichever landed second would erase the
 *    other, intermittently, with nothing thrown.
 */

import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { forEachDiagnostic } from "@codemirror/lint";
import type { HirProjection, HirSpan } from "@brink/wasm-types";
import { proseRangesOf } from "../prose.js";
import { PROSE_DICTIONARY_FILE } from "../project-session.js";
import { diagnosticSources, publishDiagnostics, diagnosticsFrom } from "../diagnostic-sources.js";

/** A non-container span, positioned by line and column. */
function span(
  kind: string,
  startLine: number,
  startChar: number,
  endLine: number,
  endChar: number,
): HirSpan {
  return {
    start_line: startLine,
    start_char: startChar,
    end_line: endLine,
    end_char: endChar,
    kind,
    container: false,
  } as unknown as HirSpan;
}

function docOf(text: string) {
  return EditorState.create({ doc: text }).doc;
}

function textOf(doc: ReturnType<typeof docOf>, ranges: { from: number; to: number }[]): string[] {
  return ranges.map((r) => doc.sliceString(r.from, r.to));
}

describe("proseRangesOf", () => {
  it("returns content spans and nothing else", () => {
    const text = "-> barter::haggle\nThe square is empty.\n#act1";
    const doc = docOf(text);
    const projection = {
      spans: [
        span("divert_stmt", 0, 0, 0, 17),
        span("content", 1, 0, 1, 20),
        span("tag", 2, 0, 2, 5),
      ],
      lines: [],
    } as unknown as HirProjection;

    expect(textOf(doc, proseRangesOf(projection, doc))).toEqual(["The square is empty."]);
  });

  it("subtracts an interpolation nested inside a content span", () => {
    // The failure this exists to prevent: `gold` is a variable name, and a
    // spell checker handed the whole line reports it as a misspelling.
    const text = "You have {gold} pieces left.";
    const doc = docOf(text);
    const projection = {
      spans: [
        span("content", 0, 0, 0, text.length),
        span("interpolation", 0, 9, 0, 15),
      ],
      lines: [],
    } as unknown as HirProjection;

    expect(textOf(doc, proseRangesOf(projection, doc))).toEqual(["You have ", " pieces left."]);
  });

  it("subtracts several holes and drops the empty remainders", () => {
    const text = "{a} between {b}";
    const doc = docOf(text);
    const projection = {
      spans: [
        span("content", 0, 0, 0, text.length),
        span("interpolation", 0, 0, 0, 3),
        span("interpolation", 0, 12, 0, 15),
      ],
      lines: [],
    } as unknown as HirProjection;

    // Only the middle survives; the two ends of the content span coincide
    // exactly with the holes and must not emit zero-width ranges.
    expect(textOf(doc, proseRangesOf(projection, doc))).toEqual([" between "]);
  });

  it("ignores containers, which cover whole knots rather than prose", () => {
    const text = "The square is empty.";
    const doc = docOf(text);
    const container = { ...span("knot", 0, 0, 0, 20), container: true } as unknown as HirSpan;
    const projection = {
      spans: [container, span("content", 0, 0, 0, 20)],
      lines: [],
    } as unknown as HirProjection;

    expect(textOf(doc, proseRangesOf(projection, doc))).toEqual(["The square is empty."]);
  });

  it("returns nothing for a document with no prose at all", () => {
    const text = "-> knot";
    const doc = docOf(text);
    const projection = {
      spans: [span("divert_stmt", 0, 0, 0, 7)],
      lines: [],
    } as unknown as HirProjection;

    expect(proseRangesOf(projection, doc)).toEqual([]);
  });
});

describe("diagnostic sources", () => {
  const makeView = () =>
    new EditorView({
      state: EditorState.create({ doc: "one two three four", extensions: [diagnosticSources] }),
    });

  const shown = (view: EditorView): string[] => {
    const out: string[] = [];
    forEachDiagnostic(view.state, (d) => out.push(d.message));
    return out;
  };

  it("keeps both producers' diagnostics instead of replacing", () => {
    const view = makeView();
    publishDiagnostics(view, "compile", [
      { from: 0, to: 3, severity: "error", message: "compile" },
    ]);
    publishDiagnostics(view, "prose", [
      { from: 4, to: 7, severity: "info", message: "prose" },
    ]);

    expect(shown(view).sort()).toEqual(["compile", "prose"]);
    view.destroy();
  });

  it("republishing one source leaves the other standing", () => {
    // The regression that motivated the registry: a second compile landing
    // after a prose check used to wipe the prose squiggles.
    const view = makeView();
    publishDiagnostics(view, "prose", [
      { from: 4, to: 7, severity: "info", message: "prose" },
    ]);
    publishDiagnostics(view, "compile", [
      { from: 0, to: 3, severity: "error", message: "compile-1" },
    ]);
    publishDiagnostics(view, "compile", [
      { from: 0, to: 3, severity: "error", message: "compile-2" },
    ]);

    expect(shown(view).sort()).toEqual(["compile-2", "prose"]);
    expect(diagnosticsFrom(view, "prose")).toHaveLength(1);
    view.destroy();
  });

  it("publishing an empty batch clears only that source", () => {
    const view = makeView();
    publishDiagnostics(view, "compile", [
      { from: 0, to: 3, severity: "error", message: "compile" },
    ]);
    publishDiagnostics(view, "prose", [
      { from: 4, to: 7, severity: "info", message: "prose" },
    ]);
    publishDiagnostics(view, "prose", []);

    expect(shown(view)).toEqual(["compile"]);
    view.destroy();
  });

  it("orders the union by position, not by which producer ran first", () => {
    const view = makeView();
    publishDiagnostics(view, "compile", [
      { from: 12, to: 18, severity: "error", message: "late" },
    ]);
    publishDiagnostics(view, "prose", [
      { from: 0, to: 3, severity: "info", message: "early" },
    ]);

    expect(shown(view)).toEqual(["early", "late"]);
    view.destroy();
  });

  it("still publishes when the registry is missing, rather than nothing", () => {
    // Insurance, not a supported path: a producer wired without the registry
    // used to publish an empty union and erase everything.
    const bare = new EditorView({ state: EditorState.create({ doc: "one two" }) });
    publishDiagnostics(bare, "compile", [
      { from: 0, to: 3, severity: "error", message: "compile" },
    ]);
    expect(shown(bare)).toEqual(["compile"]);
    bare.destroy();
  });
});


describe("the author dictionary file", () => {
  it("is a dotfile beside brink.toml, not a manuscript chapter", () => {
    // A visible file would appear in the Binder as though it were part of
    // the story. It is project metadata.
    expect(PROSE_DICTIONARY_FILE.startsWith(".")).toBe(true);
    expect(PROSE_DICTIONARY_FILE).not.toMatch(/\.(ink|brink)$/);
  });
});
