import { type Extension, Prec } from "@codemirror/state";
import { keymap, type EditorView } from "@codemirror/view";
import { elementTypeField, ElementType, dialectFacet } from "./element-type.js";
import { documentHandleFacet } from "./document-handle.js";
import { sigilBypass, characterName } from "./screenplay.js";
import { findTransition, lineHasContent, executeAction, buildContext, tryDialectTransition } from "./transitions.js";
import { CONVERTIBLE_TYPES, convertLineToType } from "./convert.js";
import { ensureStructuralStyles } from "./structural-styles.js";

const HANDLED_KEYS = ["Enter", "Shift-Enter", "Tab", "Shift-Tab", "Backspace", "Delete"] as const;

/**
 * Lines whose text may be absorbed into a character name (`@Name:<>`) by the
 * Delete/Backspace fold handlers. Only plain content folds — folding a
 * structural line (another character, choice, gather, header, divert, …) would
 * splice its sigils into the name and corrupt the syntax (e.g. `@Alice@Bob:<>:<>`).
 */
function isFoldableIntoName(type: ElementType): boolean {
  return (
    type === ElementType.NarrativeText ||
    type === ElementType.Dialogue ||
    type === ElementType.Blank
  );
}

function handleKey(key: string, view: EditorView): boolean {
  const { state } = view;
  const infos = state.field(elementTypeField);
  const line = state.doc.lineAt(state.selection.main.head);
  const lineIndex = line.number - 1;
  const info = infos[lineIndex];

  if (!info) {
    return key === "Tab" || key === "Shift-Tab";
  }

  // Tab on double-blank: insert @:<> character template. This is a
  // dialect-provided behavior (the at-cue preset's blank-tab template), NOT
  // a structural row — with the keymap now always mounted (#368: only the
  // screenplay decorations are dialect-gated, the structural weave table is
  // interpreter-owned), it must self-guard on an active dialect, unlike the
  // Character-kind branches below (whose kinds simply never appear when no
  // dialect is active).
  if (
    key === "Tab" &&
    info.type === ElementType.Blank &&
    line.text.trim() === "" &&
    state.facet(dialectFacet) !== null
  ) {
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
    const { ws, nameStart, nameEnd, name } = characterName(line, info);
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
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: line.text.slice(0, ws) + name },
        selection: { anchor: line.from + ws },
        annotations: sigilBypass.of(true),
      });
      return true;
    }

    // Delete at name end: fold the next line into the name — only when that
    // line is plain. Folding a structural line would corrupt the syntax.
    if (key === "Delete" && head === nameEnd) {
      const nextInfo = infos[lineIndex + 1];
      if (line.number < state.doc.lines && nextInfo && isFoldableIntoName(nextInfo.type)) {
        const nextLine = state.doc.line(line.number + 1);
        const nextText = nextLine.text;
        view.dispatch({
          changes: { from: line.from, to: nextLine.to, insert: "@" + name + nextText + ":<>" },
          selection: { anchor: line.from + 1 + name.length + nextText.length },
          annotations: sigilBypass.of(true),
        });
      }
      // Consume Delete at name end regardless: the cursor sits before the atomic
      // `:<>`, so the default forward-delete has nothing safe to do.
      return true;
    }

    // Enter: split at cursor — @Left:<> stays, Right goes to next line
    // Skip when name is empty — fall through to clearScreenplaySigils transition
    if (key === "Enter" && nameStart < nameEnd) {
      const suffixLen = line.to - nameEnd; // width of the trailing hidden ':<>' (3 for at-cue)
      // Name-start offset relative to line.text — derived from `nameStart`
      // (the dialect's actual content-span start), not a hardcoded "prefix
      // is exactly 1 char" assumption.
      const nameStartInLine = nameStart - line.from;
      const leftName = line.text.slice(nameStartInLine, head - line.from);
      const rightName = line.text.slice(head - line.from, line.text.length - suffixLen);
      const prefix = line.text.slice(0, ws);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: prefix + "@" + leftName + ":<>\n" + rightName },
        selection: { anchor: line.from + prefix.length + 1 + leftName.length + suffixLen + 1 },
        annotations: sigilBypass.of(true),
      });
      return true;
    }
  }

  // Backspace at start of line after a character line: fold this line's content
  // into the name — only when this line is plain. A structural line must not be
  // folded (it would splice its sigils into the name); fall through to the
  // default line-join instead.
  if (key === "Backspace" && lineIndex > 0 && state.selection.main.head === line.from) {
    const prevInfo = infos[lineIndex - 1];
    if (prevInfo?.type === ElementType.Character && isFoldableIntoName(info.type)) {
      const prevLine = state.doc.line(line.number - 1);
      const { name: prevName } = characterName(prevLine, prevInfo);
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

  // Dialect transition overlay (#368 deliverable 4): rows for a dialect-
  // declared kind resolve BEFORE the built-in structural weave table. The
  // at-cue preset ships no rows, so this is inert for the default preset.
  // Read from `state.facet(dialectFacet)` (per-view, #368 fix) — NOT a
  // module global — so this view's transition overlay always matches its
  // own mounted dialect, even when another view has a different one.
  if (tryDialectTransition(state.facet(dialectFacet), view, info, key, hasContent)) {
    return true;
  }

  const lineCtx = buildContext(infos, lineIndex);
  const transition = findTransition(info, key, hasContent, lineCtx);

  if (!transition) {
    return key === "Tab" || key === "Shift-Tab";
  }

  return executeAction(
    transition.action,
    view,
    info,
    view.state.facet(documentHandleFacet)?.handle ?? null,
  );
}

/** For character lines, find the editable name region (after @, before :<>). */
function characterNameRange(view: EditorView): { start: number; end: number } | null {
  const { state } = view;
  const infos = state.field(elementTypeField);
  const line = state.doc.lineAt(state.selection.main.head);
  const info = infos[line.number - 1];
  if (!info || info.type !== ElementType.Character) return null;

  // Name is between @ and :<>
  const { nameStart, nameEnd } = characterName(line, info);
  return { start: nameStart, end: nameEnd };
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
      const { nameEnd } = characterName(prevLine, prevInfo);
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
  ensureStructuralStyles();

  const dropdown = document.createElement("div");
  dropdown.className = "brink-element-dropdown brink-inline-picker";
  // Placement is data (custom properties); `.brink-inline-picker`'s class rule
  // positions the dropdown — hosts restyle the classes directly (#363).
  dropdown.style.setProperty("--brink-popup-left", `${cursor.left}px`);
  dropdown.style.setProperty("--brink-popup-top", `${cursor.bottom + 4}px`);

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
    } else if (e.key === "Shift" || e.key === "Alt" || e.key === "Control" || e.key === "Meta") {
      // Ignore modifier-only keypresses
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
