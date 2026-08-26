/**
 * Detached gutters (issue #3119) — the fix for WebKit's editor-layout
 * pathology.
 *
 * ## The problem
 *
 * CodeMirror's `.cm-gutters` is a `position: sticky` flex child of
 * `.cm-scroller`, stretched via `min-height` to the FULL document height
 * (27,974 px on the reporting project's largest file). In WebKit that
 * structure makes every layout of the editor ~5x more expensive: a
 * forced layout costs ~35 ms with the gutters present and ~7 ms without,
 * and felt keystroke latency is 48 ms vs 24 ms. It is paid synchronously
 * on every keystroke (CodeMirror's selection sync forces a layout read)
 * and once per frame while scrolling.
 *
 * What it is NOT — each ruled out by measurement, not argument:
 *
 * - **Not element count.** Hiding 48 of ~100 gutter elements in place
 *   changes nothing; one gutter column costs the same as four. Only
 *   removing the container helps.
 * - **Not paint or compositing.** `will-change: transform`,
 *   `transform: translateZ(0)` and `contain: paint` each move the number
 *   by zero.
 * - **Not the markup by itself.** A synthetic page reproducing the same
 *   DOM (sticky flex container, 28,000 px tall, up to 200 children with
 *   inline heights, wrapped text beside it) lays out in ~0 ms in BOTH
 *   engines. The pathology needs the real contenteditable content.
 *
 * Chromium is unaffected throughout, which is why it never appeared in
 * the browser playground.
 *
 * ## The fix
 *
 * Take the gutters out of the scroller's flex/sticky participation
 * (absolute, auto height) and pay back the horizontal space they used to
 * occupy as `padding-left` on the content, so text starts exactly where
 * it always did.
 *
 * This is sound rather than a hack: gutters are sticky in CodeMirror so
 * they survive HORIZONTAL scrolling, and this extension engages only
 * when line wrapping is on — where the content never scrolls
 * horizontally, so the dropped guarantee is one the view cannot use. A
 * non-wrapping view keeps CodeMirror's stock layout untouched.
 *
 * Vertical alignment needs no help: CodeMirror positions gutter elements
 * with margin offsets measured from the document top, which stay correct
 * once the container is detached — verified at scroll depths 0 / 6,000 /
 * 14,000 / 24,000 px, where the line-to-marker delta is constant and
 * identical to stock. Note that this alignment check CANNOT catch a
 * container whose own box is mis-sized (both the line and the marker
 * move together), which is why the `bottom: auto` note below matters and
 * why the real guard is the play-gutter click e2e.
 *
 * Measured on the same project and harness after this change: keystroke
 * latency 48 ms -> 24 ms (the gutters-removed floor is 16-24 ms), long
 * frames 55 ms -> 29 ms, forced layout ~35 ms -> ~10-14 ms.
 *
 * ## Why padding, written inline
 *
 * The compensation has to beat both CodeMirror's own
 * `.cm-editor .cm-content { margin: 0 }` and any host rule for the
 * content's padding (the studio's is `.brink-studio .editor .cm-editor
 * .cm-content`, four classes deep). An inline style wins over every
 * stylesheet without a specificity war, and `padding` specifically —
 * rather than `margin` — because hosts set `width: 100%` with
 * `box-sizing: border-box` on the content, where padding stays INSIDE
 * the box (no horizontal overflow) while a margin would push it out.
 *
 * The host's own padding is recovered rather than assumed: the computed
 * padding always equals `host base + what this plugin last wrote`, so
 * subtracting the latter yields the former. A host whose padding changes
 * responsively (the studio's `--editor-margin` is a viewport `clamp()`)
 * is therefore tracked automatically, with no ping-pong read/write.
 */

import type { Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";

/** Marks a view whose gutters are detached, so the rules below apply to
 *  it alone — a stock (non-wrapping) view is untouched. */
const DETACHED_CLASS = "brink-detached-gutters";

/**
 * `inset: 0 auto auto 0` — TOP and LEFT only. Neither a `height` nor a
 * `min-height` may come back (that full-document stretch is the cost
 * this exists to remove), and `bottom` must stay `auto` for a subtler
 * reason: CodeMirror positions gutter markers with margin offsets
 * measured from the DOCUMENT top, so the container is anchored at the
 * document origin and scrolls with the content. Pinning `bottom: 0` caps
 * the box at one viewport height while its markers keep going, so
 * everything below the fold falls OUTSIDE the container's own box and
 * silently stops hit-testing — the markers still paint, still measure as
 * visible, and simply refuse clicks (caught by the play-gutter e2e).
 * With `bottom: auto` the box grows to contain its markers.
 *
 * `z-index` keeps fold/play markers above the content, whose padded box
 * now extends underneath them, so gutter clicks still land.
 *
 * `!important` is load-bearing, not a shortcut: CodeMirror writes
 * `position: sticky` INLINE when it builds the gutter container
 * (`GutterView`, an IE11 fallback comment marks the line) and rewrites
 * `min-height` inline to the content height on every update. A normal
 * rule — at any specificity — loses to an inline style, and re-writing
 * those inline values from this plugin would race CodeMirror's own
 * writes every frame. An `!important` stylesheet declaration beats a
 * non-important inline style once, statically, with no race.
 */
const detachedTheme = EditorView.baseTheme({
  ".cm-scroller": { position: "relative" },
  [`&.${DETACHED_CLASS} .cm-gutters`]: {
    position: "absolute !important",
    inset: "0 auto auto 0",
    height: "auto !important",
    minHeight: "0 !important",
    zIndex: "200",
  },
});

export function detachedGutters(): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      /** The gutter width this plugin last added to the content's
       *  padding — the term subtracted to recover the host's own base. */
      private applied = 0;
      private detached = false;

      constructor(private readonly view: EditorView) {
        this.sync();
      }

      update(update: ViewUpdate): void {
        // Only geometry can change the gutter's width: a wider
        // line-number column past 1,000 lines, a deeper rails stack, a
        // font or pane-size change. `docChanged` covers the line-count
        // crossing before the geometry settles.
        if (update.geometryChanged || update.viewportChanged || update.docChanged) {
          this.sync();
        }
      }

      /**
       * Measure in CodeMirror's read phase and write in its write phase.
       * Writing inline would otherwise land a frame late and the text
       * would visibly jump the moment the gutter grows a digit.
       */
      private sync(): void {
        this.view.requestMeasure({
          read: (view) => {
            const gutters = view.dom.querySelector<HTMLElement>(".cm-gutters");
            return {
              // Resolved state, not construction order: the wrapping
              // facet renders as this class on the content element.
              wrapping: view.contentDOM.classList.contains("cm-lineWrapping"),
              width: gutters === null ? 0 : Math.ceil(gutters.getBoundingClientRect().width),
              paddingLeft: Number.parseFloat(getComputedStyle(view.contentDOM).paddingLeft) || 0,
            };
          },
          write: ({ wrapping, width, paddingLeft }, view) => {
            if (!wrapping) {
              this.restore(view);
              return;
            }
            if (!this.detached) {
              view.dom.classList.add(DETACHED_CLASS);
              this.detached = true;
            }
            // The host's own padding, whatever it currently is.
            const base = Math.max(0, paddingLeft - this.applied);
            if (width !== this.applied) {
              view.contentDOM.style.paddingLeft = `${base + width}px`;
              this.applied = width;
            }
          },
        });
      }

      private restore(view: EditorView): void {
        if (!this.detached) return;
        view.dom.classList.remove(DETACHED_CLASS);
        view.contentDOM.style.removeProperty("padding-left");
        this.detached = false;
        this.applied = 0;
      }

      destroy(): void {
        this.restore(this.view);
      }
    },
  );

  return [plugin, detachedTheme];
}
