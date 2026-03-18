import { StateField, type EditorState, type Transaction } from "@codemirror/state";

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

function classifyLine(text: string): LineInfo {
  const trimmed = text.trimStart();

  if (trimmed === "") {
    return { type: ElementType.Blank, depth: 0, sticky: false, standalone: false };
  }

  // Knot: === ...
  if (trimmed.startsWith("===") || (trimmed.startsWith("==") && trimmed.includes("=="))) {
    // More precise: line contains === or starts with === (knot declaration)
    if (/^={2,}\s*\w/.test(trimmed) || /^={3,}/.test(trimmed)) {
      return { type: ElementType.KnotHeader, depth: 0, sticky: false, standalone: false };
    }
  }

  // Stitch: = word (single equals followed by space and a word char)
  if (/^=\s+\w/.test(trimmed) || (trimmed.startsWith("=") && !trimmed.startsWith("==") && /^=\s*\w/.test(trimmed))) {
    return { type: ElementType.StitchHeader, depth: 0, sticky: false, standalone: false };
  }

  // Choice: leading * or + (count for depth)
  if (/^[*+]/.test(trimmed)) {
    let depth = 0;
    let sticky = false;
    let i = 0;
    while (i < trimmed.length && (trimmed[i] === "*" || trimmed[i] === "+")) {
      if (trimmed[i] === "+") sticky = true;
      depth++;
      i++;
      // Skip whitespace between sigils
      while (i < trimmed.length && trimmed[i] === " ") i++;
    }
    return { type: ElementType.Choice, depth, sticky, standalone: false };
  }

  // Gather: leading - (but not ->)
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

  // Divert: ->
  if (trimmed.startsWith("->")) {
    // Standalone: just "-> target", not a tunnel ("-> target ->")
    const isTunnel = /^->.*->/.test(trimmed);
    return { type: ElementType.Divert, depth: 0, sticky: false, standalone: !isTunnel };
  }

  // Logic: ~
  if (trimmed.startsWith("~")) {
    return { type: ElementType.Logic, depth: 0, sticky: false, standalone: false };
  }

  // Variable declarations
  if (/^(VAR|CONST|LIST)\s/.test(trimmed)) {
    return { type: ElementType.VarDecl, depth: 0, sticky: false, standalone: false };
  }

  // Comments
  if (trimmed.startsWith("//") || trimmed.startsWith("/*")) {
    return { type: ElementType.Comment, depth: 0, sticky: false, standalone: false };
  }

  // Include
  if (trimmed.startsWith("INCLUDE ")) {
    return { type: ElementType.Include, depth: 0, sticky: false, standalone: false };
  }

  // External
  if (trimmed.startsWith("EXTERNAL ")) {
    return { type: ElementType.External, depth: 0, sticky: false, standalone: false };
  }

  // Tag
  if (trimmed.startsWith("#")) {
    return { type: ElementType.Tag, depth: 0, sticky: false, standalone: false };
  }

  return { type: ElementType.NarrativeText, depth: 0, sticky: false, standalone: false };
}

function computeLineInfos(state: EditorState): LineInfo[] {
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
