/**
 * The execution highlight extension (W6/#3299). jsdom-safe pins:
 *
 * - bands render per kind, PLURAL (a choice point lights several lines);
 * - the extension re-reads ONLY on `refreshExecutionHighlight` (no
 *   polling);
 * - an edit maps the band along instead of dropping it;
 * - the paused arrow takes the play gutter's column and outranks a
 *   breakpoint dot on the same line.
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
import { playFromHereExtension } from "../play-from-here.js";

let views: EditorView[] = [];
afterEach(() => {
  for (const v of views) v.destroy();
  views = [];
  document.body.innerHTML = "";
});

const DOC = "one\ntwo\nthree\nfour\n";

function mount(get: () => readonly ExecutionHighlight[], withGutter = false) {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        elementTypeField,
        executionHighlightExtension({ getExecutionHighlights: get }),
        ...(withGutter
          ? [
              playFromHereExtension({
                onPlayFrom: () => {},
                getBreakpoints: () => [{ line: 2, state: "bound" }],
                onToggleBreakpoint: () => {},
                getExecutionHighlights: get,
              }),
            ]
          : []),
      ],
    }),
    parent,
  });
  views.push(view);
  return view;
}

const bandEls = (view: EditorView) =>
  Array.from(view.dom.querySelectorAll<HTMLElement>(".brink-exec-line")).map(
    (el) => el.className,
  );

describe("execution highlight (W6/#3299)", () => {
  it("renders plural bands with the kind taxonomy's classes", () => {
    const view = mount(() => [
      { line: 1, kind: "paused" },
      { line: 3, kind: "live" },
      { line: 4, kind: "frame" },
    ]);
    const classes = bandEls(view).join(" | ");
    expect(classes).toContain("brink-exec-paused");
    expect(classes).toContain("brink-exec-live");
    expect(classes).toContain("brink-exec-frame");
    expect(bandEls(view)).toHaveLength(3);
  });

  it("re-reads only on refresh — no polling", () => {
    let highlights: ExecutionHighlight[] = [];
    const view = mount(() => highlights);
    expect(bandEls(view)).toHaveLength(0);

    highlights = [{ line: 2, kind: "live" }];
    expect(bandEls(view)).toHaveLength(0);

    refreshExecutionHighlight(view);
    expect(bandEls(view)).toHaveLength(1);

    highlights = [];
    refreshExecutionHighlight(view);
    expect(bandEls(view)).toHaveLength(0);
  });

  it("maps the band through an edit instead of dropping it", () => {
    const view = mount(() => [{ line: 3, kind: "paused" }]);
    expect(bandEls(view)).toHaveLength(1);
    // Insert a line at the top: the decoration maps below the insertion
    // (the host's own refresh lands right behind with the re-derived
    // position; the mapped band keeps the frame visually stable).
    view.dispatch({ changes: { from: 0, to: 0, insert: "zero\n" } });
    expect(bandEls(view)).toHaveLength(1);
  });

  it("the paused arrow takes the shared gutter column and outranks the dot", () => {
    const view = mount(() => [{ line: 2, kind: "paused" }], true);
    refreshExecutionHighlight(view);
    // Line 2 carries BOTH a bound breakpoint and the paused position: the
    // arrow wins the glyph (the band still marks the line; the dot's
    // information is carried by the anchor list).
    const arrows = view.dom.querySelectorAll(".brink-exec-arrow-paused");
    expect(arrows).toHaveLength(1);
    const dots = view.dom.querySelectorAll(".brink-breakpoint-dot");
    expect(dots).toHaveLength(0);
  });

  it("a frame highlight draws the hollow accent arrow", () => {
    const view = mount(() => [{ line: 3, kind: "frame" }], true);
    refreshExecutionHighlight(view);
    expect(view.dom.querySelectorAll(".brink-exec-arrow-frame")).toHaveLength(1);
  });

  it("a rejected choice dims with its note; condition notes enrich from the line (W11)", () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: "* [Go]\n* {gold > 20} [Pricey]\n* [Other]\n",
        extensions: [
          elementTypeField,
          executionHighlightExtension({
            getExecutionHighlights: () => [
              { line: 1, kind: "live" },
              { line: 2, kind: "rejected", note: "condition false" },
              { line: 3, kind: "rejected", note: "once-only · used" },
            ],
          }),
        ],
      }),
      parent: document.body,
    });
    views.push(view);
    const rejected = Array.from(
      view.dom.querySelectorAll<HTMLElement>(".brink-exec-rejected"),
    );
    expect(rejected).toHaveLength(2);
    // The by-elimination condition case enriches from the line's own {…}.
    expect(rejected[0].getAttribute("data-brink-exec-note")).toBe("gold > 20 = false");
    // Other notes pass through verbatim.
    expect(rejected[1].getAttribute("data-brink-exec-note")).toBe("once-only · used");
  });

  it("a live highlight draws NO gutter glyph — the arrow belongs to pause", () => {
    const view = mount(() => [{ line: 3, kind: "live" }], true);
    refreshExecutionHighlight(view);
    expect(view.dom.querySelectorAll(".brink-exec-arrow").length).toBe(0);
  });
});
