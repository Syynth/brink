import { type Extension, Prec } from "@codemirror/state";
import { keymap, type EditorView } from "@codemirror/view";
import { elementTypeField, ElementType, getEditorSession } from "./element-type.js";
import { findTransition, lineHasContent, executeAction, buildContext } from "./transitions.js";

const HANDLED_KEYS = ["Enter", "Shift-Enter", "Tab", "Shift-Tab", "Backspace"] as const;

function handleKey(key: string, view: EditorView): boolean {
  const { state } = view;
  const infos = state.field(elementTypeField);
  const line = state.doc.lineAt(state.selection.main.head);
  const lineIndex = line.number - 1;
  const info = infos[lineIndex];

  if (!info) {
    return key === "Tab" || key === "Shift-Tab";
  }

  // Tab on double-blank: insert @:<> character template
  if (key === "Tab" && info.type === ElementType.Blank && line.text.trim() === "") {
    const prevBlank = lineIndex > 0 && infos[lineIndex - 1].type === ElementType.Blank;
    if (prevBlank) {
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: "@:<>" },
        selection: { anchor: line.from + 1 }, // cursor between @ and :
      });
      return true;
    }
  }

  // Backspace on empty character line (@:<>): clear entire line
  if (key === "Backspace" && info.type === ElementType.Character) {
    const trimmed = line.text.trimStart();
    if (trimmed === "@:<>") {
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: "" },
        selection: { anchor: line.from },
      });
      return true;
    }
  }

  const hasContent = lineHasContent(line.text, info);
  const lineCtx = buildContext(infos, lineIndex);
  const transition = findTransition(info, key, hasContent, lineCtx);

  if (!transition) {
    return key === "Tab" || key === "Shift-Tab";
  }

  return executeAction(transition.action, view, info, getEditorSession());
}

export function brinkKeymap(): Extension {
  return Prec.highest(
    keymap.of(
      HANDLED_KEYS.map((key) => ({
        key,
        run: (view: EditorView) => handleKey(key, view),
      })),
    ),
  );
}
