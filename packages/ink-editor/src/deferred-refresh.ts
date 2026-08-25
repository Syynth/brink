import { type Extension, type StateEffectType } from "@codemirror/state";
import { type EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";

/**
 * Adaptive deferral threshold (#3064 C2): documents under this many lines
 * rebuild their decoration state synchronously in the transaction (the
 * pre-C2 behavior — imperceptible cost, no staleness window); documents at
 * or above it map positions through the change synchronously and rebuild
 * CONTENT on the debounced refresh below. Staleness is bought only where
 * it pays for itself.
 */
export const DEFER_LINE_THRESHOLD = 1000;

/**
 * Dispatch `effect` once the document has been quiet for `delayMs` after a
 * change in a large document (#3064 C2). Small documents never schedule —
 * their fields rebuilt synchronously and a refresh would be a no-op tax.
 * The timer resets on every further edit, so a typing burst pays one
 * rebuild at its end, not one per keystroke.
 */
export function deferredRefresh(effect: StateEffectType<void>, delayMs = 120): Extension {
  return ViewPlugin.fromClass(
    class {
      private timer: ReturnType<typeof setTimeout> | null = null;

      constructor(private readonly view: EditorView) {}

      update(u: ViewUpdate): void {
        if (!u.docChanged) return;
        if (u.state.doc.lines < DEFER_LINE_THRESHOLD) return;
        if (this.timer !== null) clearTimeout(this.timer);
        this.timer = setTimeout(() => {
          this.timer = null;
          this.view.dispatch({ effects: effect.of(undefined) });
        }, delayMs);
      }

      destroy(): void {
        if (this.timer !== null) clearTimeout(this.timer);
      }
    },
  );
}
