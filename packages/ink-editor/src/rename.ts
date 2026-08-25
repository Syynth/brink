/**
 * Inline symbol rename (#323 / #324) — fully in the editor.
 *
 * Replaces the modal for editor renames. On F2 (or the context-menu "Rename…",
 * which dispatches `startInlineRenameEffect`) an inline CM6 widget — an
 * argument-widget-family chip — is anchored at the symbol's range, hosting a
 * styled name input. As the user types (debounced ~250ms) we call
 * `renameSymbolAt(offset, newName)` and surface the introduced-diagnostic count
 * in a "⚠ breaks N" badge to the right of the input; the badge is hidden when
 * the rename is safe (N = 0). The badge expands to an INLINE breakage report
 * beneath the input — the affected-reference list plus [Cancel] / [Rename
 * anyway] — never a modal. A safe rename commits on Enter with no popover.
 *
 * The plugin owns the widget's listeners, timers, and DOM and tears them all
 * down in `destroy()` (the code-actions.ts CodeActionsMenu teardown pattern) —
 * an open inline-rename must not leak when the editor unmounts.
 *
 * A host can override the breakage rendering via `onBreakage` (return `true` to
 * suppress the default inline report); the default is this inline report.
 */

import { Facet, StateEffect, StateField, type Extension } from "@codemirror/state";
import { closeHoverTooltips, Decoration, EditorView, keymap, WidgetType } from "@codemirror/view";
import type { Location, StructuralResult } from "@brink/wasm-types";
import { InlineNameInput } from "./inline-name-input.js";
import {
  isSafeRename,
  breakageCount,
  breakageEntries,
  type BreakageEntry,
} from "./breakage.js";

// Re-export the pure breakage logic from its shared home (#315 H factored it out
// of rename.ts so extract can reuse it) — the public API surface is unchanged.
export { isSafeRename, breakageCount, breakageEntries };
export type { BreakageEntry };

/** Context handed to a host `onBreakage` override. */
export interface BreakageContext {
  /** The file the rename was initiated in. */
  path: string;
  /** Whole-file UTF-16 offset of the renamed symbol. */
  offset: number;
  /** The proposed new name. */
  newName: string;
  /** The symbol's original name. */
  currentName: string;
}

export interface RenameOptions {
  /** Returns the renameable range at `offset` (view coords), or null. */
  /** Sync or async (#3110 — the studio wiring rides the worker road). */
  prepareRename: (source: string, offset: number) => Location | null | Promise<Location | null>;
  /**
   * Live (debounced) rename query: returns the safe-rename result for renaming
   * the symbol at `offset` (view coords) to `newName`. Side-effect-free — it
   * computes the new sources + breakage report without applying anything.
   */
  /** Sync or async (#3110): the live-badge query lands through
   *  InlineNameInput's existing pending machinery. */
  renameSymbolAt: (
    offset: number,
    newName: string,
  ) => StructuralResult | Promise<StructuralResult>;
  /**
   * Commit a rename: apply the (already-computed) `result` edits across files.
   * Called for a safe rename on Enter, or on an explicit "Rename anyway".
   * `currentName` is the symbol's original name (so the host can re-key tabs).
   */
  commitRename: (result: StructuralResult, newName: string, currentName: string) => void;
  /**
   * Optional host override for the breakage surface. Called when an unsafe
   * result is produced; return `true` to suppress the default inline report
   * (the host renders its own). The default (`false`/undefined) shows the
   * inline report.
   */
  onBreakage?: (result: StructuralResult, ctx: BreakageContext) => boolean;
}

// ── Query cache ─────────────────────────────────────────────────────

/** A cache of rename queries keyed by `(path, offset, newName)` (#324) — so a
 *  debounced keystroke that re-queries an unchanged triple costs no wasm call. */
export class RenameQueryCache {
  private readonly map = new Map<string, StructuralResult>();

  // #2558: was `${path}\x00${offset}\x00${newName}` (literal NUL bytes as
  // separators) -- that made this file register as "binary" to `grep`/`rg`
  // without `-a`/`--text`, silently hiding every match in it (including this
  // method's own lines) from any repo-wide sweep. JSON-encoding the triple
  // keeps the file plain, greppable UTF-8 text, and is provably
  // collision-free where a printable separator (e.g. `|` or the unit-
  // separator glyph) would not be: JSON.stringify of a fixed 3-element array
  // is an injective encoding -- JSON.parse recovers the exact three original
  // values, so two distinct (path, offset, newName) triples can never
  // serialize to the same string, regardless of what characters `path`/
  // `newName` contain (JSON escapes them, including any embedded NUL).
  private key(path: string, offset: number, newName: string): string {
    return JSON.stringify([path, offset, newName]);
  }

  get(path: string, offset: number, newName: string): StructuralResult | undefined {
    return this.map.get(this.key(path, offset, newName));
  }

  set(path: string, offset: number, newName: string, result: StructuralResult): void {
    this.map.set(this.key(path, offset, newName), result);
  }

  clear(): void {
    this.map.clear();
  }
}

// ── Inline widget + plugin ──────────────────────────────────────────

/** Effect: start an inline rename at `offset` (view coords). When `offset` is
 *  omitted the cursor position is used. Raised by F2 and by the editor
 *  context-menu "Rename…" path. */
export const startInlineRenameEffect = StateEffect.define<{
  offset: number | null;
  /** The rename target, PRE-RESOLVED by {@link startInlineRename} (#3110:
   *  resolution rides the worker road, and a StateField cannot await —
   *  so the effect carries the answer instead of the question). */
  range: Location;
  /** The target token's color-bearing classes (tok-*, brink-hir-*), captured
   *  by startInlineRename BEFORE the rename-target mark is applied — once it
   *  is, CM rebuilds the spans and the original classes are gone from the
   *  DOM (measured: an active rename's token span holds ONLY
   *  brink-rename-target). */
  tokenClasses: readonly string[];
}>();

/** The prepare-rename resolver, provided by {@link renameExtension} so
 *  {@link startInlineRename} (a module-level entry with no options access)
 *  can resolve the target before dispatching (#3110). */
const renameResolverFacet = Facet.define<
  (source: string, offset: number) => Location | null | Promise<Location | null>,
  ((source: string, offset: number) => Location | null | Promise<Location | null>) | null
>({ combine: (values) => values[0] ?? null });

/**
 * Resolve the rename target (worker road, #3110) and dispatch the inline
 * rename row on landing — under the usual guards (doc held still, view
 * alive). Resolves `true` when the row opened; `false` lets callers fall
 * back (e.g. the modal prompt).
 */
export async function startInlineRename(view: EditorView, offset?: number): Promise<boolean> {
  const pos = offset ?? view.state.selection.main.head;
  const resolver = view.state.facet(renameResolverFacet);
  if (resolver === null) return false;
  const doc = view.state.doc;
  let range: Location | null;
  try {
    range = await resolver(doc.toString(), pos);
  } catch {
    return false;
  }
  if (range === null) return false;
  if (!view.dom.isConnected || view.state.doc !== doc) return false; // stale landing
  dispatchInlineRename(view, pos, offset ?? null, range);
  return true;
}

/** The (synchronous) dispatch half: capture token classes, raise the effect. */
function dispatchInlineRename(
  view: EditorView,
  pos: number,
  offset: number | null,
  range: Location,
): void {
  // Capture the token's highlight classes NOW — the DOM is still un-marked.
  const tokenClasses: string[] = [];
  const domAt = view.domAtPos(Math.min(pos + 1, view.state.doc.length));
  let el: Element | null =
    domAt.node instanceof Element ? domAt.node : (domAt.node.parentElement ?? null);
  while (el && !el.classList.contains("cm-line")) {
    for (const cls of el.classList) {
      if (cls.startsWith("tok-") || cls.startsWith("brink-hir-")) tokenClasses.push(cls);
    }
    el = el.parentElement;
  }
  view.dispatch({
    effects: [
      // Dismiss any open hover card up front — the hover source suppresses
      // NEW cards while the rename row is open (hover.ts), but an
      // already-open card persists until closed and would sit on the badge.
      closeHoverTooltips,
      startInlineRenameEffect.of({ offset, range, tokenClasses }),
    ],
  });
}

/**
 * The inline rename widget — a chip hosting the name input, the "⚠ breaks N"
 * badge, and (when expanded) the inline breakage report. One instance lives at
 * a time, owned by the `InlineRename` plugin which tears it down cleanly.
 * Rendering + behavior are the shared {@link InlineNameInput} primitive (#315 H
 * factored it out so extract-to-knot/function reuses the same chip + report).
 */
/** Mark over the symbol being renamed — the token STAYS in the document
 *  (the old design replaced it with a widget, which is how Escape could
 *  appear to eat the text); the input floats beneath in a tooltip. */
const renameTargetMark = Decoration.mark({ class: "brink-rename-target" });

const stopInlineRenameEffect = StateEffect.define<null>();

/** The active rename target, resolved at start-effect time. */
interface ActiveRename {
  from: number;
  to: number;
  name: string;
  /** Color-bearing classes captured before the mark was applied. */
  tokenClasses: readonly string[];
}

/** A live inline-rename session — a thin adapter binding the shared
 *  {@link InlineNameInput} to the rename options (live debounced badge, safe
 *  commit on Enter). Owns nothing itself beyond the inner input, which it tears
 *  down in `dispose()`. */
class RenameController {
  private readonly inner: InlineNameInput;

  constructor(
    options: RenameOptions,
    /** View-coord UTF-16 offset of the symbol's start. Passed verbatim to
     *  `renameSymbolAt`, whose host closure folds in any fragment-view origin
     *  to produce the whole-file offset. */
    symbolOffset: number,
    path: string,
    currentName: string,
    onClose: () => void,
  ) {
    this.inner = new InlineNameInput(
      {
        initialValue: currentName,
        ariaLabel: `Rename ${currentName}`,
        forceLabel: "Rename anyway",
        liveBadge: true,
        reportHead: (newName, count) =>
          `Renaming ${currentName} → ${newName} breaks ${count} ${
            count === 1 ? "reference" : "references"
          }:`,
        query: (newName) => options.renameSymbolAt(symbolOffset, newName),
        onCommit: (result, newName) => options.commitRename(result, newName, currentName),
        onBreakage: options.onBreakage
          ? (result, ctx) =>
              options.onBreakage?.(result, {
                path,
                offset: symbolOffset,
                newName: ctx.name,
                currentName,
              }) ?? false
          : undefined,
      },
      onClose,
    );
  }

  render(): HTMLElement {
    return this.inner.render();
  }

  dispose(): void {
    this.inner.dispose();
  }
}

/** The rename editor row — a BLOCK widget inserted after the target's line
 *  (a widget row gets no gutter number, so it reads as an inserted blank
 *  line). A hidden spacer carrying the line's actual prefix text aligns the
 *  input exactly under the token — byte-for-byte, so tabs and wide glyphs
 *  align too, with no inline styles. The input carries the target token's
 *  own `tok-*` classes, so it renders in the same font and highlight color
 *  as what it renames. */
class RenameRowWidget extends WidgetType {
  private controller: RenameController | null = null;

  constructor(
    private readonly active: ActiveRename,
    private readonly options: RenameOptions,
  ) {
    super();
  }

  override eq(other: RenameRowWidget): boolean {
    return (
      this.active.from === other.active.from &&
      this.active.to === other.active.to &&
      this.active.name === other.active.name
    );
  }

  override toDOM(view: EditorView): HTMLElement {
    const row = document.createElement("div");
    row.className = "brink-rename-row";

    // Exact column alignment: the real prefix text, hidden but occupying
    // its true width.
    const line = view.state.doc.lineAt(this.active.from);
    const spacer = document.createElement("span");
    spacer.className = "brink-rename-spacer";
    spacer.textContent = view.state.sliceDoc(line.from, this.active.from);
    spacer.setAttribute("aria-hidden", "true");
    row.appendChild(spacer);

    this.controller = new RenameController(
      this.options,
      this.active.from,
      "",
      this.active.name,
      () => {
        view.dispatch({ effects: stopInlineRenameEffect.of(null) });
        setTimeout(() => {
          if (view.dom.isConnected) view.focus();
        }, 0);
      },
    );
    const inner = this.controller.render();
    // Same highlight color as the token being renamed — classes captured by
    // startInlineRename before the target mark rebuilt the spans.
    const input = inner.querySelector("input");
    if (input) {
      for (const cls of this.active.tokenClasses) input.classList.add(cls);
    }
    row.appendChild(inner);
    return row;
  }

  override destroy(): void {
    this.controller?.dispose();
    this.controller = null;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

export function renameExtension(options: RenameOptions): Extension {
  // Zed-style inserted rename row (ruled 2026-08-24): a StateField holds the
  // resolved target; it provides a mark over the token (which stays put —
  // Escape can never disturb document text) and a block widget row beneath
  // the line holding the name input.
  const field = StateField.define<ActiveRename | null>({
    create: () => null,
    update(value, tr) {
      for (const effect of tr.effects) {
        if (effect.is(startInlineRenameEffect)) {
          // The range arrives PRE-RESOLVED (#3110) — see the effect's doc.
          const range = effect.value.range;
          const source = tr.state.doc.toString();
          return {
            from: range.start,
            to: range.end,
            name: source.slice(range.start, range.end),
            tokenClasses: effect.value.tokenClasses,
          };
        }
        if (effect.is(stopInlineRenameEffect)) return null;
      }
      // A user edit while the rename is open dismisses it (its anchor moved).
      if (value !== null && tr.docChanged) return null;
      // Moving the editor cursor off the target's line dismisses WITHOUT
      // committing (ruled 2026-08-24) — clicking elsewhere is a cancel
      // gesture. Clicks inside the rename row never get here (the widget
      // ignores events), and typing in the input moves no editor selection.
      if (value !== null && tr.selection) {
        const targetLine = tr.state.doc.lineAt(value.from).number;
        const headLine = tr.state.doc.lineAt(tr.state.selection.main.head).number;
        if (headLine !== targetLine) return null;
      }
      return value;
    },
    provide: (f) => [
      EditorView.decorations.compute([f], (state) => {
        const active = state.field(f);
        if (!active) return Decoration.none;
        const line = state.doc.lineAt(active.from);
        return Decoration.set([
          renameTargetMark.range(active.from, active.to),
          Decoration.widget({
            widget: new RenameRowWidget(active, options),
            block: true,
            side: 1,
          }).range(line.to),
        ]);
      }),
    ],
  });

  const keys = keymap.of([
    {
      key: "F2",
      run(view: EditorView): boolean {
        // The renameable gate resolves on the worker road (#3110): claim
        // the key now and open the row on landing — F2 on a non-symbol
        // simply does nothing (it has no other binding to fall through
        // to, so the optimistic claim costs nothing observable).
        void startInlineRename(view, view.state.selection.main.head);
        return true;
      },
    },
  ]);

  return [
    field,
    keys,
    renameResolverFacet.of((source, offset) => options.prepareRename(source, offset)),
  ];
}
