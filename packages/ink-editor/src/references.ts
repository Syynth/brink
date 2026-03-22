import { type Extension, StateEffect, StateField, RangeSet } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, keymap } from "@codemirror/view";
import type { Location } from "@brink/wasm-types";

export interface ReferencesOptions {
  findReferences: (source: string, offset: number) => Location[];
}

const setReferenceHighlights = StateEffect.define<DecorationSet>();

const referenceHighlightField = StateField.define<DecorationSet>({
  create() {
    return RangeSet.empty;
  },
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setReferenceHighlights)) return e.value;
    }
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

const referenceHighlight = Decoration.mark({ class: "brink-reference-highlight" });

export function referencesExtension(options: ReferencesOptions): Extension {
  return [
    referenceHighlightField,
    keymap.of([
      {
        key: "Shift-Alt-f",
        run(view: EditorView): boolean {
          const pos = view.state.selection.main.head;
          const source = view.state.doc.toString();

          let refs: Location[];
          try {
            refs = options.findReferences(source, pos);
          } catch {
            return false;
          }

          if (refs.length === 0) return false;

          const decos = refs
            .map((r) => referenceHighlight.range(r.start, r.end))
            .sort((a, b) => a.from - b.from);

          view.dispatch({
            effects: setReferenceHighlights.of(Decoration.set(decos)),
          });

          // Clear highlights after 3 seconds
          setTimeout(() => {
            view.dispatch({
              effects: setReferenceHighlights.of(RangeSet.empty),
            });
          }, 3000);

          return true;
        },
      },
    ]),
  ];
}
