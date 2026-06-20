import { type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import type { Location } from "@brink/wasm-types";

export interface RenameOptions {
  /** Returns the renameable range at `offset` (view coords), or null. */
  prepareRename: (source: string, offset: number) => Location | null;
  /**
   * Begin a rename of the symbol under the cursor. `offset` is the cursor
   * position in view coords; `currentName` is the symbol's current text. The
   * host opens the safe-by-default rename prompt (cross-file + breakage report)
   * — F2 no longer applies edits itself (#305).
   */
  startRename: (offset: number, currentName: string) => void;
}

export function renameExtension(options: RenameOptions): Extension {
  return keymap.of([
    {
      key: "F2",
      run(view: EditorView): boolean {
        const pos = view.state.selection.main.head;
        const source = view.state.doc.toString();

        let range: Location | null;
        try {
          range = options.prepareRename(source, pos);
        } catch {
          return false;
        }
        if (!range) return false;

        const currentName = source.slice(range.start, range.end);
        options.startRename(pos, currentName);
        return true;
      },
    },
  ]);
}
