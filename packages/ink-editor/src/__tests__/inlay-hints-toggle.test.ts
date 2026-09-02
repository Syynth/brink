/**
 * Inlay hints on/off toggle (#3350, Settings ▸ Editor "Show inlay hints").
 *
 * `setInlayHints` is a StateField + effect, the same shape `argument-widgets.ts`
 * uses for `formGlyph`/`autoOpen` (not a compartment): the extension stays
 * mounted either way, and a view whose baseline predates the field simply
 * ignores the effect. Default ON, matching the issue's ruled default.
 */
import { afterEach, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { InlayHint } from "@brink/wasm-types";
import { inlayHintsExtension, setInlayHints } from "../inlay-hints.js";

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
});

const HINT: InlayHint = { offset: 5, label: ": string", kind: "type", padding_right: false };

function mount(hints: InlayHint[]): EditorView {
  view = new EditorView({
    state: EditorState.create({
      doc: "hello world",
      extensions: [inlayHintsExtension({ getInlayHints: () => hints })],
    }),
    parent: document.body,
  });
  return view;
}

/** Widget decorations render as `.brink-inlay-hint` spans in the DOM. */
function hintCount(v: EditorView): number {
  return v.contentDOM.querySelectorAll(".brink-inlay-hint").length;
}

describe("inlay hints on/off toggle", () => {
  it("renders hints by default (ON)", () => {
    const v = mount([HINT]);
    expect(hintCount(v)).toBe(1);
  });

  it("setInlayHints(false) hides them live, without touching the document", () => {
    const v = mount([HINT]);
    expect(hintCount(v)).toBe(1);
    setInlayHints(v, false);
    expect(hintCount(v)).toBe(0);
    expect(v.state.doc.toString()).toBe("hello world");
  });

  it("setInlayHints(true) restores them after being hidden", () => {
    const v = mount([HINT]);
    setInlayHints(v, false);
    expect(hintCount(v)).toBe(0);
    setInlayHints(v, true);
    expect(hintCount(v)).toBe(1);
  });

  it("a doc edit while hidden does not resurrect hints", () => {
    const v = mount([HINT]);
    setInlayHints(v, false);
    v.dispatch({ changes: { from: 0, insert: "x" } });
    expect(hintCount(v)).toBe(0);
  });

  it("is a no-op on a view with no inlay-hints extension mounted", () => {
    view = new EditorView({
      state: EditorState.create({ doc: "hello" }),
      parent: document.body,
    });
    expect(() => setInlayHints(view!, false)).not.toThrow();
  });
});
