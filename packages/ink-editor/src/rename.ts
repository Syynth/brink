import { type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import type { Location, FileEdit } from "@brink/wasm-types";

export interface RenameOptions {
  prepareRename: (source: string, offset: number) => Location | null;
  doRename: (source: string, offset: number, newName: string) => FileEdit[];
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

        const oldName = source.slice(range.start, range.end);
        const newName = prompt("Rename symbol:", oldName);

        if (!newName || newName === oldName) return false;

        let edits: FileEdit[];
        try {
          edits = options.doRename(source, pos, newName);
        } catch {
          return false;
        }

        if (edits.length === 0) return false;

        // Apply edits in reverse order to preserve positions
        const sorted = [...edits].sort((a, b) => b.start - a.start);
        const changes = sorted.map((edit) => ({
          from: edit.start,
          to: edit.end,
          insert: edit.new_text,
        }));

        view.dispatch({ changes });
        return true;
      },
    },
  ]);
}
