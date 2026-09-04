/**
 * The play gutter's host reads are ONCE PER RENDER, not once per visible
 * line (#3490).
 *
 * `gutter({ lineMarker })` runs its callback for every line in the
 * viewport. Reading a host hook from inside it therefore multiplies the
 * host's work by the number of visible lines — and `getExecutionHighlights`
 * is a whole-document query behind the studio's seam (measured 2026-09-03:
 * ~38 synchronous wasm `getHirSpansDoc` calls per keystroke on a 1,125-line
 * file, with no session running at all).
 *
 * Measured here with the caching reverted, the pre-fix counts scale exactly
 * with the viewport: 4 calls for a 4-line document, 20 for a 20-line one,
 * 36 for anything longer (jsdom's measurement-free viewport tops out
 * there). These pins are the shape of the fix, not its implementation:
 * however the per-line reads are collapsed, the host must be asked at most
 * once per render pass, and a refreshed answer must still reach the glyph.
 *
 * Each count test states its premise first — how many lines CodeMirror had
 * in view — because a one-line viewport would make the count assertion
 * vacuous.
 */
import { describe, expect, it, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { elementTypeField } from "../element-type.js";
import {
  executionHighlightExtension,
  refreshExecutionHighlight,
  type ExecutionHighlight,
} from "../execution-highlight.js";
import {
  playFromHereExtension,
  refreshBreakpoints,
  type BreakpointGutterMarker,
} from "../play-from-here.js";

let views: EditorView[] = [];
afterEach(() => {
  for (const v of views) v.destroy();
  views = [];
  document.body.innerHTML = "";
});

/** Long enough that a per-line read is an order of magnitude off a
 *  per-render one. */
const LINES = 60;
const DOC = Array.from({ length: LINES }, (_, i) => `line ${i + 1} of prose`).join("\n");

interface Counters {
  highlights: number;
  breakpoints: number;
}

function mount(
  highlights: () => readonly ExecutionHighlight[],
  breakpoints: () => readonly BreakpointGutterMarker[] = () => [],
): { view: EditorView; counts: Counters } {
  const counts: Counters = { highlights: 0, breakpoints: 0 };
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        elementTypeField,
        // The gutter watches `executionHighlightVersion`, which this
        // extension owns — without it a refresh never reaches the arrow.
        // Its own read is deliberately uncounted: `counts` is about what
        // the GUTTER asks for.
        executionHighlightExtension({ getExecutionHighlights: () => highlights() }),
        playFromHereExtension({
          onPlayFrom: () => {},
          onToggleBreakpoint: () => {},
          getBreakpoints: () => {
            counts.breakpoints++;
            return breakpoints();
          },
          getExecutionHighlights: () => {
            counts.highlights++;
            return highlights();
          },
        }),
      ],
    }),
    parent,
  });
  views.push(view);
  return { view, counts };
}

/** How many lines the gutter had to visit — `lineMarker` runs once for
 *  each of them, which is exactly the multiplier under test. */
function visibleLines(view: EditorView): number {
  const doc = view.state.doc;
  let n = 0;
  for (const range of view.visibleRanges) {
    n += doc.lineAt(range.to).number - doc.lineAt(range.from).number + 1;
  }
  return n;
}

describe("play gutter host reads (#3490)", () => {
  it("asks the host for execution highlights once per render, not once per line", () => {
    const { view, counts } = mount(() => [{ line: 3, kind: "paused" }]);
    expect(visibleLines(view), "viewport too small to tell the two apart").toBeGreaterThan(
      20,
    );
    expect(counts.highlights).toBeLessThanOrEqual(1);

    const afterMount = counts.highlights;
    refreshExecutionHighlight(view);
    expect(visibleLines(view)).toBeGreaterThan(20);
    expect(counts.highlights - afterMount).toBeLessThanOrEqual(1);
  });

  it("asks the host for breakpoints once per render, not once per line", () => {
    const { view, counts } = mount(
      () => [],
      () => [{ line: 4, state: "bound" }],
    );
    expect(visibleLines(view), "viewport too small to tell the two apart").toBeGreaterThan(
      20,
    );
    expect(counts.breakpoints).toBeLessThanOrEqual(1);

    const afterMount = counts.breakpoints;
    refreshBreakpoints(view);
    expect(counts.breakpoints - afterMount).toBeLessThanOrEqual(1);
  });

  it("still re-reads the host on refresh — no answer outlives its state", () => {
    let current: readonly ExecutionHighlight[] = [];
    const { view, counts } = mount(() => current);
    expect(view.dom.querySelectorAll(".brink-exec-arrow-paused")).toHaveLength(0);

    current = [{ line: 3, kind: "paused" }];
    refreshExecutionHighlight(view);
    expect(view.dom.querySelectorAll(".brink-exec-arrow-paused")).toHaveLength(1);

    current = [];
    refreshExecutionHighlight(view);
    expect(view.dom.querySelectorAll(".brink-exec-arrow-paused")).toHaveLength(0);
    // Three renders, three reads — the cache is per state, not per view.
    expect(counts.highlights).toBeGreaterThanOrEqual(3);
  });

  it("still re-reads breakpoints on refresh", () => {
    let current: readonly BreakpointGutterMarker[] = [];
    const { view } = mount(
      () => [],
      () => current,
    );
    expect(view.dom.querySelectorAll(".brink-breakpoint-dot")).toHaveLength(0);

    current = [{ line: 3, state: "bound" }];
    refreshBreakpoints(view);
    expect(view.dom.querySelectorAll(".brink-breakpoint-dot")).toHaveLength(1);
  });
});
