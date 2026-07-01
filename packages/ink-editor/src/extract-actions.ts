/**
 * Extract selection → knot / function code actions (#315 H) — the visual half.
 *
 * When the editor has a multi-line selection, the code-actions menu (Ctrl-. /
 * Cmd-.) offers "Extract to knot" and "Extract to function". Choosing one opens
 * the shared {@link InlineNameInput} name prompt anchored at the selection; on
 * Enter the name + selection offsets drive the wasm extract op
 * (`extractToKnot` / `extractToFunction`), which returns a safe-by-default
 * {@link StructuralResult}:
 *
 *  - SAFE  → applied immediately through the host's apply seam.
 *  - UNSAFE → the inline breakage report expands, applying only on "Extract
 *    anyway" (the same report primitive rename uses).
 *
 *  - extract-to-knot replaces the selection with the tunnel call `-> name ->`;
 *  - extract-to-function replaces it with `{name()}` / `~ name()`.
 *
 * The extract ops have dedicated wasm entry points (they are NOT
 * `resolve_code_action` payloads), so these actions are *synthetic*: the editor
 * offers them for a multi-line selection and dispatches them here rather than
 * round-tripping through `resolveCodeAction`. A `data.action` marker
 * distinguishes them ({@link isExtractAction}); everything else resolves the
 * normal way.
 *
 * The name-prompt plugin owns the active {@link InlineNameInput} and its point
 * decoration, and tears them down in `destroy()` — an open prompt never leaks
 * when the editor unmounts.
 */

import { StateEffect, type Extension } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type { CodeAction, StructuralResult } from "@brink/wasm-types";
import { InlineNameInput } from "./inline-name-input.js";

/** `data.action` marker for the synthetic "extract to knot" action. */
export const EXTRACT_TO_KNOT_ACTION = "extract.toKnot";
/** `data.action` marker for the synthetic "extract to function" action. */
export const EXTRACT_TO_FUNCTION_ACTION = "extract.toFunction";

export type ExtractKind = "knot" | "function";

/** True when `action` is one of the synthetic extract actions (dispatched via
 *  the name-prompt flow, not `resolveCodeAction`). */
export function isExtractAction(action: CodeAction): boolean {
  return (
    action.data.action === EXTRACT_TO_KNOT_ACTION ||
    action.data.action === EXTRACT_TO_FUNCTION_ACTION
  );
}

/** The whole-line selection span the extract prompt was opened for (view coords). */
interface ExtractSelection {
  start: number;
  end: number;
}

export interface ExtractActionsOptions {
  /**
   * Compute the extract {@link StructuralResult} for the current selection
   * (view coords) + entered name. Side-effect-free — it returns the new
   * sources + breakage without applying. The host closure folds any
   * fragment-view origin into whole-file offsets and calls the matching wasm
   * op (`extractToKnot` / `extractToFunction`). A `null` return (op error /
   * missing handle) cancels the prompt.
   */
  computeExtract: (
    kind: ExtractKind,
    start: number,
    end: number,
    name: string,
  ) => StructuralResult | null;
  /**
   * Apply an already-computed extract result — the host's apply seam
   * (`applyMoveResult`): writes `new_source` + cross-file edits, refreshes
   * views, recompiles, and surfaces a toast + Undo. Called on a safe Enter or
   * an explicit "Extract anyway".
   */
  applyExtract: (kind: ExtractKind, result: StructuralResult, name: string) => void;
}

// ── Synthetic action list ───────────────────────────────────────────

/**
 * The synthetic extract actions for the current selection, or `[]` when the
 * selection is not a multi-line body selection. A single caret or a
 * within-one-line selection offers nothing (extraction operates on lines).
 */
export function extractCodeActions(state: EditorView["state"]): CodeAction[] {
  const sel = state.selection.main;
  if (sel.empty) return [];
  const startLine = state.doc.lineAt(sel.from).number;
  const endLine = state.doc.lineAt(sel.to).number;
  // Require the selection to span at least two lines (per spec: multi-line).
  if (endLine <= startLine) return [];
  return [
    {
      title: "Extract to knot",
      kind: "refactor.extract",
      data: { action: EXTRACT_TO_KNOT_ACTION },
    },
    {
      title: "Extract to function",
      kind: "refactor.extract",
      data: { action: EXTRACT_TO_FUNCTION_ACTION },
    },
  ];
}

// ── Name-prompt widget + plugin ─────────────────────────────────────

/** Effect: open the extract name prompt for `kind` over `[start, end)` (view
 *  coords). Raised by the code-actions menu when an extract action is chosen. */
export const startExtractPromptEffect = StateEffect.define<{
  kind: ExtractKind;
  selection: ExtractSelection;
}>();

/** Dispatch the extract-prompt effect on `view` (the code-actions menu route). */
export function startExtractPrompt(
  view: EditorView,
  kind: ExtractKind,
  selection: ExtractSelection,
): void {
  view.dispatch({ effects: startExtractPromptEffect.of({ kind, selection }) });
}

/** A point widget hosting the extract name prompt, anchored at the selection
 *  start. Rendered via `Decoration.widget` (side 1) so it floats after the
 *  anchor without replacing the selected text. */
class ExtractPromptWidget extends WidgetType {
  constructor(private readonly input: InlineNameInput) {
    super();
  }

  eq(): boolean {
    return false;
  }

  toDOM(): HTMLElement {
    return this.input.render();
  }

  ignoreEvent(): boolean {
    return true;
  }
}

/**
 * The extract-prompt plugin: opens an {@link InlineNameInput} on
 * `startExtractPromptEffect`, drives the safe-by-default extract, and disposes
 * the prompt on commit/cancel, on a doc edit underneath it, or on unmount.
 */
class ExtractPrompt {
  decorations: DecorationSet = Decoration.none;
  private input: InlineNameInput | null = null;

  constructor(
    private readonly view: EditorView,
    private readonly options: ExtractActionsOptions,
  ) {}

  update(update: ViewUpdate): void {
    for (const tr of update.transactions) {
      for (const effect of tr.effects) {
        if (effect.is(startExtractPromptEffect)) {
          this.start(effect.value.kind, effect.value.selection);
        }
      }
    }
    // A user edit while the prompt is open dismisses it (the anchor moved); a
    // no-op map keeps the widget aligned across viewport-only updates.
    if (this.input !== null && update.docChanged) {
      this.stop();
    } else if (update.docChanged) {
      this.decorations = this.decorations.map(update.changes);
    }
  }

  private start(kind: ExtractKind, selection: ExtractSelection): void {
    this.stop();
    const label = kind === "knot" ? "knot" : "function";
    const input = new InlineNameInput(
      {
        initialValue: "",
        placeholder: `new ${label} name…`,
        ariaLabel: `Extract to ${label}`,
        forceLabel: "Extract anyway",
        reportHead: (name, count) =>
          `Extracting to ${label} ${name} breaks ${count} ${
            count === 1 ? "reference" : "references"
          }:`,
        query: (name) =>
          this.options.computeExtract(kind, selection.start, selection.end, name),
        onCommit: (result, name) => this.options.applyExtract(kind, result, name),
      },
      () => this.stop(),
    );
    this.input = input;
    // Anchor the prompt at the selection start (side 1 → after the position).
    this.decorations = Decoration.set([
      Decoration.widget({ widget: new ExtractPromptWidget(input), side: 1 }).range(
        selection.start,
      ),
    ]);
  }

  private stop(): void {
    if (this.input === null) return;
    const view = this.view;
    this.input.dispose();
    this.input = null;
    this.decorations = Decoration.none;
    setTimeout(() => view.focus(), 0);
  }

  destroy(): void {
    this.input?.dispose();
    this.input = null;
    this.decorations = Decoration.none;
  }
}

/**
 * The extract-actions extension: the name-prompt plugin. Enabled by the studio
 * options wiring when `computeExtract` + `applyExtract` are provided. The
 * synthetic action list ({@link extractCodeActions}) is merged into the
 * code-actions menu separately (see extensions.ts).
 */
export function extractActionsExtension(options: ExtractActionsOptions): Extension {
  return [
    ViewPlugin.define((view) => new ExtractPrompt(view, options), {
      decorations: (v) => v.decorations,
    }),
  ];
}
