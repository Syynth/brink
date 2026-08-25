import { docString } from "./doc-string";
import {
  type EditorState,
  type Extension,
  RangeSetBuilder,
  StateEffect,
  StateField,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import type { SemanticToken } from "@brink/wasm-types";
import { DEFER_LINE_THRESHOLD, deferredRefresh } from "./deferred-refresh.js";
import { perfTime } from "./perf/probe.js";

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
  /** Classifier-only token source for the keystroke path in large
   *  documents (#3064 micro) — no analysis pull; refined colors land on
   *  the deferred refresh. Optional: absent means always refined. */
  getSemanticTokensFast?: (source: string) => SemanticToken[];
  /** Async warm-up for the refined token pull (W2b): runs before the
   *  deferred refresh dispatches, so the field's synchronous rebuild
   *  assembles from warm slices instead of paying the pull on the
   *  dispatch stack. See `DeferredPrepare`. */
  prepareRefined?: () => Promise<unknown> | undefined;
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

const refreshHighlightEffect = StateEffect.define<void>();

export function highlightExtension(options: HighlightOptions): Extension {
  const typeNames = options.getTokenTypeNames();

  const build = (state: EditorState, fast: boolean): DecorationSet =>
    perfTime("cm.highlight.decorations", () =>
      buildHighlightDecorations(
        docString(state),
        state.doc,
        typeNames,
        fast && options.getSemanticTokensFast
          ? options.getSemanticTokensFast
          : options.getSemanticTokens,
      ),
    );

  // #3064 micro: in a LARGE document the keystroke rebuild uses the
  // CLASSIFIER token source (fragment-fresh positions and base kinds, no
  // analysis pull — the last synchronous analysis consumer leaves the
  // keystroke path); the deferred refresh swaps in resolution-refined
  // colors once the doc goes quiet. Small documents build refined
  // synchronously as before.
  const field = StateField.define<DecorationSet>({
    create(state) {
      return build(state, false);
    },
    update(value, tr) {
      if (tr.effects.some((e) => e.is(refreshHighlightEffect))) return build(tr.state, false);
      if (!tr.docChanged) return value;
      return build(tr.state, tr.newDoc.lines >= DEFER_LINE_THRESHOLD);
    },
    provide: (f) => EditorView.decorations.from(f),
  });
  return [
    field,
    deferredRefresh(
      refreshHighlightEffect,
      120,
      options.prepareRefined ? () => options.prepareRefined?.() : undefined,
    ),
  ];
}
