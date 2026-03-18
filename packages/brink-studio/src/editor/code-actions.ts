import { type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import type { CodeAction } from "../wasm.js";

export interface CodeActionsOptions {
  getCodeActions: (source: string, offset: number) => CodeAction[];
}

export function codeActionsExtension(options: CodeActionsOptions): Extension {
  return keymap.of([
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

        // Create a simple popup menu for code actions
        const menu = document.createElement("div");
        menu.className = "brink-code-actions-menu";

        const coords = view.coordsAtPos(pos);
        if (coords) {
          menu.style.position = "fixed";
          menu.style.left = `${coords.left}px`;
          menu.style.top = `${coords.bottom + 4}px`;
        }

        for (const action of actions) {
          const item = document.createElement("button");
          item.className = "brink-code-action-item";
          item.textContent = action.title;
          item.addEventListener("click", () => {
            menu.remove();
            // Code actions would need resolve_code_action on the wasm side
            // For now, just dismiss
          });
          menu.appendChild(item);
        }

        // Dismiss on click outside
        const dismiss = (e: MouseEvent) => {
          if (!menu.contains(e.target as Node)) {
            menu.remove();
            document.removeEventListener("click", dismiss);
          }
        };
        setTimeout(() => document.addEventListener("click", dismiss), 0);

        document.body.appendChild(menu);
        return true;
      },
    },
  ]);
}
