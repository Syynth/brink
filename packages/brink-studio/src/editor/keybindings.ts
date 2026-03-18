import { type Extension } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import { elementTypeField, ElementType } from "./element-type.js";

export function brinkKeymap(): Extension {
  return keymap.of([
    {
      key: "Enter",
      run(view) {
        const { state } = view;
        const infos = state.field(elementTypeField);
        const cursorPos = state.selection.main.head;
        const line = state.doc.lineAt(cursorPos);
        const lineIndex = line.number - 1;
        const info = infos[lineIndex];

        if (!info || info.type !== ElementType.Choice) {
          return false; // Let default Enter handle it
        }

        // Build the sigil prefix for the new choice line
        const sigil = info.sticky ? "+" : "*";
        const prefix = (sigil + " ").repeat(info.depth).slice(0, -1) + " ";
        // Actually, for depth 2 it should be "* * " not "* * * "
        // Simpler: repeat the sigil `depth` times separated by spaces
        const sigils = Array.from({ length: info.depth }, () => sigil).join(" ");

        view.dispatch(
          state.update({
            changes: {
              from: cursorPos,
              insert: "\n" + sigils + " ",
            },
            selection: {
              anchor: cursorPos + 1 + sigils.length + 1,
            },
          }),
        );

        return true;
      },
    },
  ]);
}
