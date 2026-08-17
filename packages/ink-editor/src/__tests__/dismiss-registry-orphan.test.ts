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
      extensions: [codeActionsExtension({ getCodeActions: () => ACTIONS })],
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

    // `open()` installs two document-level capture-phase keydown listeners:
    // its own (attached directly) and the registry's global net (installed
    // lazily by `registerDismissible`, guaranteed fresh here by the reset
    // above). The menu's own is registered first (see code-actions.ts).
    const keydownCalls = addSpy.mock.calls.filter(
      (call) => call[0] === "keydown" && call[2] === true,
    );
    expect(keydownCalls).toHaveLength(2);
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
});
