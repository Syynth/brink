/**
 * The rebindable editor actions (editor-actions.ts).
 *
 * These dispatch REAL keydown events at a real EditorView, because the
 * seam under test is exactly "does the chord reach the runner" — the
 * failure mode that motivated the module was chords baked where nothing
 * could see or change them.
 */
import { afterEach, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  editorActionKeymap,
  editorActionRunners,
  runEditorAction,
  setEditorActionKeys,
} from "../editor-actions.js";

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
});

function mount(ran: string[]): EditorView {
  view = new EditorView({
    state: EditorState.create({
      doc: "hello",
      extensions: [
        editorActionKeymap(),
        // Two runners, so a moved chord's OLD owner is observable.
        editorActionRunners.of({
          id: "editor.renameSymbol",
          run: () => (ran.push("rename"), true),
        }),
        editorActionRunners.of({
          id: "editor.findReferences",
          run: () => (ran.push("references"), true),
        }),
      ],
    }),
    parent: document.body,
  });
  return view;
}

const press = (v: EditorView, init: KeyboardEventInit): boolean =>
  v.contentDOM.dispatchEvent(
    new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }),
  );

describe("editor actions", () => {
  it("fires a runner from its shipped default chord", () => {
    const ran: string[] = [];
    press(mount(ran), { key: "F2" });
    expect(ran).toEqual(["rename"]);
  });

  it("consumes the chord, so nothing beneath double-handles it", () => {
    // preventDefault is what the shell key handler keys off to skip an
    // event the editor already owned.
    const ran: string[] = [];
    const v = mount(ran);
    const event = new KeyboardEvent("keydown", { key: "F2", bubbles: true, cancelable: true });
    v.contentDOM.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  it("rebinds live: the new chord fires, the old one goes dead", () => {
    const ran: string[] = [];
    const v = mount(ran);
    setEditorActionKeys(v, { "editor.renameSymbol": ["Mod-r"] });
    press(v, { key: "F2" });
    expect(ran).toEqual([]);
    // jsdom reports a non-mac platform, so Mod- means Ctrl here.
    press(v, { key: "r", ctrlKey: true });
    expect(ran).toEqual(["rename"]);
    // The untouched action keeps its shipped default.
    // Lowercase deliberately: jsdom events carry no keyCode, so CM6's
    // shifted-key fallback (which maps a real browser's "F" back to the
    // physical "f") cannot run; the direct event.key path must match.
    press(v, { key: "f", altKey: true, shiftKey: true });
    expect(ran).toEqual(["rename", "references"]);
  });

  it("null unbinds an action without touching the others", () => {
    const ran: string[] = [];
    const v = mount(ran);
    setEditorActionKeys(v, { "editor.renameSymbol": null });
    press(v, { key: "F2" });
    expect(ran).toEqual([]);
    // Lowercase deliberately: jsdom events carry no keyCode, so CM6's
    // shifted-key fallback (which maps a real browser's "F" back to the
    // physical "f") cannot run; the direct event.key path must match.
    press(v, { key: "f", altKey: true, shiftKey: true });
    expect(ran).toEqual(["references"]);
  });

  it("runs imperatively for the palette and shell commands", () => {
    const ran: string[] = [];
    const v = mount(ran);
    expect(runEditorAction(v, "editor.renameSymbol")).toBe(true);
    expect(ran).toEqual(["rename"]);
    // An action whose feature is not wired in this view: false, no throw.
    expect(runEditorAction(v, "editor.codeActions")).toBe(false);
  });

});
