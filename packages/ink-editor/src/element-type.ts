import { StateField, type EditorState, type Transaction } from "@codemirror/state";
import type { LineContext, WeaveElement } from "@brink/wasm-types";
import { documentHandleFacet } from "./document-handle.js";

export { type LineContext } from "@brink/wasm-types";

export enum ElementType {
  KnotHeader,
  StitchHeader,
  NarrativeText,
  Choice,
  ChoiceBody,
  Gather,
  Divert,
  Logic,
  VarDecl,
  Comment,
  Include,
  External,
  Tag,
  Blank,
  Character,
  Parenthetical,
  Dialogue,
}

export interface LineInfo {
  type: ElementType;
  depth: number;
  /** Whether the choice/gather uses sticky (+) sigils */
  sticky: boolean;
  /** Whether a divert is standalone (just "-> target", not a tunnel) */
  standalone: boolean;
  /**
   * Option identity: the full lineage of option indices through the weave for
   * `Choice` and `ChoiceBody` lines (e.g. `[0, 2, 1]` — third option under the
   * first option's sub-weave, second option of that group). Zero-based per
   * weave level; gathers close their level's groups so a following option at
   * the same depth starts a new group at index 0. Absent on all other lines.
   */
  optionPath?: readonly number[];
}

const ELEMENT_CLASSES: Record<ElementType, string> = {
  [ElementType.KnotHeader]: "brink-knot-header",
  [ElementType.StitchHeader]: "brink-stitch-header",
  [ElementType.NarrativeText]: "brink-narrative",
  [ElementType.Choice]: "brink-choice",
  [ElementType.ChoiceBody]: "brink-choice-body",
  [ElementType.Gather]: "brink-gather",
  [ElementType.Divert]: "brink-divert",
  [ElementType.Logic]: "brink-logic",
  [ElementType.VarDecl]: "brink-var-decl",
  [ElementType.Comment]: "brink-comment",
  [ElementType.Include]: "brink-include",
  [ElementType.External]: "brink-external",
  [ElementType.Tag]: "brink-tag",
  [ElementType.Blank]: "brink-blank",
  [ElementType.Character]: "brink-character",
  [ElementType.Parenthetical]: "brink-parenthetical",
  [ElementType.Dialogue]: "brink-dialogue",
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
  let type = lineElementToType(ctx.element);
  // Narrative inside a choice body is "choice body", not plain narrative
  if (type === ElementType.NarrativeText && ctx.weave.element === "choice_body") {
    type = ElementType.ChoiceBody;
  }
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

  if (/^@[^:]*:<>$/.test(trimmed)) {
    return { type: ElementType.Character, depth: 0, sticky: false, standalone: false };
  }

  if (/^\([^)]*\)<>$/.test(trimmed)) {
    return { type: ElementType.Parenthetical, depth: 0, sticky: false, standalone: false };
  }

  return { type: ElementType.NarrativeText, depth: 0, sticky: false, standalone: false };
}

// ── Option identity post-pass (#364) ────────────────────────────────
// Assigns every Choice line and its ChoiceBody lines an option path — the
// full lineage of zero-based option indices through the weave — so hosts can
// tell consecutive options at the same depth apart (and know which parent a
// nested option belongs to) without re-deriving the weave themselves.
//
// Rules:
// - A Choice at depth d closes any open options deeper than d, takes the next
//   index in the current depth-d group, and becomes the open option at d.
// - A Gather at depth d closes the depth-d group (and everything deeper);
//   the next Choice at depth d starts a new group at index 0.
// - ChoiceBody lines inherit the innermost open option's path.
// - Knot/stitch headers reset the weave entirely.

/** Mutates `infos` in place, setting `optionPath` on Choice/ChoiceBody lines. */
export function assignOptionPaths(infos: LineInfo[]): void {
  // path[k] = index of the currently open option whose lineage position is k.
  const path: number[] = [];
  // counters[d - 1] = next option index for the open group at weave depth d.
  const counters: number[] = [];

  for (const info of infos) {
    switch (info.type) {
      case ElementType.KnotHeader:
      case ElementType.StitchHeader:
        path.length = 0;
        counters.length = 0;
        break;

      case ElementType.Choice: {
        const d = Math.max(1, info.depth);
        // Close options deeper than this one; keep the depth-d group counting.
        if (path.length >= d) path.length = d - 1;
        if (counters.length > d) counters.length = d;
        while (counters.length < d) counters.push(0);
        const index = counters[d - 1];
        counters[d - 1] = index + 1;
        path.push(index);
        info.optionPath = [...path];
        break;
      }

      case ElementType.Gather: {
        const d = Math.max(1, info.depth);
        // A gather at depth d closes its level's group and everything deeper.
        if (path.length >= d) path.length = d - 1;
        if (counters.length >= d) counters.length = d - 1;
        break;
      }

      case ElementType.ChoiceBody:
        if (path.length > 0) info.optionPath = [...path];
        break;

      default:
        // Other lines (narrative, logic, diverts, blanks, …) neither open nor
        // close option groups.
        break;
    }
  }
}

// ── StateField ──────────────────────────────────────────────────────

function computeLineInfos(state: EditorState): LineInfo[] {
  // The view's own document handle (per-view DocId, see document-handle.ts).
  // Pushing here keeps the wasm session in sync with this view on every doc
  // change, before any extension queries run against the new state.
  const handle = state.facet(documentHandleFacet)?.handle ?? null;
  if (handle) {
    handle.pushSource(state.doc.toString());
    const contexts = handle.lineContexts();

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
    // Post-pass: blank/whitespace-only lines immediately after a Choice or
    // ChoiceBody are still inside the choice body — promote them so Tab works.
    // Only promote lines that are truly blank (no sigils in the text).
    for (let i = 1; i < infos.length; i++) {
      if (infos[i].type !== ElementType.Blank) continue;
      const lineText = state.doc.line(i + 1).text.trimStart();
      if (lineText !== "" && /^[*+\-]/.test(lineText)) continue;
      const prev = infos[i - 1];
      if (prev.type === ElementType.Choice || prev.type === ElementType.ChoiceBody) {
        infos[i] = { type: ElementType.ChoiceBody, depth: prev.depth, sticky: false, standalone: false };
      }
    }

    // Screenplay post-pass: recognize @Name:<>, (text)<>, and dialogue
    for (let i = 0; i < infos.length; i++) {
      const lt = state.doc.line(i + 1).text;
      const trimmed = lt.trimStart();
      // Preserve the line's weave depth (screenplay elements inside a choice
      // body keep their indentation, so Tab/Shift-Tab weave math stays correct).
      if (/^@[^:]*:<>$/.test(trimmed)) {
        infos[i] = { type: ElementType.Character, depth: infos[i].depth, sticky: false, standalone: false };
      } else if (/^\([^)]*\)<>$/.test(trimmed)) {
        infos[i] = { type: ElementType.Parenthetical, depth: infos[i].depth, sticky: false, standalone: false };
      }
    }
    // Narrative after character/parenthetical/dialogue → dialogue
    for (let i = 1; i < infos.length; i++) {
      const prev = infos[i - 1];
      if (
        (prev.type === ElementType.Character || prev.type === ElementType.Parenthetical || prev.type === ElementType.Dialogue) &&
        infos[i].type === ElementType.NarrativeText
      ) {
        infos[i] = { type: ElementType.Dialogue, depth: infos[i].depth, sticky: false, standalone: false };
      }
    }

    assignOptionPaths(infos);
    return infos;
  }

  // Fallback: no session yet, use regex classifier
  const infos: LineInfo[] = [];
  for (let i = 1; i <= state.doc.lines; i++) {
    const line = state.doc.line(i);
    infos.push(classifyLine(line.text));
  }
  assignOptionPaths(infos);
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
