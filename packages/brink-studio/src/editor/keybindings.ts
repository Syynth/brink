import { type Extension, Prec } from "@codemirror/state";
import { keymap, type EditorView } from "@codemirror/view";
import { elementTypeField, ElementType, getEditorSession } from "./element-type.js";
import { sigilBypass } from "./screenplay.js";
import { findTransition, lineHasContent, executeAction, buildContext } from "./transitions.js";
import { CONVERTIBLE_TYPES, convertLineToType } from "./convert.js";

const HANDLED_KEYS = ["Enter", "Shift-Enter", "Tab", "Shift-Tab", "Backspace", "Delete"] as const;

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

  // Character line special handlers
  if (info.type === ElementType.Character) {
    const trimmed = line.text.trimStart();
    const ws = line.text.length - trimmed.length;
    const nameStart = line.from + ws + 1; // after @
    const nameEnd = line.to - 3;          // before :<>
    const head = state.selection.main.head;

    // Backspace on empty (@:<>): clear entire line
    if (key === "Backspace" && trimmed === "@:<>") {
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: "" },
        selection: { anchor: line.from },
        annotations: sigilBypass.of(true),
      });
      return true;
    }

    // Backspace at name start: strip all sigils, leave name as plain text
    if (key === "Backspace" && head === nameStart) {
      const name = line.text.slice(ws + 1, line.text.length - 3);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: line.text.slice(0, ws) + name },
        selection: { anchor: line.from + ws },
        annotations: sigilBypass.of(true),
      });
      return true;
    }

    // Delete at name end: fold next line content into name
    if (key === "Delete" && head === nameEnd) {
      if (line.number < state.doc.lines) {
        const nextLine = state.doc.line(line.number + 1);
        const nextText = nextLine.text;
        const name = line.text.slice(ws + 1, line.text.length - 3);
        view.dispatch({
          changes: { from: line.from, to: nextLine.to, insert: "@" + name + nextText + ":<>" },
          selection: { anchor: line.from + 1 + name.length + nextText.length },
          annotations: sigilBypass.of(true),
        });
      }
      return true;
    }

    // Enter: split at cursor — @Left:<> stays, Right goes to next line
    // Skip when name is empty — fall through to clearScreenplaySigils transition
    if (key === "Enter" && nameStart < nameEnd) {
      const leftName = line.text.slice(ws + 1, head - line.from);
      const rightName = line.text.slice(head - line.from, line.text.length - 3);
      const prefix = line.text.slice(0, ws);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: prefix + "@" + leftName + ":<>\n" + rightName },
        selection: { anchor: line.from + prefix.length + 1 + leftName.length + 3 + 1 },
        annotations: sigilBypass.of(true),
      });
      return true;
    }
  }

  // Backspace at start of line after a character line: fold content into the name
  if (key === "Backspace" && lineIndex > 0 && state.selection.main.head === line.from) {
    const prevInfo = infos[lineIndex - 1];
    if (prevInfo?.type === ElementType.Character) {
      const prevLine = state.doc.line(line.number - 1);
      const prevTrimmed = prevLine.text.trimStart();
      const prevWs = prevLine.text.length - prevTrimmed.length;
      const prevName = prevLine.text.slice(prevWs + 1, prevLine.text.length - 3);
      const content = line.text;
      view.dispatch({
        changes: { from: prevLine.from, to: line.to, insert: "@" + prevName + content + ":<>" },
        selection: { anchor: prevLine.from + 1 + prevName.length },
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

// ── Inline element picker (Alt+Enter) ─────────────────────────────

function showInlineElementPicker(view: EditorView): boolean {
  // Remove any existing picker
  dismissInlineElementPicker();

  const cursor = view.coordsAtPos(view.state.selection.main.head);
  if (!cursor) return true;

  const dropdown = document.createElement("div");
  dropdown.className = "brink-element-dropdown brink-inline-picker";
  dropdown.style.position = "fixed";
  dropdown.style.left = `${cursor.left}px`;
  dropdown.style.top = `${cursor.bottom + 4}px`;

  let selectedIndex = 0;

  function renderItems(): void {
    dropdown.innerHTML = "";
    for (let i = 0; i < CONVERTIBLE_TYPES.length; i++) {
      const item = CONVERTIBLE_TYPES[i];
      const btn = document.createElement("button");
      btn.className = "brink-element-dropdown-item" + (i === selectedIndex ? " selected" : "");

      const label = document.createElement("span");
      label.textContent = item.label;
      btn.appendChild(label);

      const hint = document.createElement("span");
      hint.className = "brink-element-dropdown-key";
      hint.textContent = item.key.toUpperCase();
      btn.appendChild(hint);

      btn.addEventListener("mousedown", (e) => {
        e.preventDefault();
        convertLineToType(view, item.sigil);
        dismissInlineElementPicker();
      });
      dropdown.appendChild(btn);
    }
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = (selectedIndex + 1) % CONVERTIBLE_TYPES.length;
      renderItems();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = (selectedIndex - 1 + CONVERTIBLE_TYPES.length) % CONVERTIBLE_TYPES.length;
      renderItems();
    } else if (e.key === "Enter") {
      e.preventDefault();
      convertLineToType(view, CONVERTIBLE_TYPES[selectedIndex].sigil);
      dismissInlineElementPicker();
    } else if (e.key === "Escape") {
      e.preventDefault();
      dismissInlineElementPicker();
    } else {
      // Check for shortcut key match
      const match = CONVERTIBLE_TYPES.find((t) => t.key === e.key.toLowerCase() || t.key === e.key);
      if (match) {
        e.preventDefault();
        convertLineToType(view, match.sigil);
        dismissInlineElementPicker();
      } else {
        dismissInlineElementPicker();
      }
    }
  }

  function handleClick(): void {
    dismissInlineElementPicker();
  }

  renderItems();
  document.body.appendChild(dropdown);
  document.addEventListener("keydown", handleKeydown, true);
  document.addEventListener("mousedown", handleClick);

  // Store cleanup references
  (dropdown as any).__cleanup = () => {
    document.removeEventListener("keydown", handleKeydown, true);
    document.removeEventListener("mousedown", handleClick);
  };

  return true;
}

function dismissInlineElementPicker(): void {
  const existing = document.querySelector(".brink-inline-picker");
  if (existing) {
    (existing as any).__cleanup?.();
    existing.remove();
  }
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
      { key: "Alt-Enter", run: showInlineElementPicker },
    ]),
  );
}
