import { type Extension, Prec } from "@codemirror/state";
import { keymap, type EditorView } from "@codemirror/view";
import { elementTypeField, ElementType, getEditorSession } from "./element-type.js";
import { sigilBypass } from "./screenplay.js";
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
        annotations: sigilBypass.of(true),
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
        annotations: sigilBypass.of(true),
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

/** For character lines, find the editable name region (after @, before :<>). */
function characterNameRange(view: EditorView): { start: number; end: number } | null {
  const { state } = view;
  const infos = state.field(elementTypeField);
  const line = state.doc.lineAt(state.selection.main.head);
  const info = infos[line.number - 1];
  if (!info || info.type !== ElementType.Character) return null;

  const trimmed = line.text.trimStart();
  const ws = line.text.length - trimmed.length;
  // Name is between @ and :<>
  return { start: line.from + ws + 1, end: line.to - 3 };
}

function handleHome(view: EditorView): boolean {
  const range = characterNameRange(view);
  if (!range) return false;

  const head = view.state.selection.main.head;
  // If already at or before name start, trap
  if (head <= range.start) return true;
  // Otherwise, move to name start
  view.dispatch({ selection: { anchor: range.start } });
  return true;
}

function handleEnd(view: EditorView): boolean {
  const range = characterNameRange(view);
  if (!range) return false;

  const head = view.state.selection.main.head;
  // If already at or past name end, trap
  if (head >= range.end) return true;
  // Otherwise, move to name end
  view.dispatch({ selection: { anchor: range.end } });
  return true;
}

function handleArrowRight(view: EditorView): boolean {
  const range = characterNameRange(view);
  if (!range) return false;

  const head = view.state.selection.main.head;
  // At or past end of name: skip :<> and newline, land on next line
  if (head >= range.end) {
    const line = view.state.doc.lineAt(head);
    if (line.number < view.state.doc.lines) {
      const nextLine = view.state.doc.line(line.number + 1);
      view.dispatch({ selection: { anchor: nextLine.from } });
    }
    return true;
  }
  return false;
}

function handleArrowLeft(view: EditorView): boolean {
  const { state } = view;
  const head = state.selection.main.head;
  const line = state.doc.lineAt(head);

  // If cursor is at start of a line, check if previous line is a character line
  if (head === line.from && line.number > 1) {
    const prevLine = state.doc.line(line.number - 1);
    const infos = state.field(elementTypeField);
    const prevInfo = infos[prevLine.number - 1];
    if (prevInfo?.type === ElementType.Character) {
      // Jump to end of name on previous character line (before :<>)
      const nameEnd = prevLine.to - 3;
      view.dispatch({ selection: { anchor: nameEnd } });
      return true;
    }
  }
  return false;
}

export function brinkKeymap(): Extension {
  return Prec.highest(
    keymap.of([
      ...HANDLED_KEYS.map((key) => ({
        key,
        run: (view: EditorView) => handleKey(key, view),
      })),
      { key: "Home", run: handleHome },
      { key: "End", run: handleEnd },
      { key: "ArrowRight", run: handleArrowRight },
      { key: "ArrowLeft", run: handleArrowLeft },
    ]),
  );
}
