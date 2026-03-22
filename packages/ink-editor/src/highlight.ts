import { type Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import type { SemanticToken } from "@brink/wasm-types";

const decoCache = new Map<string, Decoration>();

function getDecoForType(typeName: string): Decoration {
  let deco = decoCache.get(typeName);
  if (!deco) {
    deco = Decoration.mark({ class: "tok-" + typeName });
    decoCache.set(typeName, deco);
  }
  return deco;
}

export interface HighlightOptions {
  getSemanticTokens: (source: string) => SemanticToken[];
  getTokenTypeNames: () => string[];
}

function buildHighlightDecorations(
  source: string,
  doc: EditorView["state"]["doc"],
  typeNames: string[],
  getSemanticTokens: (source: string) => SemanticToken[],
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();

  let tokens: SemanticToken[];
  try {
    tokens = getSemanticTokens(source);
  } catch {
    return builder.finish();
  }

  if (!tokens || tokens.length === 0) {
    return builder.finish();
  }

  // Collect and sort (RangeSetBuilder requires sorted input)
  const decos: { from: number; to: number; deco: Decoration }[] = [];
  for (const t of tokens) {
    const typeName = typeNames[t.token_type];
    if (!typeName) continue;

    const lineNum = t.line + 1; // 0-based to 1-based
    if (lineNum < 1 || lineNum > doc.lines) continue;

    const line = doc.line(lineNum);
    const from = line.from + t.start_char;
    const to = from + t.length;

    if (from < line.from || to > line.to) continue;

    decos.push({ from, to, deco: getDecoForType(typeName) });
  }

  decos.sort((a, b) => a.from - b.from || a.to - b.to);

  for (const { from, to, deco } of decos) {
    builder.add(from, to, deco);
  }

  return builder.finish();
}

export function highlightExtension(options: HighlightOptions): Extension {
  const typeNames = options.getTokenTypeNames();

  return EditorView.decorations.compute(["doc"], (state) => {
    return buildHighlightDecorations(
      state.doc.toString(),
      state.doc,
      typeNames,
      options.getSemanticTokens,
    );
  });
}
