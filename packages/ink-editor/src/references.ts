import { type Extension, StateEffect, StateField, RangeSet } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  keymap,
} from "@codemirror/view";
import type { Location } from "@brink/wasm-types";

export interface ReferencesOptions {
  /** Sync or async (#3110 — the studio wiring rides the worker road). */
  findReferences: (source: string, offset: number) => Location[] | Promise<Location[]>;
  /** Resolve the symbol's declaration (the same callback the cmd-click
   *  navigate surface uses). When provided, `onShowReferences` receives it
   *  as the third argument so the host can anchor a references refresh at
   *  the declaration's position (docs/search-results-cards-spec.md). */
  gotoDefinition?: (source: string, offset: number) => Location | null | Promise<Location | null>;
  /** Route results to the host's references surface (the Search panel —
   *  context-menu spec ruling). When absent, fall back to the in-view
   *  3s highlight (same-file only). */
  onShowReferences?: (
    symbol: string,
    locations: Location[],
    declaration?: Location | null,
  ) => void;
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

/**
 * Owns the auto-clear timer for reference highlights. When a `setReferenceHighlights`
 * effect sets a non-empty highlight set, schedule a 3s clear (cancelling any prior).
 * `destroy()` cancels the pending timer so it can't dispatch into a detached or
 * replaced view.
 */
const referenceClearTimer = ViewPlugin.fromClass(
  class {
    private timeout: ReturnType<typeof setTimeout> | null = null;

    constructor(private readonly view: EditorView) {}

    update(update: ViewUpdate): void {
      for (const tr of update.transactions) {
        for (const e of tr.effects) {
          if (!e.is(setReferenceHighlights)) continue;
          this.cancel();
          // Only schedule a clear when highlights were actually set (size > 0),
          // not for the clear effect itself.
          if (e.value.size > 0) {
            this.timeout = setTimeout(() => {
              this.timeout = null;
              if (this.view.dom.isConnected) {
                this.view.dispatch({ effects: setReferenceHighlights.of(RangeSet.empty) });
              }
            }, 3000);
          }
        }
      }
    }

    destroy(): void {
      this.cancel();
    }

    private cancel(): void {
      if (this.timeout !== null) {
        clearTimeout(this.timeout);
        this.timeout = null;
      }
    }
  },
);

/** Highlight every reference of the symbol at `pos` (3s auto-clear via the
 *  timer plugin). Shared by the Shift-Alt-F binding and the context menu's
 *  Find References item. */
export async function showReferencesAt(
  view: EditorView,
  pos: number,
  findReferences: ReferencesOptions["findReferences"],
): Promise<boolean> {
  const doc = view.state.doc;
  const source = doc.toString();

  let refs: Location[];
  try {
    refs = await findReferences(source, pos);
  } catch {
    return false;
  }
  if (!view.dom.isConnected || view.state.doc !== doc) return false; // stale landing

  if (refs.length === 0) return false;

  const decos = refs
    .map((r) => referenceHighlight.range(r.start, r.end))
    .sort((a, b) => a.from - b.from);

  // The referenceClearTimer plugin schedules the auto-clear.
  view.dispatch({
    effects: setReferenceHighlights.of(Decoration.set(decos)),
  });

  return true;
}

/** Find references at `pos` and present them: through the host's surface
 *  when wired (the Search panel), else the in-view highlight fallback. */
export async function findReferencesAt(
  view: EditorView,
  pos: number,
  options: ReferencesOptions,
): Promise<boolean> {
  if (!options.onShowReferences) {
    return showReferencesAt(view, pos, options.findReferences);
  }
  const doc = view.state.doc;
  const source = doc.toString();
  let refs: Location[];
  try {
    refs = await options.findReferences(source, pos);
  } catch {
    return false;
  }
  if (refs.length === 0) return false;
  if (!view.dom.isConnected || view.state.doc !== doc) return false; // stale landing
  const word = view.state.wordAt(pos);
  const symbol = word ? view.state.sliceDoc(word.from, word.to) : "";
  let declaration: Location | null = null;
  try {
    declaration = (await options.gotoDefinition?.(source, pos)) ?? null;
  } catch {
    declaration = null;
  }
  options.onShowReferences(symbol, refs, declaration);
  return true;
}

export function referencesExtension(options: ReferencesOptions): Extension {
  return [
    referenceHighlightField,
    referenceClearTimer,
    keymap.of([
      {
        key: "Shift-Alt-f",
        run: (view: EditorView) => {
          // Async resolution (#3110): claim the key; an empty result just
          // shows nothing (Shift-Alt-F has no other binding to fall to).
          void findReferencesAt(view, view.state.selection.main.head, options);
          return true;
        },
      },
    ]),
  ];
}
