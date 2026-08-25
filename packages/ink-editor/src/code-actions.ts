import { type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, keymap } from "@codemirror/view";
import type { CodeAction } from "@brink/wasm-types";
import { ensureStructuralStyles } from "./structural-styles.js";
import { registerDismissible } from "./dismiss-registry.js";

export interface CodeActionsOptions {
  /** Actions available at `offset` (cursor). Resolved + applied via `onSelect`. */
  /** Sync or async (W2c of `docs/editor-worker-spec.md`). An async
   *  result opens the menu only if the document and cursor held still
   *  while it was in flight; a rejected pull contributes no actions
   *  (selection-derived extract actions still show). */
  getCodeActions: (source: string, offset: number) => CodeAction[] | Promise<CodeAction[]>;
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
 * Owns the code-actions popup menu and its dismiss listeners so both are
 * torn down in `destroy()` — otherwise an open menu (and its `document`
 * listeners) would leak when the editor unmounts.
 */
class CodeActionsMenu {
  private menu: HTMLElement | null = null;
  private onPointerDown: ((e: PointerEvent) => void) | null = null;
  private onKeyDown: ((e: KeyboardEvent) => void) | null = null;
  private navKeyDown: ((e: KeyboardEvent) => void) | null = null;
  private unregisterDismiss: (() => void) | null = null;

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

    // Keyboard reachability within the menu: ↑/↓ move between items. Scoped
    // to `menu` (not document) on purpose — it only matters once focus is
    // already inside.
    const navKeyDown = (e: KeyboardEvent) => {
      const active = document.activeElement;
      const idx = items.indexOf(active as HTMLButtonElement);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        items[(idx + 1 + items.length) % items.length]?.focus();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        items[(idx - 1 + items.length) % items.length]?.focus();
      }
    };

    // Dismissal: Escape anywhere, pointerdown outside the menu — `document`,
    // capture phase, matching Overlay's dismiss contract (#279). The
    // previous shape scoped Escape to the menu ELEMENT itself
    // (`menu.addEventListener("keydown", ...)`) and deferred attaching an
    // outside-CLICK listener by one tick to dodge the opening keystroke —
    // but a keydown dispatched anywhere other than the menu's own subtree
    // (including, in the real bug, any Escape pressed before that deferred
    // tick moved focus in) never reached it: the menu was unescapable. Both
    // listeners here are safe to attach immediately: they're a different
    // event type (pointerdown/keydown-Escape) from the Ctrl-./Cmd-. keydown
    // that opened the menu, so there's no risk of the opening keystroke
    // self-triggering a dismiss.
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      this.close();
      this.view.focus();
    };
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target;
      if (target instanceof Node && menu.contains(target)) return;
      this.close();
    };

    this.menu = menu;
    this.onPointerDown = onPointerDown;
    this.onKeyDown = onKeyDown;
    this.navKeyDown = navKeyDown;
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("pointerdown", onPointerDown, true);
    menu.addEventListener("keydown", navKeyDown);
    // Global Escape safety net (#279) — registered separately from the
    // listeners above so a bug in that logic (or a re-render that orphans
    // it) can't take this registration down with it. See dismiss-registry.ts.
    this.unregisterDismiss = registerDismissible(() => this.close());
    // Defer moving focus so the opening keystroke doesn't get re-delivered
    // to the first item.
    setTimeout(() => {
      if (this.menu === menu) items[0]?.focus();
    }, 0);

    document.body.appendChild(menu);
  }

  private select(action: CodeAction): void {
    // Close first so a name-prompt widget the handler mounts can take focus.
    this.close();
    this.onSelect?.(action, this.view);
  }

  private close(): void {
    if (this.onKeyDown) {
      document.removeEventListener("keydown", this.onKeyDown, true);
      this.onKeyDown = null;
    }
    if (this.onPointerDown) {
      document.removeEventListener("pointerdown", this.onPointerDown, true);
      this.onPointerDown = null;
    }
    if (this.unregisterDismiss) {
      this.unregisterDismiss();
      this.unregisterDismiss = null;
    }
    if (this.menu) {
      if (this.navKeyDown) this.menu.removeEventListener("keydown", this.navKeyDown);
      this.menu.remove();
      this.menu = null;
    }
    this.navKeyDown = null;
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

          const open = (wasmActions: CodeAction[]): boolean => {
            actions.push(...wasmActions);
            if (actions.length === 0) return false;
            view.plugin(codeActionsMenu)?.open(actions, pos);
            return true;
          };

          let produced: CodeAction[] | Promise<CodeAction[]>;
          try {
            produced = options.getCodeActions(source, pos);
          } catch {
            produced = [];
          }
          if (produced instanceof Promise) {
            const doc = view.state.doc;
            void produced.then(
              (wasmActions) => {
                // The menu anchors at `pos` — open it only if the doc and
                // cursor held still while the pull was in flight.
                if (!view.dom.isConnected) return;
                if (view.state.doc !== doc) return;
                if (view.state.selection.main.head !== pos) return;
                open(wasmActions);
              },
              () => {
                if (!view.dom.isConnected || view.state.doc !== doc) return;
                if (view.state.selection.main.head !== pos) return;
                open([]); // extract actions alone, matching the sync catch path
              },
            );
            // Claim the keybinding: the menu opens (or silently doesn't)
            // when the pull lands — async hosts trade the fall-through-on
            // -empty behavior for an off-thread pull.
            return true;
          }
          return open(produced);
        },
      },
    ]),
  ];
}
