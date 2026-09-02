import {
  type EditorState,
  type Extension,
  RangeSetBuilder,
  StateEffect,
  StateField,
  type Transaction,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import { DEFER_LINE_THRESHOLD, deferredRefresh } from "./deferred-refresh.js";
import type { InlayHint } from "@brink/wasm-types";
import { ensureStructuralStyles } from "./structural-styles.js";
import { perfTime } from "./perf/probe.js";

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
  /** Async warm-up for the hints pull over `[start, end)` (W2b): runs
   *  before the deferred refresh dispatches so the field's synchronous
   *  rebuild hits the warmed memo. See `DeferredPrepare`. */
  prepareHints?: (start: number, end: number) => Promise<unknown> | undefined;
}

const refreshInlayHintsEffect = StateEffect.define<void>();

/**
 * Live on/off switch (#3350, Settings ▸ Editor "Show inlay hints"): a
 * StateField + effect, the same shape `argument-widgets.ts` uses for
 * `formGlyph`/`autoOpen` — the extension stays mounted either way, so no
 * compartment reconfiguration is needed, and a view whose baseline predates
 * this field simply ignores the effect (matches `setEditorActionKeys`'s
 * no-op-on-a-stale-view contract). Default true: hidden state is opt-in.
 */
const setInlayHintsEnabledEffect = StateEffect.define<boolean>();
const inlayHintsEnabledField = StateField.define<boolean>({
  create: () => true,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setInlayHintsEnabledEffect)) return e.value;
    return value;
  },
});

/** Switch a view's inlay hints on/off live (the Settings toggle dispatches
 *  this) — broadcast across every open editor by `DocumentSessions.setInlayHints`. */
export function setInlayHints(view: EditorView, on: boolean): void {
  view.dispatch({ effects: setInlayHintsEnabledEffect.of(on) });
}

export function inlayHintsExtension(options: InlayHintsOptions): Extension {
  const build = (state: EditorState): DecorationSet =>
    state.field(inlayHintsEnabledField)
      ? perfTime("cm.inlayHints.decorations", () => buildInlayDecorations(state, options))
      : Decoration.none;
  // #3064 C2 adaptive deferral: hints are advisory paint — in a LARGE
  // document a doc change maps the existing widgets through the edit and
  // the content refreshes once the burst ends; small documents rebuild
  // synchronously as before.
  const field = StateField.define<DecorationSet>({
    create(state) {
      return build(state);
    },
    update(value, tr: Transaction) {
      if (tr.effects.some((e) => e.is(refreshInlayHintsEffect) || e.is(setInlayHintsEnabledEffect))) {
        return build(tr.state);
      }
      if (!tr.docChanged) return value;
      if (tr.newDoc.lines >= DEFER_LINE_THRESHOLD) return value.map(tr.changes);
      return build(tr.state);
    },
    provide: (f) => EditorView.decorations.from(f),
  });
  return [
    inlayHintsEnabledField,
    field,
    deferredRefresh(
      refreshInlayHintsEffect,
      120,
      options.prepareHints
        ? (view) => options.prepareHints?.(0, view.state.doc.length)
        : undefined,
    ),
  ];
}

function buildInlayDecorations(state: EditorState, options: InlayHintsOptions): DecorationSet {
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
}
