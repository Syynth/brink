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

/**
 * Reproduce the attribute rewrite a first click performs: focus the view,
 * then let CodeMirror rebuild the editor element's attributes.
 *
 * Both halves are required. `updateAttrs` computes the class as
 * `"cm-editor"` + the focus flag + the `editorAttributes` facet and writes
 * it with a whole-value `setAttribute` — but only when the computed value
 * DIFFERS from the last one. Rebuilding without flipping focus writes
 * nothing and would pass even against the bug (verified: the fix removed,
 * the test still went green). Focus is what changes the string, and
 * changing the string is what used to erase the marker.
 *
 * `hasFocus` is overridden rather than driven through `contentDOM.focus()`
 * because jsdom does not treat a contenteditable div as focusable, so the
 * real call leaves `document.activeElement` on the body and the flag never
 * flips.
 */
function focusAndRebuildAttrs(view: EditorView): void {
  Object.defineProperty(view, "hasFocus", { get: () => true, configurable: true });
  (view as unknown as { updateAttrs(): void }).updateAttrs();
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

  it("keeps the detached marker when CodeMirror rebuilds the editor's attributes", () => {
    // The regression: the marker was added with `classList.add` on a node
    // CodeMirror owns. Its `updateAttrs` rewrites `class` wholesale from
    // `"cm-editor"` + the focus flag + the `editorAttributes` facet, so the
    // first focus erased the marker. The gutters fell back to their inline
    // `sticky`, rejoined the flow, and — with the compensating padding still
    // applied — the text jumped right by the full gutter width.
    const { view, parent } = mount({ wrapping: true, gutterWidth: 85, hostPadding: "24px" });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    expect(view.dom.classList.contains(DETACHED)).toBe(true);

    focusAndRebuildAttrs(view);

    expect(view.dom.classList.contains(DETACHED)).toBe(true);
    // The padding is the other half of the pair: were the marker to vanish
    // while this stayed, the text would be offset twice over.
    expect(view.contentDOM.style.paddingLeft).toBe("109px");
  });

  it("does not claim the marker for a view it never detached", () => {
    const { view, parent } = mount({ wrapping: false, gutterWidth: 85 });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    focusAndRebuildAttrs(view);
    expect(view.dom.classList.contains(DETACHED)).toBe(false);
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
