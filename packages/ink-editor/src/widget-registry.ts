/**
 * Built-in argument-widget registry (argument-widget-spec.md, stage 1).
 *
 * A widget attaches to a semantic type and gives an author a richer affordance
 * than a raw literal: an inline in-text chip plus an editor opened on invoke.
 * Stage 1 ships one built-in (`color`); the registry is the seam future
 * built-ins (and, later, host widgets) plug into through the same interface.
 *
 * Inline is *always* studio-rendered (the host never mounts DOM in the source
 * line — see the spec's resolved fork 1). The editor is the only rich surface;
 * the studio supplies the popover/modal chrome and the widget fills the body,
 * resolving or cancelling through `WidgetEditorHost`.
 */

/** The studio-provided handle a widget editor resolves/cancels through. */
export interface WidgetEditorHost {
  /** The current literal value (quotes stripped), e.g. `#FF8800`. */
  readonly initial: string;
  /** Commit a new value. May be called repeatedly (live, as the user drags);
   *  the host rewrites the literal each time. */
  resolve(value: string): void;
  /** Dismiss without committing a further change. */
  cancel(): void;
}

export interface BuiltinWidget {
  /** Widget kind, e.g. `"color"`. Matches the manifest `widget.kind`. */
  readonly kind: string;
  /** Build the in-text affordance (a swatch/chip) for `value`. Studio-drawn. */
  renderInline(value: string): HTMLElement;
  /** Open the editor anchored to `anchor`. Returns a teardown that closes it. */
  openEditor(anchor: HTMLElement, host: WidgetEditorHost): () => void;
}

const registry = new Map<string, BuiltinWidget>();

/** Register a built-in widget by kind (idempotent — last registration wins). */
export function registerBuiltinWidget(widget: BuiltinWidget): void {
  registry.set(widget.kind, widget);
}

/** Look up a built-in widget by kind. */
export function getBuiltinWidget(kind: string): BuiltinWidget | undefined {
  return registry.get(kind);
}
