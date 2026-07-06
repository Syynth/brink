import { type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, keymap } from "@codemirror/view";
import type { CodeAction } from "@brink/wasm-types";
import { ensureStructuralStyles } from "./structural-styles.js";

export interface CodeActionsOptions {
  /** Actions available at `offset` (cursor). Resolved + applied via `onSelect`. */
  getCodeActions: (source: string, offset: number) => CodeAction[];
  /**
   * Extra, selection-derived actions merged ahead of `getCodeActions` — the
   * synthetic "Extract to knot/function" entries (#315 H) when there is a
   * multi-line selection. Kept separate from the wasm `getCodeActions` because
   * extraction has its own op + name-prompt dispatch, not a `resolveCodeAction`
   * payload. Ordered first so they head the menu.
   */
  getSelectionActions?: (view: EditorView) => CodeAction[];
  /**
   * Invoke the chosen action. The host resolves + applies it (the #321 studio
   * apply seam) — for a normal action through `resolveCodeAction`, for a
   * synthetic extract action through the name-prompt flow. The menu closes
   * before this runs, so a prompt widget can take focus. When absent, choosing
   * an action just dismisses the menu (the pre-#315 placeholder behavior).
   */
  onSelect?: (action: CodeAction, view: EditorView) => void;
}

/**
 * Owns the code-actions popup menu and its outside-click dismiss listener so
 * both are torn down in `destroy()` — otherwise an open menu (and its
 * `document` click listener) would leak when the editor unmounts.
 */
class CodeActionsMenu {
  private menu: HTMLElement | null = null;
  private dismiss: ((e: MouseEvent) => void) | null = null;
  private keyNav: ((e: KeyboardEvent) => void) | null = null;

  constructor(
    private readonly view: EditorView,
    private readonly onSelect?: (action: CodeAction, view: EditorView) => void,
  ) {}

  open(actions: CodeAction[], pos: number): void {
    this.close();
    ensureStructuralStyles();

    const menu = document.createElement("div");
    menu.className = "brink-code-actions-menu";
    menu.setAttribute("role", "menu");
    menu.setAttribute("aria-label", "Code actions");

    // Position at the caret. `coordsAtPos` can throw in environments without a
    // real layout (jsdom); a positioning failure must not sink the menu.
    let coords: { left: number; bottom: number } | null = null;
    try {
      coords = this.view.coordsAtPos(pos);
    } catch {
      coords = null;
    }
    // Placement is data (custom properties), not inline styles — the class
    // rule positions the menu; hosts restyle `.brink-code-actions-menu` (#363).
    if (coords) {
      menu.style.setProperty("--brink-popup-left", `${coords.left}px`);
      menu.style.setProperty("--brink-popup-top", `${coords.bottom + 4}px`);
    }

    const items: HTMLButtonElement[] = [];
    for (const action of actions) {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "brink-code-action-item";
      item.setAttribute("role", "menuitem");
      item.textContent = action.title;
      item.addEventListener("click", () => this.select(action));
      menu.appendChild(item);
      items.push(item);
    }

    const dismiss = (e: MouseEvent) => {
      if (!menu.contains(e.target as Node)) this.close();
    };
    // Keyboard reachability: ↑/↓ move between items, Enter activates, Esc
    // dismisses. Focus starts on the first item so the menu is usable with no
    // pointer.
    const keyNav = (e: KeyboardEvent) => {
      const active = document.activeElement;
      const idx = items.indexOf(active as HTMLButtonElement);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        items[(idx + 1 + items.length) % items.length]?.focus();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        items[(idx - 1 + items.length) % items.length]?.focus();
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.close();
        this.view.focus();
      }
    };

    this.menu = menu;
    this.dismiss = dismiss;
    this.keyNav = keyNav;
    // Defer attaching so the opening keystroke/click doesn't immediately close it.
    setTimeout(() => {
      if (this.dismiss === dismiss) document.addEventListener("click", dismiss);
      items[0]?.focus();
    }, 0);
    menu.addEventListener("keydown", keyNav);

    document.body.appendChild(menu);
  }

  private select(action: CodeAction): void {
    // Close first so a name-prompt widget the handler mounts can take focus.
    this.close();
    this.onSelect?.(action, this.view);
  }

  private close(): void {
    if (this.dismiss) {
      document.removeEventListener("click", this.dismiss);
      this.dismiss = null;
    }
    if (this.menu) {
      if (this.keyNav) this.menu.removeEventListener("keydown", this.keyNav);
      this.menu.remove();
      this.menu = null;
    }
    this.keyNav = null;
  }

  destroy(): void {
    this.close();
  }
}

export function codeActionsExtension(options: CodeActionsOptions): Extension {
  const codeActionsMenu = ViewPlugin.define(
    (view) => new CodeActionsMenu(view, options.onSelect),
  );

  return [
    codeActionsMenu,
    keymap.of([
      {
        key: "Ctrl-.",
        mac: "Cmd-.",
        run(view: EditorView): boolean {
          const pos = view.state.selection.main.head;
          const source = view.state.doc.toString();

          const actions: CodeAction[] = [];
          // Selection-derived (extract) actions head the list.
          if (options.getSelectionActions) {
            try {
              actions.push(...options.getSelectionActions(view));
            } catch {
              // ignore — fall through to cursor actions
            }
          }
          try {
            actions.push(...options.getCodeActions(source, pos));
          } catch {
            if (actions.length === 0) return false;
          }

          if (actions.length === 0) return false;

          view.plugin(codeActionsMenu)?.open(actions, pos);
          return true;
        },
      },
    ]),
  ];
}
