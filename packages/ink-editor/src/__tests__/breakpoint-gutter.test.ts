/**
 * Breakpoint dots in the play gutter (W4/#3297 — RULED 2026-08-29: one
 * shared column). What this suite pins, all jsdom-safe:
 *
 * - dots render from the host's markers with the right state class
 *   (bound solid / unbound hollow / disabled dimmed);
 * - the gutter re-reads ONLY on `refreshBreakpoints` — host data changing
 *   without the effect changes nothing (no polling);
 * - a doc edit above a dot reports the old→new line pair through
 *   `onBreakpointsMoved`, in a microtask, mapped through the change set;
 * - a gutter click on a non-header line toggles; on a header line it stays
 *   play-from-here (the ruled conflict rule).
 *
 * Hover behavior (the preview dot, ▶ keeping the glyph on hovered headers)
 * needs real pointer geometry — that lives with the Playwright/e2e layer,
 * not jsdom.
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { elementTypeField } from "../element-type.js";
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

const DOC = "=== tavern ===\nThe tavern is loud tonight.\n~ temp x = 0\nhello\n-> END\n";

function mount(options: {
  breakpoints?: () => readonly BreakpointGutterMarker[];
  onToggle?: (line: number) => void;
  onMoved?: (moves: readonly { from: number; to: number }[]) => void;
  onPlayFrom?: (path: string) => void;
}) {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        elementTypeField,
        playFromHereExtension({
          onPlayFrom: (p) => options.onPlayFrom?.(p),
          getBreakpoints: options.breakpoints ?? (() => []),
          onToggleBreakpoint: options.onToggle ?? (() => {}),
          onBreakpointsMoved: options.onMoved,
        }),
      ],
    }),
    parent,
  });
  views.push(view);
  return view;
}

function dotEls(view: EditorView): HTMLElement[] {
  return Array.from(view.dom.querySelectorAll<HTMLElement>(".brink-breakpoint-dot"));
}

describe("breakpoint gutter (W4/#3297)", () => {
  it("renders the host's dots with the state taxonomy's classes", () => {
    const view = mount({
      breakpoints: () => [
        { line: 2, state: "bound" },
        { line: 3, state: "unbound" },
        { line: 4, state: "disabled" },
      ],
    });
    const classes = dotEls(view).map((el) => el.className);
    expect(classes).toEqual([
      "brink-breakpoint-dot brink-breakpoint-bound",
      "brink-breakpoint-dot brink-breakpoint-unbound",
      "brink-breakpoint-dot brink-breakpoint-disabled",
    ]);
  });

  it("re-reads only on refreshBreakpoints — no polling", () => {
    let markers: BreakpointGutterMarker[] = [];
    const view = mount({ breakpoints: () => markers });
    expect(dotEls(view)).toHaveLength(0);

    // Host data changed, no effect dispatched: the gutter must NOT notice.
    markers = [{ line: 2, state: "bound" }];
    expect(dotEls(view)).toHaveLength(0);

    refreshBreakpoints(view);
    expect(dotEls(view)).toHaveLength(1);

    markers = [];
    refreshBreakpoints(view);
    expect(dotEls(view)).toHaveLength(0);
  });

  it("reports old→new line pairs when an edit above a dot moves it", async () => {
    const onMoved = vi.fn();
    const view = mount({
      breakpoints: () => [{ line: 3, state: "bound" }],
      onMoved,
    });

    // Insert a line at the top: line 3's start maps below the insertion.
    view.dispatch({ changes: { from: 0, to: 0, insert: "// header\n" } });
    // Delivery is a microtask by contract (never inside the update cycle).
    expect(onMoved).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(onMoved).toHaveBeenCalledWith([{ from: 3, to: 4 }]);
  });

  it("does not report moves for edits below the dot", async () => {
    const onMoved = vi.fn();
    const view = mount({
      breakpoints: () => [{ line: 2, state: "bound" }],
      onMoved,
    });
    const end = view.state.doc.length;
    view.dispatch({ changes: { from: end, to: end, insert: "tail\n" } });
    await Promise.resolve();
    expect(onMoved).not.toHaveBeenCalled();
  });

  it("gutter click toggles on a plain line and plays on a header line", () => {
    const onToggle = vi.fn();
    const onPlayFrom = vi.fn();
    // Dots on the header AND a plain line: jsdom (no layout) only
    // materializes gutter elements that carry markers, and a dot on the
    // header also pins the ruled priority — a header click is play, even
    // with a breakpoint rendered there.
    const view = mount({
      breakpoints: () => [
        { line: 1, state: "bound" },
        { line: 2, state: "bound" },
      ],
      onToggle,
      onPlayFrom,
    });

    const dots = dotEls(view);
    expect(dots).toHaveLength(2);
    const mousedown = (el: HTMLElement): void => {
      // Bubbles up to the gutter element, where CM's `domEventHandlers`
      // resolves the line — the real dispatch path. jsdom has no layout,
      // so every click resolves to line 1 (y=0); each branch is therefore
      // pinned with its own document whose line 1 IS the case under test.
      el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    };

    mousedown(dots[0] as HTMLElement); // line 1 — the `=== tavern ===` header
    expect(onPlayFrom).toHaveBeenCalledWith("tavern");
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("gutter click on a plain line toggles a breakpoint there", () => {
    const onToggle = vi.fn();
    const onPlayFrom = vi.fn();
    const parent = document.createElement("div");
    document.body.appendChild(parent);
    const view = new EditorView({
      state: EditorState.create({
        // Line 1 is plain prose — the toggle branch (see the note above on
        // jsdom resolving every gutter click to line 1).
        doc: "The tavern is loud tonight.\n=== tavern ===\nhello\n",
        extensions: [
          elementTypeField,
          playFromHereExtension({
            onPlayFrom,
            getBreakpoints: () => [{ line: 1, state: "unbound" }],
            onToggleBreakpoint: onToggle,
          }),
        ],
      }),
      parent,
    });
    views.push(view);

    const dot = dotEls(view)[0];
    expect(dot).toBeDefined();
    dot?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    expect(onToggle).toHaveBeenCalledWith(1);
    expect(onPlayFrom).not.toHaveBeenCalled();
  });
});
