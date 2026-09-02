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
 * padding always equals `host base + the compensation currently applied`,
 * so subtracting the latter yields the former.
 *
 * ## Why the compensation is recorded in the DOM (#3352)
 *
 * That subtraction needs to know how much compensation is in force RIGHT
 * NOW, and the only trustworthy place to keep that is next to the value
 * it describes. It used to live in a per-instance `applied` accumulator,
 * which drifts the moment the two disagree — and they do:
 * `EditorView.updateAttrs` writes `contentDOM`'s inline style with a
 * WHOLE-VALUE `dom.style.cssText = attrs.style` (@codemirror/view's
 * `updateAttrs` helper), so any update that changes the content's
 * attribute-derived style string — a `tabSize` reconfigure, a
 * `contentAttributes` source whose style changes — silently erases the
 * compensating `padding-left` while the plugin instance survives believing
 * it is still applied. With a stale accumulator the next pass then
 * computed `base = max(0, hostPadding - applied)` from a padding that no
 * longer contained the compensation, and — because the gutter width itself
 * had not changed — wrote NOTHING at all. The text sat one gutter width to
 * the left, UNDER the floating gutter overlay, with nothing overflowing,
 * so horizontal scrolling could not bring it back. Only a reload recovered.
 *
 * So the compensation is recorded as `--brink-detached-gutter-compensation`
 * in the same inline declaration as the padding it pays for. This is
 * bookkeeping, not a design token: it is never read by any stylesheet. Its
 * value is that it is written, erased and clobbered ATOMICALLY with the
 * padding — anything that drops one drops the other, so the pair is either
 * both present (subtract to recover the base) or both gone (the computed
 * padding IS the base). Every pass therefore recomputes the target from
 * the gutter's actual measured width and the DOM's actual state, with no
 * carried-over term, and writes whenever the DOM does not already say
 * exactly that. Any drift — however it arose — self-heals on the next
 * layout instead of persisting until reload.
 *
 * One consequence worth stating plainly: because the inline padding masks
 * the host's cascaded value, `base` is whatever the host's padding was
 * when the compensation was last (re)established, not a live read of it. A
 * host whose padding changes responsively (the studio's `--editor-margin`
 * is a viewport `clamp()`) is picked up the next time the pair is cleared
 * and re-applied, not on the resize itself.
 */

import type { Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";

/** Marks a view whose gutters are detached, so the rules below apply to
 *  it alone — a stock (non-wrapping) view is untouched. */
const DETACHED_CLASS = "brink-detached-gutters";

/**
 * The compensation currently written into `contentDOM`'s `padding-left`,
 * recorded in the SAME inline declaration so the two can only ever be
 * present or absent together (see the header, #3352). Bookkeeping, not a
 * design token — no stylesheet reads it.
 */
const COMPENSATION_PROP = "--brink-detached-gutter-compensation";

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

/**
 * Drop the compensating padding and its record together — the atomicity
 * the recovery in `sync` depends on (#3352). Never one without the other.
 */
function clearCompensation(view: EditorView): void {
  view.contentDOM.style.removeProperty("padding-left");
  view.contentDOM.style.removeProperty(COMPENSATION_PROP);
}

export function detachedGutters(): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      /**
       * Public because the `editorAttributes` source below reads it back
       * off the view — that facet, not the imperative `classList` write,
       * is what makes the class survive (see `detachedAttributes`).
       */
      detached = false;

      constructor(private readonly view: EditorView) {
        this.sync();
      }

      update(update: ViewUpdate): void {
        // Only geometry can change the gutter's width: a wider
        // line-number column past 1,000 lines, a deeper rails stack, a
        // font or pane-size change. `docChanged` covers the line-count
        // crossing before the geometry settles.
        //
        // A reconfigure changes no geometry at all, and is here for the
        // other half of #3352: it is the update on which CodeMirror may
        // rewrite `contentDOM`'s inline style wholesale (see the header),
        // erasing the compensation. Syncing on it heals the erasure in the
        // same frame rather than waiting for the next keystroke.
        if (
          update.geometryChanged ||
          update.viewportChanged ||
          update.docChanged ||
          update.transactions.some((tr) => tr.reconfigured)
        ) {
          this.sync();
        }
      }

      /**
       * Measure in CodeMirror's read phase and write in its write phase.
       * Writing inline would otherwise land a frame late and the text
       * would visibly jump the moment the gutter grows a digit.
       *
       * Every input the write phase acts on is read here, from the DOM,
       * on this pass — nothing is carried between passes (#3352). That is
       * what makes the write authoritative: it can always name the value
       * the content SHOULD have, whatever happened to it since.
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
              // The compensation the content is actually carrying, read
              // back off the element rather than remembered. Absent (0)
              // whenever the inline declaration was dropped or clobbered,
              // which is precisely when the padding lost it too.
              compensation:
                Number.parseFloat(
                  view.contentDOM.style.getPropertyValue(COMPENSATION_PROP),
                ) || 0,
            };
          },
          write: ({ wrapping, width, paddingLeft, compensation }, view) => {
            if (!wrapping) {
              this.restore(view);
              return;
            }
            if (!this.detached) {
              view.dom.classList.add(DETACHED_CLASS);
              this.detached = true;
            }
            if (width === 0) {
              // No gutters to pay for. Hand the content back to the host's
              // own cascade rather than pinning it at the base we happened
              // to measure.
              if (compensation !== 0) clearCompensation(view);
              return;
            }
            // The host's own padding, recovered from the pair the DOM
            // carries. A padding SMALLER than the compensation it is
            // supposed to contain is a contradiction, and it has exactly
            // one honest reading: the padding was replaced behind this
            // plugin's back, so none of what is there now is ours and all
            // of it is the host's — which the rewrite below then
            // compensates afresh.
            const base = paddingLeft >= compensation ? paddingLeft - compensation : paddingLeft;
            const target = base + width;
            // Written whenever the DOM does not ALREADY say exactly this,
            // not when the width changed: a width that never changes is
            // exactly the case where drift used to become permanent.
            if (compensation !== width || paddingLeft !== target) {
              view.contentDOM.style.setProperty(COMPENSATION_PROP, `${width}px`);
              view.contentDOM.style.paddingLeft = `${target}px`;
            }
          },
        });
      }

      private restore(view: EditorView): void {
        if (!this.detached) return;
        view.dom.classList.remove(DETACHED_CLASS);
        clearCompensation(view);
        this.detached = false;
      }

      destroy(): void {
        this.restore(this.view);
      }
    },
  );

  /**
   * Republish the marker class every time CodeMirror rebuilds the editor
   * element's attributes. This is load-bearing, not redundancy with the
   * `classList.add` above.
   *
   * CodeMirror owns `view.dom`'s `class` attribute: `updateAttrs` writes it
   * with a whole-value `setAttribute("class", …)` built from `"cm-editor"`,
   * the focus flag and the `editorAttributes` facet. Any class added
   * imperatively is therefore erased the next time that runs — and one of
   * the times it runs is the focus change. That is the bug this fixes
   * (#3131 follow-up): loading the studio and clicking anywhere in the
   * editor dropped `brink-detached-gutters`, the gutters fell back from
   * `absolute` to their inline `sticky`, and the text jumped right by the
   * full gutter width (~70px) because the compensating padding stayed. It
   * read as "the editor slides sideways when you click it".
   *
   * A FUNCTION source rather than a static value: `attrsFromFacet`
   * evaluates function sources on every `updateAttrs`, so this re-reads the
   * plugin's live flag instead of freezing whatever it was at
   * configuration time. `view.plugin(plugin)` scopes the read to the view
   * being rendered, so one shared extension still serves many views.
   *
   * The imperative write stays for immediacy — the measure write phase
   * flips the class in the same frame it measures, with no update cycle to
   * wait for — and this makes it durable.
   */
  const detachedAttributes = EditorView.editorAttributes.of((view) =>
    view.plugin(plugin)?.detached === true ? { class: DETACHED_CLASS } : null,
  );

  return [plugin, detachedAttributes, detachedTheme];
}
