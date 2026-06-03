import { type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, keymap } from "@codemirror/view";
import type { CodeAction } from "@brink/wasm-types";

export interface CodeActionsOptions {
  getCodeActions: (source: string, offset: number) => CodeAction[];
}

/**
 * Owns the code-actions popup menu and its outside-click dismiss listener so
 * both are torn down in `destroy()` — otherwise an open menu (and its
 * `document` click listener) would leak when the editor unmounts.
 */
class CodeActionsMenu {
  private menu: HTMLElement | null = null;
  private dismiss: ((e: MouseEvent) => void) | null = null;

  constructor(private readonly view: EditorView) {}

  open(actions: CodeAction[], pos: number): void {
    this.close();

    const menu = document.createElement("div");
    menu.className = "brink-code-actions-menu";

    const coords = this.view.coordsAtPos(pos);
    if (coords) {
      menu.style.position = "fixed";
      menu.style.left = `${coords.left}px`;
      menu.style.top = `${coords.bottom + 4}px`;
    }

    for (const action of actions) {
      const item = document.createElement("button");
      item.className = "brink-code-action-item";
      item.textContent = action.title;
      // Code actions would need resolve_code_action on the wasm side; for now
      // selecting one just dismisses the menu.
      item.addEventListener("click", () => this.close());
      menu.appendChild(item);
    }

    const dismiss = (e: MouseEvent) => {
      if (!menu.contains(e.target as Node)) this.close();
    };
    this.menu = menu;
    this.dismiss = dismiss;
    // Defer attaching so the opening keystroke/click doesn't immediately close it.
    setTimeout(() => {
      if (this.dismiss === dismiss) document.addEventListener("click", dismiss);
    }, 0);

    document.body.appendChild(menu);
  }

  private close(): void {
    if (this.dismiss) {
      document.removeEventListener("click", this.dismiss);
      this.dismiss = null;
    }
    if (this.menu) {
      this.menu.remove();
      this.menu = null;
    }
  }

  destroy(): void {
    this.close();
  }
}

const codeActionsMenu = ViewPlugin.fromClass(CodeActionsMenu);

export function codeActionsExtension(options: CodeActionsOptions): Extension {
  return [
    codeActionsMenu,
    keymap.of([
      {
        key: "Ctrl-.",
        mac: "Cmd-.",
        run(view: EditorView): boolean {
          const pos = view.state.selection.main.head;
          const source = view.state.doc.toString();

          let actions: CodeAction[];
          try {
            actions = options.getCodeActions(source, pos);
          } catch {
            return false;
          }

          if (actions.length === 0) return false;

          view.plugin(codeActionsMenu)?.open(actions, pos);
          return true;
        },
      },
    ]),
  ];
}
