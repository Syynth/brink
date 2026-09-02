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
 * - the host's padding is recovered by subtracting the compensation the
 *   content is currently carrying, so it is added to rather than
 *   accumulated on;
 * - every pass recomputes that from the DOM and the gutter's actual
 *   measured width, so a compensation that goes missing is re-established
 *   on the next layout instead of persisting until reload (#3352);
 * - teardown leaves the view exactly as it found it.
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { detachedGutters } from "../gutter-layout.js";

const DETACHED = "brink-detached-gutters";

/** jsdom has no layout, so the gutter's width is stubbed per view — the
 *  plugin's only geometry input. */
function mount(options: {
  wrapping: boolean;
  gutterWidth: number;
  /** Applied INLINE, so a wholesale inline-style rewrite takes it too. */
  hostPadding?: string;
  /** Applied via a STYLESHEET, so it survives such a rewrite — which is
   *  how a real host (`.brink-studio .editor .cm-editor .cm-content`) sets
   *  it, and what makes the #3352 recovery assertions meaningful. */
  hostPaddingRule?: string;
  /** Reconfigurable slot, so a test can drive a real compartment
   *  reconfigure through the view. */
  extra?: Compartment;
}) {
  const parent = document.createElement("div");
  parent.className = "gutter-layout-host";
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({
      doc: "hello\nworld\n",
      extensions: [
        ...(options.wrapping ? [EditorView.lineWrapping] : []),
        detachedGutters(),
        ...(options.extra === undefined ? [] : [options.extra.of([])]),
      ],
    }),
    parent,
  });
  if (options.hostPadding !== undefined) {
    view.contentDOM.style.paddingLeft = options.hostPadding;
  }
  // Appended AFTER the view: CodeMirror's base theme sets
  // `.cm-content { padding: 4px 0 }` from a sheet it injects when the view
  // mounts, and jsdom's `getComputedStyle` applies matching rules in sheet
  // order rather than by specificity — so a host rule has to come last to
  // win, however many classes deep it is.
  let sheet: HTMLStyleElement | null = null;
  if (options.hostPaddingRule !== undefined) {
    sheet = document.createElement("style");
    sheet.textContent = `.gutter-layout-host .cm-editor .cm-content { padding-left: ${options.hostPaddingRule}; }`;
    document.head.appendChild(sheet);
  }
  const gutters = document.createElement("div");
  gutters.className = "cm-gutters";
  gutters.getBoundingClientRect = () =>
    ({ width: options.gutterWidth, height: 0, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0 }) as DOMRect;
  view.dom.appendChild(gutters);
  return { view, parent, gutters, sheet };
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

  /**
   * #3352 — the field report: after a long session the text sat one gutter
   * width to the LEFT, under the floating gutter overlay, and horizontal
   * scrolling could not bring it back (nothing was overflowing; it was
   * occlusion). Only a reload recovered.
   *
   * The trigger reproduced here is CodeMirror's own, not a poke at the DOM:
   * `EditorView.updateAttrs` recomputes the content's attribute-derived
   * `style` string on every non-empty update and hands it to the
   * `updateAttrs` helper, which applies a style attribute with a
   * WHOLE-VALUE `dom.style.cssText = attrs.style` — erasing every inline
   * declaration the plugin put there. A `tabSize` reconfigure is the
   * smallest real change to that string, and the plugin instance survives
   * it. Verified against the installed @codemirror/view 6.43.9: before the
   * reconfigure the content's style attribute reads
   * `"tab-size: 4; padding-left: 109px; …"`, after it reads `"tab-size: 8;"`.
   *
   * Against the old accumulator this was unrecoverable: it believed 85px
   * was still applied, so the host padding it recovered was
   * `max(0, 24 - 85) = 0` — and since the gutter width had not changed,
   * `width !== applied` was false and it wrote NOTHING AT ALL. The content
   * stayed at the host's bare 24px for the rest of the session.
   */
  it("re-establishes the compensation a compartment reconfigure wiped off the content", () => {
    const tabSize = new Compartment();
    const { view, parent, sheet } = mount({
      wrapping: true,
      gutterWidth: 85,
      hostPaddingRule: "24px",
      extra: tabSize,
    });
    cleanups.push(() => { view.destroy(); parent.remove(); sheet?.remove(); });
    flush(view);
    expect(view.contentDOM.style.paddingLeft).toBe("109px");

    view.dispatch({ effects: tabSize.reconfigure(EditorState.tabSize.of(8)) });
    // CodeMirror really did take the whole inline declaration with it.
    expect(view.contentDOM.style.paddingLeft).toBe("");
    flush(view);

    // Healed in the same measure pass, back to host 24 + gutter 85 — the
    // text is where it always was, not under the gutters.
    expect(view.contentDOM.style.paddingLeft).toBe("109px");
    expect(view.dom.classList.contains(DETACHED)).toBe(true);
  });

  it("recomputes from the gutter's actual width when it changed while the compensation was gone", () => {
    const tabSize = new Compartment();
    const { view, parent, gutters, sheet } = mount({
      wrapping: true,
      gutterWidth: 85,
      hostPaddingRule: "24px",
      extra: tabSize,
    });
    cleanups.push(() => { view.destroy(); parent.remove(); sheet?.remove(); });
    flush(view);
    expect(view.contentDOM.style.paddingLeft).toBe("109px");

    view.dispatch({ effects: tabSize.reconfigure(EditorState.tabSize.of(8)) });
    // ...and the line-number column grew a digit while the padding was off.
    gutters.getBoundingClientRect = () => ({ width: 93 }) as DOMRect;
    flush(view);

    // 24 + 93. Not 109 + 93, and not the 85 the plugin last wrote.
    expect(view.contentDOM.style.paddingLeft).toBe("117px");
  });

  /**
   * The other shape of the same drift: the padding is overwritten with the
   * host's bare value while the record of what was compensating it stays.
   * A padding smaller than the compensation it supposedly contains is a
   * contradiction with one honest reading — none of it is the plugin's —
   * so the whole of it counts as the host's base and is compensated afresh.
   */
  it("re-applies the compensation when the padding alone was overwritten", () => {
    const { view, parent } = mount({ wrapping: true, gutterWidth: 85, hostPadding: "24px" });
    cleanups.push(() => { view.destroy(); parent.remove(); });
    flush(view);
    expect(view.contentDOM.style.paddingLeft).toBe("109px");

    view.contentDOM.style.paddingLeft = "24px";
    view.dispatch({ changes: { from: 0, insert: "x" } });
    flush(view);

    // The gutter width never changed, which is exactly the case the old
    // `width !== applied` guard refused to write for.
    expect(view.contentDOM.style.paddingLeft).toBe("109px");
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
    // The steady state writes nothing: the DOM already says exactly what
    // this pass would write, record included.
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });
});
