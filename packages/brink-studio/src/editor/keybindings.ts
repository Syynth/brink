import { type Extension, Prec } from "@codemirror/state";
import { keymap, type EditorView } from "@codemirror/view";
import { elementTypeField } from "./element-type.js";
import { findTransition, lineHasContent, executeAction, buildContext } from "./transitions.js";

const HANDLED_KEYS = ["Enter", "Shift-Enter", "Tab", "Shift-Tab"] as const;

function handleKey(key: string, view: EditorView): boolean {
  const { state } = view;
  const infos = state.field(elementTypeField);
  const line = state.doc.lineAt(state.selection.main.head);
  const lineIndex = line.number - 1;
  const info = infos[lineIndex];

  if (!info) {
    return key === "Tab" || key === "Shift-Tab";
  }

  const hasContent = lineHasContent(line.text, info);
  const lineCtx = buildContext(infos, lineIndex);
  const transition = findTransition(info, key, hasContent, lineCtx);

  if (!transition) {
    return key === "Tab" || key === "Shift-Tab";
  }

  return executeAction(transition.action, view, info);
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
