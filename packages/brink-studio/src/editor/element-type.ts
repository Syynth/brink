import { StateField, type EditorState, type Transaction } from "@codemirror/state";
import type { LineContext, WeaveElement, EditorSessionHandle } from "../wasm.js";

export { type LineContext } from "../wasm.js";

export enum ElementType {
  KnotHeader,
  StitchHeader,
  NarrativeText,
  Choice,
  Gather,
  Divert,
  Logic,
  VarDecl,
  Comment,
  Include,
  External,
  Tag,
  Blank,
}

export interface LineInfo {
  type: ElementType;
  depth: number;
  /** Whether the choice/gather uses sticky (+) sigils */
  sticky: boolean;
  /** Whether a divert is standalone (just "-> target", not a tunnel) */
  standalone: boolean;
}

const ELEMENT_CLASSES: Record<ElementType, string> = {
  [ElementType.KnotHeader]: "brink-knot-header",
  [ElementType.StitchHeader]: "brink-stitch-header",
  [ElementType.NarrativeText]: "brink-narrative",
  [ElementType.Choice]: "brink-choice",
  [ElementType.Gather]: "brink-gather",
  [ElementType.Divert]: "brink-divert",
  [ElementType.Logic]: "brink-logic",
  [ElementType.VarDecl]: "brink-var-decl",
  [ElementType.Comment]: "brink-comment",
  [ElementType.Include]: "brink-include",
  [ElementType.External]: "brink-external",
  [ElementType.Tag]: "brink-tag",
  [ElementType.Blank]: "brink-blank",
};

export function elementClass(type: ElementType): string {
  return ELEMENT_CLASSES[type];
}

// ── LineContext → LineInfo conversion ────────────────────────────────

function lineElementToType(element: string): ElementType {
  switch (element) {
    case "knot_header": return ElementType.KnotHeader;
    case "stitch_header": return ElementType.StitchHeader;
    case "narrative": return ElementType.NarrativeText;
    case "choice": return ElementType.Choice;
    case "gather": return ElementType.Gather;
    case "divert": return ElementType.Divert;
    case "logic": return ElementType.Logic;
    case "var_decl": return ElementType.VarDecl;
    case "comment": return ElementType.Comment;
    case "include": return ElementType.Include;
    case "external": return ElementType.External;
    case "tag": return ElementType.Tag;
    default: return ElementType.Blank;
  }
}

function isSticky(weaveElement: WeaveElement): boolean {
  if (typeof weaveElement === "object" && "choice_line" in weaveElement) {
    return weaveElement.choice_line.sticky;
  }
  return false;
}

function lineContextToLineInfo(ctx: LineContext, lineText: string): LineInfo {
  const type = lineElementToType(ctx.element);
  const depth = ctx.weave.depth;
  const sticky = isSticky(ctx.weave.element);

  // Determine standalone for diverts (not a tunnel: "-> target ->")
  let standalone = false;
  if (type === ElementType.Divert) {
    const trimmed = lineText.trimStart();
    standalone = trimmed.startsWith("->") && !/^->.*->/.test(trimmed);
  }

  return { type, depth, sticky, standalone };
}

// ── Regex fallback for when session hasn't been updated yet ─────────

function classifyLine(text: string): LineInfo {
  const trimmed = text.trimStart();

  if (trimmed === "") {
    return { type: ElementType.Blank, depth: 0, sticky: false, standalone: false };
  }

  if (/^={2,}\s*\w/.test(trimmed) || /^={3,}/.test(trimmed)) {
    return { type: ElementType.KnotHeader, depth: 0, sticky: false, standalone: false };
  }

  if (/^=\s+\w/.test(trimmed) || (trimmed.startsWith("=") && !trimmed.startsWith("==") && /^=\s*\w/.test(trimmed))) {
    return { type: ElementType.StitchHeader, depth: 0, sticky: false, standalone: false };
  }

  if (/^[*+]/.test(trimmed)) {
    let depth = 0;
    let sticky = false;
    let i = 0;
    while (i < trimmed.length && (trimmed[i] === "*" || trimmed[i] === "+")) {
      if (trimmed[i] === "+") sticky = true;
      depth++;
      i++;
      while (i < trimmed.length && trimmed[i] === " ") i++;
    }
    return { type: ElementType.Choice, depth, sticky, standalone: false };
  }

  if (trimmed.startsWith("-") && !trimmed.startsWith("->")) {
    let depth = 0;
    let i = 0;
    while (i < trimmed.length && trimmed[i] === "-") {
      depth++;
      i++;
      while (i < trimmed.length && trimmed[i] === " ") i++;
    }
    return { type: ElementType.Gather, depth, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("->")) {
    const isTunnel = /^->.*->/.test(trimmed);
    return { type: ElementType.Divert, depth: 0, sticky: false, standalone: !isTunnel };
  }

  if (trimmed.startsWith("~")) {
    return { type: ElementType.Logic, depth: 0, sticky: false, standalone: false };
  }

  if (/^(VAR|CONST|LIST)\s/.test(trimmed)) {
    return { type: ElementType.VarDecl, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("//") || trimmed.startsWith("/*")) {
    return { type: ElementType.Comment, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("INCLUDE ")) {
    return { type: ElementType.Include, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("EXTERNAL ")) {
    return { type: ElementType.External, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("#")) {
    return { type: ElementType.Tag, depth: 0, sticky: false, standalone: false };
  }

  return { type: ElementType.NarrativeText, depth: 0, sticky: false, standalone: false };
}

// ── StateField ──────────────────────────────────────────────────────

/** Facet-like holder for the session reference, set during editor creation. */
let _sessionRef: EditorSessionHandle | null = null;

export function setEditorSession(session: EditorSessionHandle): void {
  _sessionRef = session;
}

function computeLineInfos(state: EditorState): LineInfo[] {
  if (_sessionRef) {
    // Update the session with current source
    const source = state.doc.toString();
    _sessionRef.updateSource(source);
    const contexts = _sessionRef.getLineContexts();

    const infos: LineInfo[] = [];
    for (let i = 0; i < contexts.length && i < state.doc.lines; i++) {
      const line = state.doc.line(i + 1);
      infos.push(lineContextToLineInfo(contexts[i], line.text));
    }
    // Fill remaining lines with regex fallback (shouldn't happen normally)
    for (let i = infos.length; i < state.doc.lines; i++) {
      const line = state.doc.line(i + 1);
      infos.push(classifyLine(line.text));
    }
    return infos;
  }

  // Fallback: no session yet, use regex classifier
  const infos: LineInfo[] = [];
  for (let i = 1; i <= state.doc.lines; i++) {
    const line = state.doc.line(i);
    infos.push(classifyLine(line.text));
  }
  return infos;
}

export const elementTypeField = StateField.define<LineInfo[]>({
  create(state) {
    return computeLineInfos(state);
  },
  update(value, tr: Transaction) {
    if (!tr.docChanged) return value;
    return computeLineInfos(tr.state);
  },
});
