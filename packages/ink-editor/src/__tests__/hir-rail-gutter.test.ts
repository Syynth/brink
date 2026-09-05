/**
 * Structural rails — position, single hover, no per-bar handlers (#3501,
 * ruled 2026-09-03 during the maintainer's drive-it, decision-log
 * "Structural rails: rightmost gutter, one compact hover for the whole
 * stack").
 *
 * Three things this suite pins:
 *
 * 1. **Position**: the rails gutter (`brink-hir-rail-gutter`) is the
 *    RIGHTMOST `.cm-gutter` — after line numbers and the play/breakpoint and
 *    host gutters, directly adjacent to `.cm-content`. Achieved via CM's own
 *    gutter precedence (`Prec.lowest`), not CSS reordering, so this has to
 *    hold for the real DOM order CodeMirror builds, with every other gutter
 *    mounted alongside it — a single-gutter test would not catch a
 *    precedence regression.
 * 2. **One hover for the whole stack**: hovering the rails column at a line
 *    shows ONE tooltip listing every container in that line's stack,
 *    outermost first — not a tooltip per bar.
 * 3. **No per-bar handlers remain**: a bar element itself carries no
 *    listener of its own; only the wrapping element the gutter mounts does.
 */

import { describe, expect, it, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import type { HirProjection } from "@brink/wasm-types";
import { elementTypeField } from "../element-type.js";
import { playFromHereExtension } from "../play-from-here.js";
import { hostGutterExtension } from "../host-gutter.js";
import { hirOverlayExtension } from "../hir-overlay.js";

const DOC = "=== start ===\n= inner\nChoice line\n-> done\n";

/** A 3-deep stack on line 3 (`Choice line`): knot -> stitch -> choice,
 *  outermost first — matching the wire contract
 *  (`crates/brink-web/src/editor_dto.rs`'s `HirLineContainerJs` doc: "One
 *  entry of a line's container stack (outermost→innermost by depth)"). */
const PROJECTION: HirProjection = {
  spans: [
    { start_line: 0, start_char: 0, end_line: 3, end_char: 7, kind: "knot", container: true, depth: 0, handle: 1 },
    { start_line: 1, start_char: 0, end_line: 3, end_char: 7, kind: "stitch", container: true, depth: 1, handle: 2 },
    { start_line: 2, start_char: 0, end_line: 2, end_char: 11, kind: "choice", container: true, depth: 2, handle: 3 },
  ],
  lines: [
    [{ kind: "knot", handle: 1, depth: 0 }],
    [{ kind: "knot", handle: 1, depth: 0 }, { kind: "stitch", handle: 2, depth: 1 }],
    [
      { kind: "knot", handle: 1, depth: 0 },
      { kind: "stitch", handle: 2, depth: 1 },
      { kind: "choice", handle: 3, depth: 2 },
    ],
    [{ kind: "knot", handle: 1, depth: 0 }, { kind: "stitch", handle: 2, depth: 1 }],
  ],
};

let views: EditorView[] = [];
afterEach(() => {
  for (const v of views) v.destroy();
  views = [];
  document.body.innerHTML = "";
});

/** The full, realistic gutter set — mirrors the real composition order
 *  (`extensions.ts`: play-from-here/host gutters inside `ideCompartment`,
 *  after the HIR overlay; `setup.ts`'s `brinkBasicSetup` — line numbers —
 *  composed AFTER the whole `brinkStudio()` bundle by `document-sessions.ts`)
 *  so the rightmost-gutter assertion is meaningful against every other
 *  gutter this editor actually mounts, not a synthetic pair. */
function mount(): EditorView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        elementTypeField,
        playFromHereExtension({ onPlayFrom: () => {} }),
        hostGutterExtension({ getGutterMarkers: () => [] }),
        hirOverlayExtension({ getHirProjection: () => PROJECTION }),
        // Registered LAST in the tree — proving the rightmost position is a
        // real precedence effect, not an accident of declaration order.
        lineNumbers(),
      ],
    }),
    parent,
  });
  views.push(view);
  return view;
}

function gutterEls(view: EditorView): HTMLElement[] {
  return Array.from(view.dom.querySelectorAll<HTMLElement>(".cm-gutters > .cm-gutter"));
}

function railWrapOnLine(view: EditorView, lineNumber: number): HTMLElement {
  const gutter = view.dom.querySelector<HTMLElement>(".brink-hir-rail-gutter");
  if (!gutter) throw new Error("rails gutter not mounted");
  // One `.cm-gutterElement` per visible line, in document order, for this
  // short unwrapped fixture doc.
  const elements = Array.from(gutter.querySelectorAll<HTMLElement>(".cm-gutterElement"));
  const wrap = elements[lineNumber - 1]?.querySelector<HTMLElement>(".brink-hir-rails");
  if (!wrap) {
    throw new Error(`no rails wrap on line ${lineNumber} (found ${elements.length} gutter elements)`);
  }
  return wrap;
}

describe("structural rails gutter order (#3501)", () => {
  it("mounts the rails gutter as the LAST .cm-gutter, after line numbers, play and host gutters", () => {
    const view = mount();
    const gutters = gutterEls(view);
    const classes = gutters.map((g) => g.className);
    expect(gutters.length).toBeGreaterThanOrEqual(4);
    expect(gutters[gutters.length - 1]?.classList.contains("brink-hir-rail-gutter")).toBe(
      true,
    );
    // Every other known gutter precedes it.
    const railIndex = gutters.findIndex((g) => g.classList.contains("brink-hir-rail-gutter"));
    for (let i = 0; i < gutters.length; i++) {
      if (i === railIndex) continue;
      expect(i, `gutter classes: ${classes.join(" | ")}`).toBeLessThan(railIndex);
    }
  });
});

describe("structural rails single hover (#3501)", () => {
  it("lists every container in the line's stack, outermost first, on ONE hover", () => {
    const view = mount();
    const wrap = railWrapOnLine(view, 3);

    expect(document.querySelector(".brink-rail-tooltip")).toBeNull();
    wrap.dispatchEvent(new Event("pointerenter"));

    const tip = document.querySelector(".brink-rail-tooltip");
    expect(tip).not.toBeNull();
    const entries = tip!.querySelectorAll(".brink-rail-tooltip-label");
    expect(entries).toHaveLength(3);
    // Outermost first: knot, stitch, choice.
    expect(entries[0]?.textContent).toContain("start");
    expect(entries[1]?.textContent).toContain("inner");
    expect(entries[2]?.textContent).toContain("Choice line");

    const metas = tip!.querySelectorAll(".brink-rail-tooltip-meta");
    expect(metas).toHaveLength(3);

    wrap.dispatchEvent(new Event("pointerleave"));
    expect(document.querySelector(".brink-rail-tooltip")).toBeNull();
  });

  it("attaches no listener to an individual bar — only the wrapper", () => {
    const view = mount();
    const wrap = railWrapOnLine(view, 3);
    const bar = wrap.querySelector<HTMLElement>(".brink-hir-rail");
    expect(bar).not.toBeNull();

    // Dispatched directly on the bar itself (bypassing any real hit-test
    // geometry jsdom can't do): a per-bar handler would have shown a
    // tooltip here in the old design. It must not.
    bar!.dispatchEvent(new Event("pointerenter"));
    expect(document.querySelector(".brink-rail-tooltip")).toBeNull();

    // The wrapper still works.
    wrap.dispatchEvent(new Event("pointerenter"));
    expect(document.querySelector(".brink-rail-tooltip")).not.toBeNull();
  });
});
