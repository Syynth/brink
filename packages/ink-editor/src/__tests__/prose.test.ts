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
import { proseExtension, proseRangesOf, withoutCueLines, type ProseLint } from "../prose.js";
import { elementTypeField } from "../element-type.js";
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

/**
 * Cue lines are not prose.
 *
 * A cue is the speaker's NAME — the same category as the knot and stitch
 * names prose checking has always excluded — but to the HIR projection an
 * ink cue line is an ordinary content span, so it arrives here looking like
 * prose.
 *
 * This is one half of a two-half fix and fails on its own: the dictionary
 * seeds a cue name in TITLE case (`Griswold`), because that is what the
 * prose uses and matching is literal, and Harper's proper-noun metadata then
 * reports the all-caps cue line itself. Excluding the line is what makes the
 * title-case seed safe.
 */
describe("withoutCueLines", () => {
  /** A state with the dialect classification the editor really uses. */
  function stateOf(text: string) {
    return EditorState.create({ doc: text, extensions: [elementTypeField] });
  }

  const SCRIPT = "@GRISWOLD:<>\nBuying or dying?\n";

  it("drops the cue line, keeping the dialogue under it", () => {
    const state = stateOf(SCRIPT);
    const whole = [{ from: 0, to: state.doc.length }];
    const kept = withoutCueLines(whole, state);
    expect(textOf(state.doc, kept).join("").includes("GRISWOLD")).toBe(false);
    expect(textOf(state.doc, kept).join("")).toContain("Buying or dying?");
  });

  it("leaves a document with no cues untouched", () => {
    const state = stateOf("Just narrative here.\n");
    const whole = [{ from: 0, to: state.doc.length }];
    expect(withoutCueLines(whole, state)).toEqual(whole);
  });

  it("keeps parentheticals and dialogue, which ARE prose", () => {
    // Only the name is excluded. A parenthetical is written prose and an
    // author wants its typos found.
    const state = stateOf("@GRISWOLD:<>\n(quietly)<>\nBuying or dying?\n");
    const kept = textOf(state.doc, withoutCueLines([{ from: 0, to: state.doc.length }], state));
    expect(kept.join("")).toContain("quietly");
    expect(kept.join("")).toContain("Buying or dying?");
  });

  it("is a no-op when the field is absent, rather than throwing", () => {
    // A headless composition may not install the classifier at all; prose
    // checking should degrade to "checks a bit too much", never to an
    // exception on the debounce path.
    const bare = EditorState.create({ doc: SCRIPT });
    const whole = [{ from: 0, to: bare.doc.length }];
    expect(withoutCueLines(whole, bare)).toEqual(whole);
  });
});

/**
 * Reporting findings out to the host (#3256).
 *
 * The squiggles go into CodeMirror; the Problems panel needs the same
 * findings as data. Reported from the same guarded point as the squiggles
 * so the two can never disagree — a host list holding rows the editor has
 * already cleared is the failure that placement rules out.
 */
describe("onLints", () => {
  const lint = (start: number, end: number): ProseLint => ({
    start,
    end,
    kind: "Spelling",
    message: "Did you mean to spell `Griswold` this way?",
    suggestions: [],
  });

  /** A projection whose whole doc is one content span. */
  const wholeDocProjection = (length: number) => ({
    spans: [
      {
        kind: "content",
        container: false,
        start_line: 0,
        start_char: 0,
        end_line: 0,
        end_char: length,
      } as HirSpan,
    ],
    lines: [],
  }) as unknown as HirProjection;

  function mount(checker: unknown, onLints: (l: readonly ProseLint[]) => void) {
    const doc = "Griswold waits.";
    return new EditorView({
      state: EditorState.create({
        doc,
        extensions: [
          proseExtension({
            getChecker: () => checker as never,
            getHirProjection: () => wholeDocProjection(doc.length),
            onLints,
            debounceMs: 0,
          }),
        ],
      }),
    });
  }

  const settle = () => new Promise((r) => setTimeout(r, 20));

  it("labels a prose lint with the checker's rule name, not its severity", async () => {
    // `spelling` says more than `info` would, and it is the same slot the
    // compiler fills with `warning` — one anatomy, two producers.
    const view = mount({ check: async () => [lint(0, 8)] }, () => {});
    await settle();
    // `renderMessage` is typed as returning a bare `Node`; the anatomy
    // always builds an element. Collected rather than assigned inside the
    // callback, which narrows to `never` after the loop.
    const rendered: (Node | undefined)[] = [];
    forEachDiagnostic(view.state, (d) => {
      rendered.push(d.renderMessage?.(view));
    });
    const first = rendered[0];
    const dom = first instanceof HTMLElement ? first : null;
    expect(dom?.querySelector(".cm-diag-label")?.textContent).toBe("spelling");
    expect(dom?.querySelector(".cm-diag-title")?.textContent).toContain("Griswold");
    view.destroy();
  });

  it("reports the findings of a check", async () => {
    const seen: (readonly ProseLint[])[] = [];
    const view = mount({ check: async () => [lint(0, 8)] }, (l) => seen.push(l));
    await settle();
    expect(seen.at(-1)).toHaveLength(1);
    expect(seen.at(-1)?.[0]?.message).toContain("Griswold");
    view.destroy();
  });

  it("reports an empty set when checking is switched off", async () => {
    // `[prose] enable = false` unregisters the checker. A host list that
    // kept its rows would show findings the editor no longer shows — the
    // setting would look broken from the panel.
    const seen: (readonly ProseLint[])[] = [];
    const view = mount(null, (l) => seen.push(l));
    await settle();
    expect(seen.at(-1)).toEqual([]);
    view.destroy();
  });

  it("reports an empty set when the check finds nothing", async () => {
    const seen: (readonly ProseLint[])[] = [];
    const view = mount({ check: async () => [] }, (l) => seen.push(l));
    await settle();
    expect(seen.at(-1)).toEqual([]);
    view.destroy();
  });
});
