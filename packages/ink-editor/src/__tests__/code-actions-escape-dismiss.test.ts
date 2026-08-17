/**
 * The code-actions popup menu (Ctrl-./Cmd-. — the "editor context menu" #279
 * names as a suspect) must be dismissible by Escape and outside-pointerdown
 * from the moment it opens.
 *
 * The pre-fix `CodeActionsMenu.open()` had two dismiss bugs, both invisible
 * to a hand read that only checks "does it have an Escape handler":
 *
 *  1. Escape was handled by a listener on the MENU ELEMENT itself
 *     (`menu.addEventListener("keydown", keyNav)`), not `document`. A
 *     keydown only reaches an element's listener if it bubbles through that
 *     element's subtree. `open()` deferred moving focus into the menu
 *     (`items[0]?.focus()`) by one tick so the opening keystroke wouldn't
 *     immediately re-trigger anything — so for that whole tick,
 *     `document.activeElement` was still whatever had focus before Ctrl-.
 *     was pressed, NOT the menu. An Escape pressed in that window (a fast
 *     keyboard user, or — as reproduced here — any Escape dispatched at
 *     `document` rather than routed through the menu's own DOM subtree)
 *     never reached `keyNav` at all: the menu was unescapable. This is
 *     exactly #279's "Escape did nothing".
 *  2. Outside-dismiss listened for a bubble-phase `click` on `document`,
 *     unlike `Overlay`'s capture-phase `pointerdown` contract — weaker, and
 *     inconsistent with every other transient surface in the app.
 *
 * The fix moves both to `document`, capture phase, matching Overlay
 * (overlay.tsx) — see code-actions.ts.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import type { CodeAction } from "@brink/wasm-types";
import { codeActionsExtension } from "../code-actions.js";

const DOC = "=== opening ===\nThe lights dim.\n-> END\n";

const ACTIONS: CodeAction[] = [{ title: "Do the thing", kind: "quickfix" }];

function mount(onSelect?: (action: CodeAction) => void): EditorView {
  return new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        codeActionsExtension({
          getCodeActions: () => ACTIONS,
          onSelect,
        }),
      ],
    }),
    parent: document.body,
  });
}

/** Run the Ctrl-./Cmd-. keymap the way CM6 dispatches it (jsdom does not
 *  route a raw keydown through the keymap facet) — mirrors
 *  extract-actions.test.ts's `openMenu`. */
function openMenu(view: EditorView): HTMLElement {
  const handled = runScopeHandlers(
    view,
    new KeyboardEvent("keydown", { key: ".", ctrlKey: true }),
    "editor",
  );
  const menu = document.querySelector<HTMLElement>(".brink-code-actions-menu");
  if (menu === null) throw new Error(`code-actions menu not opened (handled=${handled})`);
  return menu;
}

function menuGone(): boolean {
  return document.querySelector(".brink-code-actions-menu") === null;
}

function pressEscapeAt(target: EventTarget): void {
  target.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
  );
}

describe("code-actions menu dismissal (#279)", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it("closes on Escape dispatched at document — the pre-focus-tick window", () => {
    const view = mount();
    openMenu(view);
    // Do NOT wait for the deferred focus-into-menu tick. This is the exact
    // window the pre-fix implementation could never dismiss from.
    pressEscapeAt(document);
    expect(menuGone()).toBe(true);
    view.destroy();
  });

  it("still closes on Escape after focus has moved into the menu", async () => {
    const view = mount();
    openMenu(view);
    await new Promise((resolve) => setTimeout(resolve, 0));
    pressEscapeAt(document);
    expect(menuGone()).toBe(true);
    view.destroy();
  });

  it("closes on outside pointerdown", () => {
    const view = mount();
    openMenu(view);
    document.body.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, cancelable: true }),
    );
    expect(menuGone()).toBe(true);
    view.destroy();
  });

  it("does NOT close on a pointerdown inside the menu itself", () => {
    const view = mount();
    const menu = openMenu(view);
    menu.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
    expect(menuGone()).toBe(false);
    view.destroy();
  });

  it("Arrow keys still navigate items once focus is inside the menu", async () => {
    const view = mount();
    const menu = openMenu(view);
    await new Promise((resolve) => setTimeout(resolve, 0));
    const items = [...menu.querySelectorAll<HTMLButtonElement>(".brink-code-action-item")];
    expect(document.activeElement).toBe(items[0]);
    menu.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    // Single-item menu: ArrowDown wraps back to the same (only) item.
    expect(document.activeElement).toBe(items[0]);
    view.destroy();
  });
});
