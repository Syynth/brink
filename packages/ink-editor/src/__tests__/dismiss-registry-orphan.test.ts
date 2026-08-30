/**
 * The global "dismiss all transient surfaces on Escape" safety net (#279
 * part b, dismiss-registry.ts) exists specifically for the failure #279
 * named: a surface's own dismiss listener is lost — orphaned by a
 * re-render/error — while the surface itself stays mounted and visible. A
 * test that only opens a menu and presses Escape through its OWN listener
 * proves nothing about that case (its own listener is exactly what's
 * supposed to be missing). This file forces the orphan directly: it opens
 * the code-actions menu for real, then detaches JUST its own capture-phase
 * Escape listener (leaving the registry registration — a separate effect —
 * untouched), and proves Escape still closes it.
 */

import { describe, it, expect, afterEach, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import type { CodeAction } from "@brink/wasm-types";
import { codeActionsExtension } from "../code-actions.js";
// The chord moved out of codeActionsExtension into the shared actions
// keymap (editor-actions.ts) — real editors get it from the brinkStudio
// baseline, so a bare mount must add it to keep Ctrl-. opening the menu.
import { editorActionKeymap } from "../editor-actions.js";
import { resetDismissRegistryForTests } from "../dismiss-registry.js";

const DOC = "=== opening ===\nThe lights dim.\n-> END\n";
// See code-actions-escape-dismiss.test.ts: `data` is required by `CodeAction`;
// this test never resolves the action, so the marker is inert on purpose.
const ACTIONS: CodeAction[] = [
  { title: "Do the thing", kind: "quickfix", data: { action: "TestNoop" } },
];

function mount(): EditorView {
  return new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [editorActionKeymap(), codeActionsExtension({ getCodeActions: () => ACTIONS })],
    }),
    parent: document.body,
  });
}

function openMenu(view: EditorView): HTMLElement {
  runScopeHandlers(view, new KeyboardEvent("keydown", { key: ".", ctrlKey: true }), "editor");
  const menu = document.querySelector<HTMLElement>(".brink-code-actions-menu");
  if (menu === null) throw new Error("code-actions menu not opened");
  return menu;
}

afterEach(() => {
  document.body.replaceChildren();
  resetDismissRegistryForTests();
});

describe("code-actions menu — orphaned local listener still closes via the global net (#279)", () => {
  it("Escape still dismisses when the menu's own capture-phase listener has been detached", () => {
    resetDismissRegistryForTests();
    const addSpy = vi.spyOn(document, "addEventListener");

    const view = mount();
    openMenu(view);

    // `open()` installs exactly ONE document-level capture-phase keydown
    // listener: its own. The registry's global net is installed on `window`,
    // bubble phase — deliberately NOT `document`/capture — so it never shows
    // up in this filter; see the "LISTENER ORDERING" note on
    // dismiss-registry.ts.
    const keydownCalls = addSpy.mock.calls.filter(
      (call) => call[0] === "keydown" && call[2] === true,
    );
    expect(keydownCalls).toHaveLength(1);
    const localHandler = keydownCalls[0][1] as EventListener;
    addSpy.mockRestore();

    // Simulate the orphan: the menu's own listener is gone, but nothing
    // else about the open menu changed — it's still in the DOM, and the
    // registry's separate registration was never touched.
    document.removeEventListener("keydown", localHandler, true);

    expect(document.querySelector(".brink-code-actions-menu")).not.toBeNull();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    // The detached local listener could not have done this — only the
    // independent registry-based net could.
    expect(document.querySelector(".brink-code-actions-menu")).toBeNull();

    view.destroy();
  });

  it("a normal Escape (nothing orphaned) closes the menu without a leaked global listener", () => {
    resetDismissRegistryForTests();
    const view = mount();
    openMenu(view);

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(document.querySelector(".brink-code-actions-menu")).toBeNull();

    view.destroy();
  });

  it("on the SECOND open, with the net already pre-installed, Escape still runs the menu's own close (view.focus(), preventDefault()) — not just the net's bare dismiss", () => {
    // Deliberately no `resetDismissRegistryForTests()` before the FIRST open
    // below — this test recreates the production shape: the net installs
    // once, on this first `registerDismissible()` call, and stays installed
    // for every menu opened after. Before the ordering fix, the net's
    // `document`-capture listener would then run BEFORE the SECOND menu's own
    // (freshly attached, so registered later) `document`-capture listener,
    // calling `this.close()` first — which removes `this.onKeyDown` mid-
    // dispatch (never invoked) so `this.view.focus()` never ran and
    // `e.preventDefault()` was skipped, leaking Escape to CM6's keymap.
    resetDismissRegistryForTests();
    const view = mount();

    // First open + close: installs the net (via registerDismissible in
    // open()) and leaves it installed after this menu closes.
    openMenu(view);
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(document.querySelector(".brink-code-actions-menu")).toBeNull();

    // Second open — the net is already live; this menu's own document-capture
    // listener is attached fresh, strictly after it.
    openMenu(view);
    const focusSpy = vi.spyOn(view, "focus");
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.dispatchEvent(event);

    expect(document.querySelector(".brink-code-actions-menu")).toBeNull();
    // The menu's own onKeyDown ran (not just the net's bare `close()`): it
    // calls `e.preventDefault()` and `this.view.focus()` before the net ever
    // gets a turn.
    expect(event.defaultPrevented).toBe(true);
    expect(focusSpy).toHaveBeenCalledTimes(1);

    view.destroy();
  });
});
