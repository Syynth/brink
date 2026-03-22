import { type Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import type { InlayHint } from "@brink/wasm-types";

class InlayHintWidget extends WidgetType {
  constructor(
    readonly label: string,
    readonly paddingRight: boolean,
  ) {
    super();
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "brink-inlay-hint";
    span.textContent = this.label;
    if (this.paddingRight) {
      span.style.marginRight = "4px";
    }
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
