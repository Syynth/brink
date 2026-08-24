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

import { StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap, WidgetType } from "@codemirror/view";
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
  prepareRename: (source: string, offset: number) => Location | null;
  /**
   * Live (debounced) rename query: returns the safe-rename result for renaming
   * the symbol at `offset` (view coords) to `newName`. Side-effect-free — it
   * computes the new sources + breakage report without applying anything.
   */
  renameSymbolAt: (offset: number, newName: string) => StructuralResult;
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
export const startInlineRenameEffect = StateEffect.define<{ offset: number | null }>();

/** Dispatch the inline-rename start effect on `view` (used by the
 *  context-menu route, which has a view but enters via a command). */
export function startInlineRename(view: EditorView, offset?: number): void {
  view.dispatch({ effects: startInlineRenameEffect.of({ offset: offset ?? null }) });
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
    // Same highlight color as the token being renamed: copy its semantic
    // token classes onto the input.
    const domAt = view.domAtPos(Math.min(this.active.from + 1, this.active.to));
    const tokEl =
      domAt.node instanceof Element
        ? domAt.node
        : (domAt.node.parentElement ?? null);
    const tokHost = tokEl?.closest('[class*="tok-"]');
    const input = inner.querySelector("input");
    if (tokHost && input) {
      for (const cls of tokHost.classList) {
        if (cls.startsWith("tok-")) input.classList.add(cls);
      }
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
          const pos = effect.value.offset ?? tr.state.selection.main.head;
          const source = tr.state.doc.toString();
          let range: Location | null;
          try {
            range = options.prepareRename(source, pos);
          } catch {
            return null;
          }
          if (range === null) return value;
          return {
            from: range.start,
            to: range.end,
            name: source.slice(range.start, range.end),
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
        // Only intercept F2 when the cursor is on a renameable symbol — else the
        // key falls through (matching the old prepareRename gate).
        const pos = view.state.selection.main.head;
        const source = view.state.doc.toString();
        let range: Location | null;
        try {
          range = options.prepareRename(source, pos);
        } catch {
          return false;
        }
        if (range === null) return false;
        startInlineRename(view, pos);
        return true;
      },
    },
  ]);

  return [field, keys];
}
