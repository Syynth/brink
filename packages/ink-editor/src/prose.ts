/**
 * Prose checking — the editor's half (#3209).
 *
 * This module holds **no checker**. It knows how to find the prose in a
 * brink document, how to ask something else about it, and how to put the
 * answers on screen. The engine lives behind [`ProseChecker`], in its own
 * lazily-loaded wasm module, because it is 6.5 MB gzipped — larger than the
 * entire compiler — and an embedder that only plays stories must never pay
 * for it. No checker registered means no checking, silently and correctly.
 *
 * ## Finding the prose
 *
 * Not a heuristic: the HIR projection already says. A `content` span is
 * authored prose; everything else on the line — diverts, tags, logic,
 * interpolations — is machinery, and checking it would report `barter` and
 * `act1` as misspellings. [`proseRangesOf`] takes the content spans and
 * *subtracts* the non-content spans nested inside them, so
 * `You have {gold} pieces` is checked as two ranges rather than one sentence
 * containing a variable name.
 */

import { StateEffect, type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import type { Diagnostic } from "@codemirror/lint";
import type { HirProjection, HirSpan } from "@brink/wasm-types";
import { diagnosticSources, publishDiagnostics } from "./diagnostic-sources.js";

/** A half-open range of the document, in CodeMirror positions. */
export interface ProseRange {
  from: number;
  to: number;
}

/** One finding, as `brink-prose` reports it. */
export interface ProseLint {
  /** CodeMirror positions — UTF-16, the unit the checker's boundary uses. */
  start: number;
  end: number;
  /** Harper's rule category: `Spelling`, `Repetition`, `Capitalization`, … */
  kind: string;
  message: string;
  suggestions: { kind: string; text: string }[];
}

/**
 * The seam. Implemented by the host, never by this package.
 *
 * Async by contract even if an implementation is synchronous: the only
 * implementation that matters runs a 6.5 MB wasm module that must not be on
 * the keystroke path, and a sync signature would invite exactly that.
 */
export interface ProseChecker {
  check(request: {
    text: string;
    spans: { start: number; end: number }[];
    dictionary: string[];
    dialect: string;
  }): Promise<ProseLint[]>;
}

export interface ProseOptions {
  /** The checker, or `null`/absent for no checking. */
  getChecker: () => ProseChecker | null;
  /** The document's HIR projection — the same getter the overlay uses. */
  getHirProjection: () => HirProjection;
  /** Project proper nouns. Without these, every invented name is a typo. */
  getDictionary?: () => string[];
  /** `american` | `british` | `canadian` | `australian`. */
  getDialect?: () => string;
  /** Add a word to the project's own dictionary. Absent ⇒ no such action
   *  is offered, rather than one that silently does nothing. */
  onAddToDictionary?: (word: string) => void;
  /** Debounce before checking, ms. */
  debounceMs?: number;
}

/** Force a re-check without a doc change (config change, dictionary grew). */
export const refreshProseEffect = StateEffect.define<void>();

const DEFAULT_DEBOUNCE_MS = 700;

function posOf(doc: EditorView["state"]["doc"], line: number, char: number): number | null {
  const lineNum = line + 1;
  if (lineNum < 1 || lineNum > doc.lines) return null;
  const l = doc.line(lineNum);
  const pos = l.from + char;
  return pos > l.to ? null : pos;
}

/**
 * The ranges of `doc` that hold authored prose.
 *
 * Content spans, minus every non-content span nested inside them. The
 * subtraction is the point: an interpolation is *inside* a content span, so
 * taking content spans alone would hand `{gold}` to the spell checker.
 *
 * Exported for tests — it is the part with an interesting failure mode.
 */
export function proseRangesOf(
  projection: HirProjection,
  doc: EditorView["state"]["doc"],
): ProseRange[] {
  const toRange = (s: HirSpan): ProseRange | null => {
    const from = posOf(doc, s.start_line, s.start_char);
    const to = posOf(doc, s.end_line, s.end_char);
    return from === null || to === null || to <= from ? null : { from, to };
  };

  const content: ProseRange[] = [];
  const holes: ProseRange[] = [];
  for (const span of projection.spans) {
    if (span.container) continue;
    const range = toRange(span);
    if (range === null) continue;
    if (span.kind === "content") content.push(range);
    else holes.push(range);
  }

  const ranges: ProseRange[] = [];
  for (const range of content) {
    // Walk the holes that overlap this content span, in order, emitting the
    // gaps between them.
    const inside = holes
      .filter((h) => h.to > range.from && h.from < range.to)
      .sort((a, b) => a.from - b.from);

    let cursor = range.from;
    for (const hole of inside) {
      if (hole.from > cursor) ranges.push({ from: cursor, to: Math.min(hole.from, range.to) });
      cursor = Math.max(cursor, hole.to);
      if (cursor >= range.to) break;
    }
    if (cursor < range.to) ranges.push({ from: cursor, to: range.to });
  }

  return ranges.filter((r) => r.to > r.from);
}

/**
 * Debounced prose checking, published through the shared diagnostic sources.
 *
 * A ViewPlugin so the pending timer dies in `destroy()` — the same reason
 * `diagnosticsExtension` is one. An in-flight check that lands after the
 * document moved on is dropped rather than applied at stale offsets.
 */
export function proseExtension(options: ProseOptions): Extension {
  const debounceMs = options.debounceMs ?? DEFAULT_DEBOUNCE_MS;

  return [
    // Carried WITH the producer — see diagnostic-sources.ts.
    diagnosticSources,
    ViewPlugin.fromClass(
    class {
      private timeout: ReturnType<typeof setTimeout> | null = null;
      private docGen = 0;
      private destroyed = false;

      constructor(private readonly view: EditorView) {
        this.schedule();
      }

      update(update: ViewUpdate): void {
        if (update.docChanged || update.transactions.some((t) => t.effects.some((e) => e.is(refreshProseEffect)))) {
          this.docGen += 1;
          this.schedule();
        }
      }

      destroy(): void {
        this.destroyed = true;
        if (this.timeout !== null) clearTimeout(this.timeout);
        this.timeout = null;
      }

      private schedule(): void {
        if (this.timeout !== null) clearTimeout(this.timeout);
        this.timeout = setTimeout(() => {
          this.timeout = null;
          void this.run();
        }, debounceMs);
      }

      private async run(): Promise<void> {
        if (this.destroyed) return;
        const generation = this.docGen;
        const checker = options.getChecker();
        if (checker === null) {
          // CLEAR, don't return. "No checker" is a real answer — it is what
          // `[prose] enable = false` produces — and leaving the last run's
          // squiggles on screen would make the setting look broken. This is
          // the one path where an empty publish is the point rather than a
          // side effect.
          this.publish(generation, []);
          return;
        }


        const doc = this.view.state.doc;
        const text = doc.toString();

        let ranges: ProseRange[];
        try {
          ranges = proseRangesOf(options.getHirProjection(), doc);
        } catch {
          // The projection pull can fail transiently (the session is mid-swap).
          // Leave the previous prose diagnostics standing rather than clearing
          // them — a flicker to zero and back reads as a bug.
          return;
        }
        if (ranges.length === 0) {
          this.publish(generation, []);
          return;
        }

        let lints: ProseLint[];
        try {
          lints = await checker.check({
            text,
            spans: ranges.map((r) => ({ start: r.from, end: r.to })),
            dictionary: options.getDictionary?.() ?? [],
            dialect: options.getDialect?.() ?? "american",
          });
        } catch {
          // A failed check is not an editor error. The author sees no prose
          // squiggles, which is the same as having no checker installed.
          return;
        }

        this.publish(
          generation,
          lints.map((lint) => ({
            from: Math.min(lint.start, text.length),
            to: Math.min(Math.max(lint.end, lint.start), text.length),
            severity: "info" as const,
            source: `prose:${lint.kind}`,
            message: lint.message,
            actions: [
              ...lint.suggestions
                .filter((s) => s.kind === "replace" || s.kind === "remove")
                .slice(0, MAX_SUGGESTIONS)
                .map((s) => ({
                  name: s.kind === "remove" ? "Remove" : s.text,
                  apply: (view: EditorView, from: number, to: number) => {
                    view.dispatch({ changes: { from, to, insert: s.text } });
                  },
                })),
              // Only on spellings, and only when the host can actually store
              // it. "Add to dictionary" on a repeated-word or capitalisation
              // lint would be nonsense, and offering it with nowhere to write
              // would be a control that silently does nothing.
              ...(lint.kind === "Spelling" && options.onAddToDictionary !== undefined
                ? [
                    {
                      name: "Add to dictionary",
                      apply: (view: EditorView, from: number, to: number) => {
                        options.onAddToDictionary?.(view.state.sliceDoc(from, to));
                        view.dispatch({ effects: refreshProseEffect.of() });
                      },
                    },
                  ]
                : []),
            ],
          })),
        );
      }

      private publish(generation: number, diagnostics: Diagnostic[]): void {
        // The document moved while the check was in flight, so these offsets
        // describe text that is no longer there. A newer run is already
        // scheduled.
        if (this.destroyed || generation !== this.docGen) return;
        publishDiagnostics(this.view, "prose", diagnostics);
      }
    },
    ),
  ];
}

/**
 * Quick-fixes shown per lint. Harper offers a ranked list whose tail is
 * mostly noise ("Kaelen" suggested "Keen", "Karen", "Katelyn"), and a menu
 * that long buries the one an author wants.
 */
const MAX_SUGGESTIONS = 3;
