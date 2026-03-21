import { type Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { elementTypeField, elementClass, ElementType } from "./element-type.js";

const DEPTH_INDENT_EM = 2;

// ── Superscript depth indicators ───────────────────────────────────

const SUPERSCRIPT_DIGITS = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"];

function superscriptNumber(n: number): string {
  return String(n)
    .split("")
    .map((ch) => SUPERSCRIPT_DIGITS[Number(ch)])
    .join("");
}

class DepthSigilWidget extends WidgetType {
  constructor(
    readonly sigil: string,
    readonly depth: number,
  ) {
    super();
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "brink-depth-sigil";
    span.textContent = this.sigil + superscriptNumber(this.depth) + " ";
    return span;
  }

  eq(other: DepthSigilWidget): boolean {
    return this.sigil === other.sigil && this.depth === other.depth;
  }
}

class EmptySigilWidget extends WidgetType {
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.style.display = "none";
    return span;
  }
  eq(): boolean { return true; }
}

// ── Sigil prefix parsing ───────────────────────────────────────────

/** Find the full sigil prefix range (all sigils + spaces) for a choice/gather line. */
function findSigilRange(
  text: string,
  type: ElementType,
): { start: number; end: number; sigil: string } | null {
  const trimmed = text.trimStart();
  const ws = text.length - trimmed.length;

  const validSigils =
    type === ElementType.Choice ? ["*", "+"] : type === ElementType.Gather ? ["-"] : [];
  if (validSigils.length === 0) return null;

  let pos = ws;
  let firstSigil = "";

  while (pos < text.length) {
    if (validSigils.includes(text[pos])) {
      if (!firstSigil) firstSigil = text[pos];
      pos++;
      while (pos < text.length && text[pos] === " ") pos++;
    } else {
      break;
    }
  }

  if (!firstSigil) return null;
  return { start: ws, end: pos, sigil: firstSigil };
}

// ── Line decorations ──────────────────────────────────────────────

function buildLineDecos(view: EditorView): DecorationSet {
  const infos = view.state.field(elementTypeField);
  const builder = new RangeSetBuilder<Decoration>();

  for (let i = 1; i <= view.state.doc.lines; i++) {
    const line = view.state.doc.line(i);
    const info = infos[i - 1];
    if (!info || info.type === ElementType.Blank) continue;

    const cls = elementClass(info.type);
    const attrs: Record<string, string> = { class: cls };

    // Indent choices/gathers at depth > 1
    if (
      (info.type === ElementType.Choice || info.type === ElementType.Gather) &&
      info.depth > 1
    ) {
      attrs.style = `padding-left: ${(info.depth - 1) * DEPTH_INDENT_EM}em`;
    }

    // Right-align standalone diverts only
    if (info.type === ElementType.Divert && info.standalone) {
      attrs.style = (attrs.style || "") + "; text-align: right";
    }

    builder.add(line.from, line.from, Decoration.line({ attributes: attrs }));

    // Replace sigil prefix with depth widget for choices/gathers at depth > 1
    if (
      (info.type === ElementType.Choice || info.type === ElementType.Gather) &&
      info.depth > 1
    ) {
      const range = findSigilRange(line.text, info.type);
      if (range) {
        const widget = new DepthSigilWidget(range.sigil, info.depth);
        builder.add(
          line.from + range.start,
          line.from + range.end,
          Decoration.replace({ widget }),
        );
      }
    }

    // Character line: hide @ prefix and :<> suffix
    if (info.type === ElementType.Character) {
      const trimmed = line.text.trimStart();
      const ws = line.text.length - trimmed.length;
      // Hide @ at start
      builder.add(
        line.from + ws,
        line.from + ws + 1,
        Decoration.replace({ widget: new EmptySigilWidget() }),
      );
      // Hide :<> at end
      const colonGlueStart = line.to - 3; // :<> is 3 chars
      if (colonGlueStart > line.from + ws + 1) {
        builder.add(
          colonGlueStart,
          line.to,
          Decoration.replace({ widget: new EmptySigilWidget() }),
        );
      }
    }

    // Parenthetical line: hide <> at end
    if (info.type === ElementType.Parenthetical) {
      const glueStart = line.to - 2; // <> is 2 chars
      if (glueStart > line.from) {
        builder.add(
          glueStart,
          line.to,
          Decoration.replace({ widget: new EmptySigilWidget() }),
        );
      }
    }
  }

  return builder.finish();
}

const screenplayPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildLineDecos(view);
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildLineDecos(update.view);
      }
    }
  },
  {
    decorations: (v) => v.decorations,
  },
);

// ── Bracket mark decorations ──────────────────────────────────────

function buildBracketDecos(view: EditorView): DecorationSet {
  const infos = view.state.field(elementTypeField);
  const builder = new RangeSetBuilder<Decoration>();

  for (let i = 1; i <= view.state.doc.lines; i++) {
    const line = view.state.doc.line(i);
    const info = infos[i - 1];
    if (!info || info.type !== ElementType.Choice) continue;

    const text = line.text;
    let bracketStart = -1;
    for (let j = 0; j < text.length; j++) {
      if (text[j] === "[") {
        bracketStart = j;
      }
      if (text[j] === "]" && bracketStart >= 0) {
        builder.add(
          line.from + bracketStart,
          line.from + j + 1,
          Decoration.mark({ class: "brink-choice-bracket" }),
        );
        bracketStart = -1;
      }
    }
  }

  return builder.finish();
}

const bracketPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildBracketDecos(view);
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildBracketDecos(update.view);
      }
    }
  },
  {
    decorations: (v) => v.decorations,
  },
);

// ── Atomic ranges for screenplay sigils ───────────────────────────
// Prevents cursor from landing inside hidden @, :<>, <> regions.

const atomicMark = Decoration.mark({});

const screenplayAtomicRanges = EditorView.atomicRanges.of((view) => {
  const infos = view.state.field(elementTypeField);
  const builder = new RangeSetBuilder<Decoration>();

  for (let i = 1; i <= view.state.doc.lines; i++) {
    const line = view.state.doc.line(i);
    const info = infos[i - 1];
    if (!info) continue;

    if (info.type === ElementType.Character) {
      const trimmed = line.text.trimStart();
      const ws = line.text.length - trimmed.length;
      // @ at start
      builder.add(line.from + ws, line.from + ws + 1, atomicMark);
      // :<> at end
      const colonGlueStart = line.to - 3;
      if (colonGlueStart > line.from + ws + 1) {
        builder.add(colonGlueStart, line.to, atomicMark);
      }
    }

    if (info.type === ElementType.Parenthetical) {
      // <> at end
      const glueStart = line.to - 2;
      if (glueStart > line.from) {
        builder.add(glueStart, line.to, atomicMark);
      }
    }
  }

  return builder.finish();
});

export function screenplayDecorations(): Extension {
  return [elementTypeField, screenplayPlugin, bracketPlugin, screenplayAtomicRanges];
}
