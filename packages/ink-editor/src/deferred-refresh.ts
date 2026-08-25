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
 * An async warm-up run before a deferred refresh dispatches (W2b of
 * `docs/editor-worker-spec.md`): the expensive pull rides the session
 * facade off the interactive path, and the refresh effect — whose field
 * rebuild then hits the warmed memo/cache — fires only once it settles.
 * Returning `undefined` (e.g. no live handle) skips straight to the
 * dispatch, as does an absent `prepare` entirely (the pre-W2b behavior).
 */
export type DeferredPrepare = (view: EditorView) => Promise<unknown> | undefined;

/**
 * Dispatch `effect` once the document has been quiet for `delayMs` after a
 * change in a large document (#3064 C2). Small documents never schedule —
 * their fields rebuilt synchronously and a refresh would be a no-op tax.
 * The timer resets on every further edit, so a typing burst pays one
 * rebuild at its end, not one per keystroke.
 *
 * With a `prepare` (W2b), the quiet-fire first awaits it, then dispatches
 * under landing guards: a doc that moved during the prepare skips the
 * dispatch (that change re-armed the timer, so a fresh fire follows), and
 * a destroyed plugin never dispatches. A *rejected* prepare still
 * dispatches — rejection means the query was superseded by a sibling
 * view's identical pull (whose execution warmed the same memo) or failed
 * outright, and in both cases the field's own synchronous pull is the
 * correct fallback; skipping would strand this view on stale content.
 */
export function deferredRefresh(
  effect: StateEffectType<void>,
  delayMs = 120,
  prepare?: DeferredPrepare,
): Extension {
  return ViewPlugin.fromClass(
    class {
      private timer: ReturnType<typeof setTimeout> | null = null;
      private destroyed = false;

      constructor(private readonly view: EditorView) {}

      update(u: ViewUpdate): void {
        if (!u.docChanged) return;
        if (u.state.doc.lines < DEFER_LINE_THRESHOLD) return;
        if (this.timer !== null) clearTimeout(this.timer);
        this.timer = setTimeout(() => {
          this.timer = null;
          this.fire();
        }, delayMs);
      }

      private fire(): void {
        const pending = prepare?.(this.view);
        if (pending === undefined) {
          this.view.dispatch({ effects: effect.of(undefined) });
          return;
        }
        const doc = this.view.state.doc;
        void pending.catch(() => undefined).then(() => {
          if (this.destroyed || this.view.state.doc !== doc) return;
          this.view.dispatch({ effects: effect.of(undefined) });
        });
      }

      destroy(): void {
        this.destroyed = true;
        if (this.timer !== null) clearTimeout(this.timer);
      }
    },
  );
}
