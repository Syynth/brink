import { EditorView } from "@codemirror/view";
import { ElementType, type LineInfo } from "./element-type.js";
import type { ConvertTarget } from "@brink/wasm-types";
import type { EditorSessionHandle } from "@brink/wasm";
import { sigilBypass } from "./screenplay.js";
import { extractLineContent } from "@brink/ink-operations";

// ── Line utilities ─────────────────────────────────────────────────

/** Build sigil prefix: depth 2 with "*" → "* *" */
export function buildSigils(sigil: string, depth: number): string {
  return Array.from({ length: depth }, () => sigil).join(" ");
}

/** Find the byte range of the sigil prefix in a line. */
export function parseSigilPrefix(text: string, sigils: string[]): { end: number; count: number } {
  const ws = text.length - text.trimStart().length;
  let pos = ws;
  let count = 0;
  while (pos < text.length) {
    if (sigils.includes(text[pos])) {
      count++;
      pos++;
      while (pos < text.length && text[pos] === " ") pos++;
    } else {
      break;
    }
  }
  return { end: pos, count };
}

/** Whether a line has content after its sigil prefix. */
export function lineHasContent(text: string, info: LineInfo): boolean {
  if (info.type === ElementType.Choice) {
    const { end } = parseSigilPrefix(text, ["*", "+"]);
    return text.slice(end).trim() !== "";
  }
  if (info.type === ElementType.Gather) {
    const { end } = parseSigilPrefix(text, ["-"]);
    return text.slice(end).trim() !== "";
  }
  if (info.type === ElementType.ChoiceBody) {
    return text.trim() !== "";
  }
  if (info.type === ElementType.Character) {
    // Content is the name between @ and :<>
    const trimmed = text.trimStart();
    const m = trimmed.match(/^@([^:]*):<>$/);
    return m !== null && m[1].trim() !== "";
  }
  if (info.type === ElementType.Parenthetical) {
    // Content is text between ( and )<>
    const trimmed = text.trimStart();
    const m = trimmed.match(/^\((.*)\)<>$/);
    return m !== null && m[1].trim() !== "";
  }
  return text.trim() !== "";
}

// ── Context queries ────────────────────────────────────────────────

/** Find the nearest non-blank line above `lineIndex` (0-based). */
export function findPrev(infos: LineInfo[], lineIndex: number): LineInfo | null {
  for (let i = lineIndex - 1; i >= 0; i--) {
    if (infos[i].type !== ElementType.Blank) return infos[i];
  }
  return null;
}

/** Find the nearest ancestor choice/gather above `lineIndex` (0-based). */
export function findAncestor(infos: LineInfo[], lineIndex: number): LineInfo | null {
  for (let i = lineIndex - 1; i >= 0; i--) {
    const t = infos[i].type;
    if (t === ElementType.Choice || t === ElementType.Gather) return infos[i];
    // Stop at structural boundaries
    if (t === ElementType.KnotHeader || t === ElementType.StitchHeader) return null;
  }
  return null;
}

export interface LineContext {
  prev: LineInfo | null;
  ancestor: LineInfo | null;
}

export function buildContext(infos: LineInfo[], lineIndex: number): LineContext {
  return {
    prev: findPrev(infos, lineIndex),
    ancestor: findAncestor(infos, lineIndex),
  };
}

// ── Action types ───────────────────────────────────────────────────

export type ActionId =
  | "newSibling"
  | "convertToBody"
  | "newBodyLine"
  | "convertToNarrative"
  | "convertToIndentedNarrative"
  | "increaseDepth"
  | "decreaseDepth"
  | "convertToChoice"
  | "convertToGather"
  | "trap"
  | "newDialogueLine"
  | "convertToParenthetical"
  | "convertToDialogue"
  | "clearScreenplaySigils"
  | "stripToNarrative";

// ── Transition table ───────────────────────────────────────────────

export interface TransitionContext {
  /** Nearest non-blank line above must be one of these types. */
  prev?: ElementType[];
  /** Nearest non-blank line above must NOT be one of these types. */
  prevNot?: ElementType[];
  /** Nearest choice/gather ancestor must be one of these types. */
  ancestor?: ElementType[];
}

export interface Transition {
  element: ElementType;
  key: "Enter" | "Shift-Enter" | "Tab" | "Shift-Tab";
  /** If specified, only matches when the line has/doesn't have content after sigils. */
  hasContent?: boolean;
  /** If specified, only matches at this depth (or "min2" for depth >= 2). */
  depth?: "min2";
  /** If specified, surrounding lines must match these conditions. */
  context?: TransitionContext;
  /** The action to execute. */
  action: ActionId;
  /** Human-readable hint for the status bar. */
  hint: string;
}

export const TRANSITIONS: Transition[] = [
  // ── Choice ───────────────────────────────────────────────────────
  // Enter
  { element: ElementType.Choice, key: "Enter",       hasContent: true,  action: "newSibling",               hint: "new choice" },
  { element: ElementType.Choice, key: "Enter",       hasContent: false, action: "convertToBody",            hint: "body text" },
  // Shift+Enter
  { element: ElementType.Choice, key: "Shift-Enter",                    action: "newBodyLine",              hint: "body text" },
  // Tab: choice → choice body
  { element: ElementType.Choice, key: "Tab",                            action: "convertToIndentedNarrative", hint: "body text" },
  // Shift+Tab: choice → gather
  { element: ElementType.Choice, key: "Shift-Tab",                      action: "convertToGather",          hint: "gather" },

  // ── Choice Body ─────────────────────────────────────────────────
  // Tab: choice body → gather
  { element: ElementType.ChoiceBody, key: "Tab",                        action: "convertToGather",          hint: "gather" },
  // Shift+Tab: choice body → choice
  { element: ElementType.ChoiceBody, key: "Shift-Tab",                  action: "convertToChoice",          hint: "choice" },

  // ── Gather ───────────────────────────────────────────────────────
  // Tab: gather → choice
  { element: ElementType.Gather, key: "Tab",                            action: "convertToChoice",          hint: "choice" },
  // Shift+Tab: gather → choice body
  { element: ElementType.Gather, key: "Shift-Tab",                      action: "convertToBody",            hint: "choice body" },

  // ── Narrative ────────────────────────────────────────────────────
  // Tab: narrative → gather (enters the cycle)
  { element: ElementType.NarrativeText, key: "Tab",                     action: "convertToGather",          hint: "gather" },

  // ── Character ─────────────────────────────────────────────────
  { element: ElementType.Character, key: "Tab",         hasContent: true,  action: "convertToParenthetical",   hint: "parenthetical" },
  { element: ElementType.Character, key: "Tab",         hasContent: false, action: "trap",                     hint: "" },
  { element: ElementType.Character, key: "Enter",       hasContent: true,  action: "newDialogueLine",          hint: "dialogue" },
  { element: ElementType.Character, key: "Enter",       hasContent: false, action: "clearScreenplaySigils",    hint: "clear" },
  { element: ElementType.Character, key: "Shift-Tab",                      action: "stripToNarrative",         hint: "narrative" },

  // ── Parenthetical ─────────────────────────────────────────────
  { element: ElementType.Parenthetical, key: "Tab",                        action: "convertToDialogue",        hint: "dialogue" },
  { element: ElementType.Parenthetical, key: "Enter",   hasContent: true,  action: "newDialogueLine",          hint: "dialogue" },
  { element: ElementType.Parenthetical, key: "Enter",   hasContent: false, action: "convertToDialogue",        hint: "dialogue" },
  { element: ElementType.Parenthetical, key: "Shift-Tab",                  action: "stripToNarrative",         hint: "narrative" },

  // ── Dialogue ──────────────────────────────────────────────────
  { element: ElementType.Dialogue, key: "Tab",                             action: "convertToParenthetical",   hint: "parenthetical" },
  { element: ElementType.Dialogue, key: "Enter",        hasContent: true,  action: "newDialogueLine",          hint: "dialogue" },
  { element: ElementType.Dialogue, key: "Enter",        hasContent: false, action: "stripToNarrative",         hint: "narrative" },
  { element: ElementType.Dialogue, key: "Shift-Tab",                       action: "stripToNarrative",         hint: "narrative" },
];

// ── Matching ───────────────────────────────────────────────────────

function contextMatches(ctx: TransitionContext, lineCtx: LineContext): boolean {
  if (ctx.prev !== undefined) {
    if (!lineCtx.prev || !ctx.prev.includes(lineCtx.prev.type)) return false;
  }
  if (ctx.prevNot !== undefined) {
    if (lineCtx.prev && ctx.prevNot.includes(lineCtx.prev.type)) return false;
  }
  if (ctx.ancestor !== undefined) {
    if (!lineCtx.ancestor || !ctx.ancestor.includes(lineCtx.ancestor.type)) return false;
  }
  return true;
}

function transitionMatches(
  t: Transition,
  info: LineInfo,
  key: string,
  hasContent: boolean,
  lineCtx: LineContext,
): boolean {
  if (t.element !== info.type) return false;
  if (t.key !== key) return false;
  if (t.hasContent !== undefined && t.hasContent !== hasContent) return false;
  if (t.depth === "min2" && info.depth < 2) return false;
  if (t.context && !contextMatches(t.context, lineCtx)) return false;
  return true;
}

export function findTransition(
  info: LineInfo,
  key: string,
  hasContent: boolean,
  lineCtx: LineContext,
): Transition | undefined {
  return TRANSITIONS.find((t) => transitionMatches(t, info, key, hasContent, lineCtx));
}

/** Get all transitions for a given element type (for status bar hints). */
export function getHintsForElement(
  info: LineInfo,
  hasContent: boolean,
  lineCtx: LineContext,
): { key: string; hint: string }[] {
  const hints: { key: string; hint: string }[] = [];
  const seen = new Set<string>();

  for (const t of TRANSITIONS) {
    if (!transitionMatches(t, info, t.key, hasContent, lineCtx)) continue;
    if (seen.has(t.key)) continue;
    seen.add(t.key);
    hints.push({ key: formatKey(t.key), hint: t.hint });
  }

  return hints;
}

function formatKey(key: string): string {
  switch (key) {
    case "Shift-Enter": return "Shift+Enter";
    case "Shift-Tab": return "Shift+Tab";
    default: return key;
  }
}

// ── Action execution ───────────────────────────────────────────────

function applyConversion(view: EditorView, session: EditorSessionHandle | null, target: ConvertTarget): boolean {
  if (!session) return true; // trap the key even if session unavailable
  const cursorPos = view.state.selection.main.head;
  const edit = session.convertElement(cursorPos, target);
  if (!edit) return true; // trap the key even if conversion not applicable
  view.dispatch({
    changes: { from: edit.from, to: edit.to, insert: edit.insert },
    selection: { anchor: edit.from + edit.insert.length },
  });
  return true;
}

export function executeAction(action: ActionId, view: EditorView, info: LineInfo, session: EditorSessionHandle | null): boolean {
  const { state } = view;
  const cursorPos = state.selection.main.head;

  switch (action) {
    case "newSibling": {
      const sigil = info.type === ElementType.Gather
        ? "-"
        : info.sticky ? "+" : "*";
      const prefix = buildSigils(sigil, info.depth) + " ";
      view.dispatch(state.update({
        changes: { from: cursorPos, insert: "\n" + prefix },
        selection: { anchor: cursorPos + 1 + prefix.length },
      }));
      return true;
    }

    case "convertToBody":
      return applyConversion(view, session, "choice_body");

    case "newBodyLine": {
      const indent = "  ".repeat(info.depth);
      view.dispatch(state.update({
        changes: { from: cursorPos, insert: "\n" + indent },
        selection: { anchor: cursorPos + 1 + indent.length },
      }));
      return true;
    }

    case "convertToIndentedNarrative":
      return applyConversion(view, session, "choice_body");

    case "convertToNarrative":
      return applyConversion(view, session, "narrative");

    case "increaseDepth":
    case "decreaseDepth":
      // These don't map to a simple ConvertTarget — keep TS fallback for now
      return executeDepthChange(action, view, info);

    case "convertToChoice":
      return applyConversion(view, session, "choice");

    case "convertToGather":
      return applyConversion(view, session, "gather");

    case "trap":
      return true;

    case "newDialogueLine": {
      view.dispatch(state.update({
        changes: { from: cursorPos, insert: "\n" },
        selection: { anchor: cursorPos + 1 },
      }));
      return true;
    }

    case "convertToParenthetical": {
      const line = state.doc.lineAt(cursorPos);
      const content = extractLineContent(line.text);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: "(" + content + ")<>" },
        selection: { anchor: line.from + 1 + content.length }, // after content, before )<>
        annotations: sigilBypass.of(true),
      });
      return true;
    }

    case "convertToDialogue": {
      const line = state.doc.lineAt(cursorPos);
      const content = extractLineContent(line.text);
      const ws = line.text.length - line.text.trimStart().length;
      const prefix = line.text.slice(0, ws);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: prefix + content },
        selection: { anchor: line.from + ws + content.length },
        annotations: sigilBypass.of(true),
      });
      return true;
    }

    case "clearScreenplaySigils": {
      const line = state.doc.lineAt(cursorPos);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: "" },
        selection: { anchor: line.from },
        annotations: sigilBypass.of(true),
      });
      return true;
    }

    case "stripToNarrative": {
      const line = state.doc.lineAt(cursorPos);
      const ws = line.text.length - line.text.trimStart().length;
      const prefix = line.text.slice(0, ws);
      const content = extractLineContent(line.text);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: prefix + content },
        selection: { anchor: line.from + ws + content.length },
        annotations: sigilBypass.of(true),
      });
      return true;
    }
  }
}

const CHOICE_SIGILS = ["*", "+"] as const;
const GATHER_SIGILS = ["-"] as const;

function executeDepthChange(action: "increaseDepth" | "decreaseDepth", view: EditorView, info: LineInfo): boolean {
  const { state } = view;
  const line = state.doc.lineAt(state.selection.main.head);
  const delta = action === "increaseDepth" ? 1 : -1;
  const newDepth = info.depth + delta;
  if (newDepth < 1) return false;

  const sigil = info.type === ElementType.Gather
    ? "-"
    : info.sticky ? "+" : "*";
  const sigils = info.type === ElementType.Gather ? [...GATHER_SIGILS] : [...CHOICE_SIGILS];
  const newPrefix = buildSigils(sigil, newDepth) + " ";
  const { end } = parseSigilPrefix(line.text, sigils);
  view.dispatch({
    changes: { from: line.from, to: line.from + end, insert: newPrefix },
    selection: { anchor: line.from + newPrefix.length },
  });
  return true;
}
