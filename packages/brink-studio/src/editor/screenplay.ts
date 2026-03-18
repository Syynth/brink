import { type Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { elementTypeField, elementClass, ElementType } from "./element-type.js";

const DEPTH_INDENT_EM = 2;

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

    // Replace decorations to hide extra sigils for deep choices/gathers
    if (
      (info.type === ElementType.Choice || info.type === ElementType.Gather) &&
      info.depth > 1
    ) {
      const text = line.text;
      const leadingWs = text.length - text.trimStart().length;
      let pos = leadingWs;
      let sigilCount = 0;

      while (pos < text.length && sigilCount < info.depth - 1) {
        const ch = text[pos];
        if (
          (info.type === ElementType.Choice && (ch === "*" || ch === "+")) ||
          (info.type === ElementType.Gather && ch === "-")
        ) {
          sigilCount++;
          pos++;
          // Skip trailing spaces after this sigil
          while (pos < text.length && text[pos] === " ") pos++;
        } else {
          break;
        }
      }

      // Hide from leadingWs to pos (the extra sigils)
      if (sigilCount > 0 && pos > leadingWs) {
        builder.add(line.from + leadingWs, line.from + pos, Decoration.replace({}));
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

// Separate plugin for bracket mark decorations on choice lines
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

export function screenplayDecorations(): Extension {
  return [elementTypeField, screenplayPlugin, bracketPlugin];
}
