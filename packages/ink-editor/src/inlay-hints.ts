import { type Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import type { InlayHint } from "@brink/wasm-types";
import { ensureStructuralStyles } from "./structural-styles.js";

class InlayHintWidget extends WidgetType {
  constructor(
    readonly label: string,
    readonly paddingRight: boolean,
  ) {
    super();
  }

  toDOM(): HTMLElement {
    ensureStructuralStyles();
    const span = document.createElement("span");
    // `-pad` carries the hint's requested trailing gap as a class (not an
    // inline style) so hosts can restyle it (#363).
    span.className = this.paddingRight ? "brink-inlay-hint brink-inlay-hint-pad" : "brink-inlay-hint";
    span.textContent = this.label;
    return span;
  }

  eq(other: InlayHintWidget): boolean {
    return this.label === other.label && this.paddingRight === other.paddingRight;
  }
}

export interface InlayHintsOptions {
  getInlayHints: (source: string, start: number, end: number) => InlayHint[];
}

export function inlayHintsExtension(options: InlayHintsOptions): Extension {
  return EditorView.decorations.compute(["doc"], (state) => {
    const source = state.doc.toString();
    const builder = new RangeSetBuilder<Decoration>();

    let hints: InlayHint[];
    try {
      hints = options.getInlayHints(source, 0, source.length);
    } catch {
      return builder.finish();
    }

    // Sort by offset for RangeSetBuilder
    hints.sort((a, b) => a.offset - b.offset);

    for (const hint of hints) {
      if (hint.offset < 0 || hint.offset > source.length) continue;
      // Value-list labels (#174) are rendered by the argument-widgets extension
      // as an interactive picker chip instead of this passive hint (#224); the
      // LSP, a separate consumer, still gets them.
      if (hint.kind === "value") continue;
      const widget = new InlayHintWidget(hint.label, hint.padding_right);
      builder.add(
        hint.offset,
        hint.offset,
        Decoration.widget({ widget, side: 1 }),
      );
    }

    return builder.finish();
  });
}
