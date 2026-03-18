import { EditorView } from "@codemirror/view";
import { ElementType, type LineInfo } from "./element-type.js";

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
  | "increaseDepth"
  | "decreaseDepth"
  | "convertToChoice"
  | "convertToGather"
  | "trap";

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
  { element: ElementType.Choice, key: "Enter",       hasContent: true,  action: "newSibling",      hint: "new choice" },
  { element: ElementType.Choice, key: "Enter",       hasContent: false, action: "convertToBody",   hint: "body text" },
  // Shift+Enter
  { element: ElementType.Choice, key: "Shift-Enter",                    action: "newBodyLine",     hint: "body text" },
  // Tab
  { element: ElementType.Choice, key: "Tab",         hasContent: true,  action: "increaseDepth",   hint: "deeper" },
  { element: ElementType.Choice, key: "Tab",         hasContent: false, action: "convertToBody",   hint: "body text" },
  // Shift+Tab
  { element: ElementType.Choice, key: "Shift-Tab",   depth: "min2",     action: "decreaseDepth",   hint: "shallower" },
  { element: ElementType.Choice, key: "Shift-Tab",                      action: "convertToGather", hint: "gather" },

  // ── Gather ───────────────────────────────────────────────────────
  { element: ElementType.Gather, key: "Enter",       hasContent: true,  action: "newSibling",        hint: "new gather" },
  { element: ElementType.Gather, key: "Enter",       hasContent: false, action: "convertToNarrative", hint: "narrative" },
  { element: ElementType.Gather, key: "Tab",                            action: "convertToChoice",   hint: "choice" },
  { element: ElementType.Gather, key: "Shift-Tab",   depth: "min2",     action: "decreaseDepth",     hint: "shallower" },
  { element: ElementType.Gather, key: "Shift-Tab",                      action: "convertToNarrative", hint: "narrative" },

  // ── Narrative ────────────────────────────────────────────────────
  { element: ElementType.NarrativeText, key: "Tab",                     action: "convertToChoice",   hint: "choice" },
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

const CHOICE_SIGILS = ["*", "+"] as const;
const GATHER_SIGILS = ["-"] as const;

export function executeAction(action: ActionId, view: EditorView, info: LineInfo): boolean {
  const { state } = view;
  const cursorPos = state.selection.main.head;
  const line = state.doc.lineAt(cursorPos);

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

    case "convertToBody": {
      const indent = "  ".repeat(info.depth);
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: indent },
        selection: { anchor: line.from + indent.length },
      });
      return true;
    }

    case "newBodyLine": {
      const indent = "  ".repeat(info.depth);
      view.dispatch(state.update({
        changes: { from: cursorPos, insert: "\n" + indent },
        selection: { anchor: cursorPos + 1 + indent.length },
      }));
      return true;
    }

    case "convertToNarrative": {
      const sigils = info.type === ElementType.Gather ? [...GATHER_SIGILS] : [...CHOICE_SIGILS];
      const { end } = parseSigilPrefix(line.text, sigils);
      view.dispatch({
        changes: { from: line.from, to: line.from + end, insert: "" },
        selection: { anchor: line.from },
      });
      return true;
    }

    case "increaseDepth": {
      const sigil = info.type === ElementType.Gather
        ? "-"
        : info.sticky ? "+" : "*";
      const sigils = info.type === ElementType.Gather ? [...GATHER_SIGILS] : [...CHOICE_SIGILS];
      const newPrefix = buildSigils(sigil, info.depth + 1) + " ";
      const { end } = parseSigilPrefix(line.text, sigils);
      view.dispatch({
        changes: { from: line.from, to: line.from + end, insert: newPrefix },
        selection: { anchor: line.from + newPrefix.length },
      });
      return true;
    }

    case "decreaseDepth": {
      const sigil = info.type === ElementType.Gather
        ? "-"
        : info.sticky ? "+" : "*";
      const sigils = info.type === ElementType.Gather ? [...GATHER_SIGILS] : [...CHOICE_SIGILS];
      const newPrefix = buildSigils(sigil, info.depth - 1) + " ";
      const { end } = parseSigilPrefix(line.text, sigils);
      view.dispatch({
        changes: { from: line.from, to: line.from + end, insert: newPrefix },
        selection: { anchor: line.from + newPrefix.length },
      });
      return true;
    }

    case "convertToChoice": {
      const sigils = info.type === ElementType.Gather ? [...GATHER_SIGILS] : [];
      const { end } = parseSigilPrefix(line.text, sigils);
      const newPrefix = "* ";
      view.dispatch({
        changes: { from: line.from, to: line.from + end, insert: newPrefix },
        selection: { anchor: line.from + newPrefix.length },
      });
      return true;
    }

    case "convertToGather": {
      const sigils = info.type === ElementType.Choice ? [...CHOICE_SIGILS] : [];
      const { end } = parseSigilPrefix(line.text, sigils);
      const newPrefix = "- ";
      view.dispatch({
        changes: { from: line.from, to: line.from + end, insert: newPrefix },
        selection: { anchor: line.from + newPrefix.length },
      });
      return true;
    }

    case "trap":
      return true;
  }
}
