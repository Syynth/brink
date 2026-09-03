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

import { StateEffect, type EditorState, type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import type { Diagnostic } from "@codemirror/lint";
import type { HirProjection, HirSpan } from "@brink/wasm-types";
import { diagnosticSources, publishDiagnostics } from "./diagnostic-sources.js";
import { perfSpan } from "./perf/probe.js";
import { ElementType, elementTypeField } from "./element-type.js";
import { renderDiagnosticMessage } from "./diagnostic-anatomy.js";

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
  /**
   * The findings of the most recent check, reported to the host.
   *
   * The squiggles are published into CodeMirror directly; this is the
   * second consumer — a host that lists prose findings alongside compile
   * diagnostics (the Problems panel). Called with `[]` whenever the set
   * clears, including when checking is switched off, so a host list never
   * keeps rows the editor has already stopped showing.
   */
  onLints?: (lints: readonly ProseLint[]) => void;
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

  return subtractRanges(content, holes);
}

/**
 * `content` minus `holes` — the gaps left over, in order.
 *
 * Shared by the two subtraction passes: interpolations and machinery nested
 * inside a content span, and whole lines that are not prose at all
 * ({@link withoutCueLines}). One interval walk rather than two, because the
 * second one written independently is the one that gets the boundary
 * conditions wrong.
 */
function subtractRanges(content: ProseRange[], holes: ProseRange[]): ProseRange[] {
  const ranges: ProseRange[] = [];
  for (const range of content) {
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
 * `ranges` with every character-cue line removed.
 *
 * A cue is the speaker's NAME, not prose — the same category as the knot and
 * stitch names prose checking has always excluded. It reads as prose to the
 * HIR projection, though: an ink cue line is an ordinary content span, so
 * without this pass the cue's own text is spell-checked.
 *
 * That matters more than it looks. The dictionary seeds a cue name in TITLE
 * case (`Griswold`), because that is the spelling the prose uses and
 * matching is literal. Harper's proper-noun metadata then reports the
 * all-caps cue line itself — measured, not assumed. So excluding the cue
 * line and title-casing the seed are two halves of one fix; neither works
 * alone.
 *
 * Only `character` lines. A parenthetical (`(quietly)`) and a dialogue line
 * ARE prose and stay checked.
 */
export function withoutCueLines(ranges: ProseRange[], state: EditorState): ProseRange[] {
  const infos = state.field(elementTypeField, false);
  if (infos === undefined) return ranges;

  const holes: ProseRange[] = [];
  for (const [i, info] of infos.entries()) {
    if (info.type !== ElementType.Character) continue;
    const line = state.doc.line(i + 1);
    holes.push({ from: line.from, to: line.to });
  }
  return holes.length === 0 ? ranges : subtractRanges(ranges, holes);
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
          ranges = withoutCueLines(
            proseRangesOf(options.getHirProjection(), doc),
            this.view.state,
          );
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

        // Permanent, not a probe left behind: prose checking is the one
        // debounced consumer whose cost is NOT the compiler's, so without a
        // span of its own a stall here reads as an unattributed long task
        // (#3491 — a 651 ms freeze that the Aug 24 baseline could not see
        // because the feature landed four days after it was taken). The
        // annotation is the document length, so a run's report says what the
        // duration was paid for.
        const endCheck = perfSpan("prose.check");
        let lints: ProseLint[];
        try {
          lints = await checker.check({
            text,
            spans: ranges.map((r) => ({ start: r.from, end: r.to })),
            dictionary: options.getDictionary?.() ?? [],
            dialect: options.getDialect?.() ?? "american",
          });
          endCheck(text.length);
        } catch {
          endCheck(text.length);
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
            // The same anatomy the compiler's diagnostics use. The label
            // here is the checker's own rule name (`spelling`), which says
            // more than this lint's Info severity would — and it is the
            // same slot the compiler fills with `warning`.
            renderMessage: () => renderDiagnosticMessage(lint.kind, lint.message),
            actions: [
              ...lint.suggestions
                .filter((s) => s.kind === "replace" || s.kind === "remove")
                .slice(0, MAX_SUGGESTIONS)
                .map((s, i) => ({
                  name: s.kind === "remove" ? "Remove" : s.text,
                  // Marked rather than styled by position: the checker ranks
                  // its suggestions, and the top one is what an author takes
                  // most of the time. `:first-of-type` would have inferred
                  // that from DOM order, which is the same answer for the
                  // wrong reason and breaks the moment an action is added
                  // ahead of them.
                  markClass: i === 0 ? "cm-prose-fix cm-prose-fix-primary" : "cm-prose-fix",
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
                      // Distinct from the replacements: it changes the
                      // PROJECT rather than this line, so it should not look
                      // like a fourth spelling to pick from.
                      markClass: "cm-prose-dict",
                      apply: (view: EditorView, from: number, to: number) => {
                        options.onAddToDictionary?.(view.state.sliceDoc(from, to));
                        view.dispatch({ effects: refreshProseEffect.of() });
                      },
                    },
                  ]
                : []),
            ],
          })),
          lints,
        );
      }

      private publish(
        generation: number,
        diagnostics: Diagnostic[],
        lints: readonly ProseLint[] = [],
      ): void {
        // The document moved while the check was in flight, so these offsets
        // describe text that is no longer there. A newer run is already
        // scheduled.
        if (this.destroyed || generation !== this.docGen) return;
        publishDiagnostics(this.view, "prose", diagnostics);
        // Reported from the same guarded point as the squiggles, so the two
        // views of one result can never disagree — a host list showing rows
        // the editor has cleared is the failure this placement rules out.
        options.onLints?.(lints);
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
