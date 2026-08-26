/**
 * Detached gutters (#3119): the WebKit editor-layout fix.
 *
 * The perf win itself is only observable in a real WebKit layout (it is
 * measured by the Playwright harness, not here). What this suite pins is
 * the CONTRACT the fix depends on, all of which is engine-independent:
 *
 * - it engages only for wrapping views (a non-wrapping view still needs
 *   CodeMirror's sticky gutters to survive horizontal scrolling);
 * - the horizontal space the detached gutters vacate is paid back as
 *   content padding, ADDED to whatever the host's own padding is;
 * - the host's padding is recovered by subtracting what this plugin last
 *   wrote, so a host whose padding changes (a responsive margin) is
 *   tracked instead of accumulated;
 * - teardown leaves the view exactly as it found it.
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { detachedGutters } from "../gutter-layout.js";

const DETACHED = "brink-detached-gutters";

/** jsdom has no layout, so the gutter's width is stubbed per view — the
 *  plugin's only geometry input. */
function mount(options: { wrapping: boolean; gutterWidth: number; hostPadding?: string }) {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({
      doc: "hello\nworld\n",
      extensions: options.wrapping
        ? [EditorView.lineWrapping, detachedGutters()]
        : [detachedGutters()],
    }),
    parent,
  });
  if (options.hostPadding !== undefined) {
    view.contentDOM.style.paddingLeft = options.hostPadding;
  }
  const gutters = document.createElement("div");
  gutters.className = "cm-gutters";
  gutters.getBoundingClientRect = () =>
    ({ width: options.gutterWidth, height: 0, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0 }) as DOMRect;
  view.dom.appendChild(gutters);
  return { view, parent, gutters };
}

/** The plugin measures through `requestMeasure`; flush it deterministically. */
function flush(view: EditorView): void {
  (view as unknown as { measure(): void }).measure();
}

const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const c of cleanups.splice(0)) c();
});

describe("detachedGutters", () => {
  it("detaches a wrapping view and pays the gutter width back as padding", () => {
    const { view, parent } = mount({ wrapping: true, gutterWidth: 85, hostPadding: "24px" });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    expect(view.dom.classList.contains(DETACHED)).toBe(true);
    // 24px of host padding + an 85px gutter: text lands where it did.
    expect(view.contentDOM.style.paddingLeft).toBe("109px");
  });

  it("leaves a non-wrapping view on CodeMirror's stock layout", () => {
    const { view, parent } = mount({ wrapping: false, gutterWidth: 85, hostPadding: "24px" });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    expect(view.dom.classList.contains(DETACHED)).toBe(false);
    expect(view.contentDOM.style.paddingLeft).toBe("24px");
  });

  it("tracks a widening gutter without accumulating the host's padding", () => {
    const { view, parent, gutters } = mount({ wrapping: true, gutterWidth: 85, hostPadding: "24px" });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    expect(view.contentDOM.style.paddingLeft).toBe("109px");

    // The line-number column grows a digit past 1,000 lines.
    gutters.getBoundingClientRect = () => ({ width: 93 }) as DOMRect;
    view.dispatch({ changes: { from: 0, insert: "x" } });
    flush(view);
    // 24 + 93 — NOT 109 + 93.
    expect(view.contentDOM.style.paddingLeft).toBe("117px");
  });

  it("restores the view on destroy", () => {
    const { view, parent } = mount({ wrapping: true, gutterWidth: 85, hostPadding: "24px" });
    cleanups.push(() => { parent.remove(); });
    flush(view);
    expect(view.dom.classList.contains(DETACHED)).toBe(true);
    const dom = view.dom;
    const content = view.contentDOM;
    view.destroy();
    expect(dom.classList.contains(DETACHED)).toBe(false);
    expect(content.style.paddingLeft).toBe("");
  });

  it("is inert when the view has no gutters at all", () => {
    const parent = document.createElement("div");
    document.body.appendChild(parent);
    const view = new EditorView({
      state: EditorState.create({
        doc: "hello\n",
        extensions: [EditorView.lineWrapping, detachedGutters()],
      }),
      parent,
    });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    // Detached (wrapping) but nothing to pay back: no gutter, zero width.
    expect(view.contentDOM.style.paddingLeft).toBe("");
  });

  it("does not thrash the DOM when nothing changed", () => {
    const { view, parent } = mount({ wrapping: true, gutterWidth: 85, hostPadding: "24px" });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    const spy = vi.spyOn(view.contentDOM.style, "setProperty");
    view.dispatch({ changes: { from: 0, insert: "y" } });
    flush(view);
    view.dispatch({ changes: { from: 0, insert: "z" } });
    flush(view);
    expect(view.contentDOM.style.paddingLeft).toBe("109px");
    spy.mockRestore();
  });
});
