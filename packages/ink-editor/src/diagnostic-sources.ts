/**
 * Two producers, one diagnostic set (#3209).
 *
 * `@codemirror/lint`'s `setDiagnostics` **replaces** the whole set. That was
 * fine while the compile was the only producer, and it is a trap the moment
 * a second one exists: whichever of the compile and the prose check landed
 * last would silently erase the other's squiggles, intermittently, depending
 * on which debounce fired second. Nothing would error.
 *
 * So neither producer calls `setDiagnostics` directly any more. Each publishes
 * into its own field through [`publishDiagnostics`], which combines the fields
 * and dispatches the union. Adding a third source means adding a key here, not
 * finding this comment.
 *
 * Sort order matters for the panel and for the gutter's "worst severity wins":
 * the union is sorted by position so a prose lint on line 3 does not sit under
 * a compile error on line 40 just because the compile produced it first.
 */

import { StateEffect, StateField, type Extension } from "@codemirror/state";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import type { EditorView } from "@codemirror/view";

/** Which producer a batch came from. */
export type DiagnosticSource = "compile" | "prose";

const setSourceDiagnostics = StateEffect.define<{
  source: DiagnosticSource;
  diagnostics: readonly Diagnostic[];
}>();

type SourceMap = Readonly<Record<DiagnosticSource, readonly Diagnostic[]>>;

const EMPTY: SourceMap = { compile: [], prose: [] };

/**
 * The per-source batches.
 *
 * Held in a field rather than a plugin instance so a batch survives the
 * *other* producer's plugin being recreated — a tab switch rebuilds the
 * extension set, and a prose result that outlived its own plugin should not
 * resurrect after the compile's next publish.
 */
const sourcesField = StateField.define<SourceMap>({
  create() {
    return EMPTY;
  },
  update(value, tr) {
    let next = value;
    for (const effect of tr.effects) {
      if (effect.is(setSourceDiagnostics)) {
        next = { ...next, [effect.value.source]: effect.value.diagnostics };
      }
    }
    // Positions in a stale batch would drift under an edit. Rather than
    // remapping them (a diagnostic whose text no longer describes its span is
    // worse than a missing one), a doc change drops nothing here — both
    // producers are debounced on the same change and will republish. What it
    // must NOT do is keep publishing the old set as if it were current, which
    // is why `publishDiagnostics` is only ever called with fresh results.
    return next;
  },
});

/**
 * Publish one source's diagnostics and re-dispatch the union.
 *
 * Two dispatches on purpose: the field must be updated before the union is
 * computed, and threading `setDiagnostics`'s own spec into the same
 * transaction buys one fewer render at the cost of depending on the internal
 * shape of that spec.
 */
export function publishDiagnostics(
  view: EditorView,
  source: DiagnosticSource,
  diagnostics: readonly Diagnostic[],
): void {
  // Absent field: the caller wired a producer without the registry. Publish
  // this source alone rather than nothing — a missing registry costing the
  // OTHER source's squiggles would be the same silent erasure this module
  // exists to prevent, just arrived at differently. Both producers include
  // the registry themselves, so this is insurance, not a supported path.
  if (view.state.field(sourcesField, false) === undefined) {
    view.dispatch(setDiagnostics(view.state, [...diagnostics]));
    return;
  }

  view.dispatch({ effects: setSourceDiagnostics.of({ source, diagnostics }) });

  const sources = view.state.field(sourcesField, false) ?? EMPTY;
  const union = [...sources.compile, ...sources.prose].sort(
    (a, b) => a.from - b.from || a.to - b.to,
  );
  view.dispatch(setDiagnostics(view.state, union));
}

/**
 * The source registry.
 *
 * Both producers include this in the extension they return, so a view that
 * has a producer always has the registry — wiring them separately meant a
 * host could install one without the other and lose every diagnostic, which
 * is exactly what happened the first time this was written. CodeMirror
 * dedupes extensions by identity, so including it twice costs nothing.
 */
export const diagnosticSources: Extension = sourcesField;

/** The diagnostics currently published by one source — for tests. */
export function diagnosticsFrom(
  view: EditorView,
  source: DiagnosticSource,
): readonly Diagnostic[] {
  return (view.state.field(sourcesField, false) ?? EMPTY)[source];
}
