/**
 * Tooltip portal (#3349): CM6's `tooltips()` extension defaults to mounting
 * every tooltip — hover cards, lint popups, the autocomplete list — as a
 * child of the editor's own `.cm-editor` element. That is fine in isolation,
 * but a sibling pane with its own stacking context or `overflow` (the
 * Player split, `player.css`'s `z-index: 30`; a group split) clips or
 * paints over it: `position: fixed` positioning escapes SCROLL clipping,
 * not an ancestor's stacking order or `overflow` box.
 *
 * The fix is `tooltips({ parent })` — reparenting the tooltip layer
 * altogether so it is never a descendant of the pane that would otherwise
 * clip it. Not a z-index bump: an `overflow` boundary clips regardless of
 * stacking order, so no z-index fixes it.
 *
 * `parent` has to be a concrete `HTMLElement`, chosen once the view's DOM
 * is actually in the document — extensions are built before that (before
 * `new EditorView({ parent })` runs), so this resolves it lazily via a
 * `ViewPlugin` and pushes it into a `Compartment` instead of hardcoding
 * `document.body`:
 *
 *   - Reusing the real `.brink-studio` root (found via `closest`, the same
 *     lookup `widget-popover.ts` already uses for the same reason) keeps
 *     tooltips inside the DOM subtree the theme scope actually lives on —
 *     `--bs-*` tokens are declared on `.brink-studio[data-theme="…"]`
 *     (see `packages/studio-shell/src/styles/themes/*.css`), a class+
 *     attribute selector that requires the tooltip to still be a
 *     descendant of that exact element, not merely of `document.body`.
 *   - A headless embed (no `.brink-studio` root at all, `theme: false`)
 *     falls back to `document.body` — still escaping every clip/stack
 *     trap, and correct, since a headless embed has no theme scope to
 *     preserve in the first place.
 *
 * `packages/studio-ui/src/styles/editor.css`'s tooltip rules drop the
 * `.cm-editor` ancestor they used to require (`.brink-studio .cm-editor
 * .cm-tooltip`, …) in the same change — CM6 still adds `.cm-tooltip` /
 * `.cm-tooltip-hover` / `.cm-tooltip-autocomplete` to the reparented node,
 * but it is no longer inside `.cm-editor`, so a selector requiring that
 * ancestor would silently stop matching.
 *
 * Reparenting does not disturb the hover-close behavior #3266 already
 * worried about: CM6's own dismiss check (`tooltip.dom.contains(event.
 * relatedTarget)`, `@codemirror/view`'s `HoverTooltipHost.mouseleave`)
 * tests the tooltip's OWN dom, wherever it is mounted — it never assumes
 * the tooltip is inside `.cm-editor`. Reparenting is the officially
 * supported use of `tooltips({ parent })`, not a workaround it fights.
 */
import { Compartment, type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, tooltips } from "@codemirror/view";

/** The theme-scoped root every `--bs-*` token selector requires. */
const STUDIO_ROOT_SELECTOR = ".brink-studio";

/** Reconfigured once per view — see the module doc for why this can't be a
 *  plain (non-compartmentalized) part of the extension list. */
const tooltipParentCompartment = new Compartment();

function resolveTooltipParent(view: EditorView): HTMLElement {
  return view.dom.closest<HTMLElement>(STUDIO_ROOT_SELECTOR) ?? document.body;
}

/**
 * Resolves `view.dom`'s eventual parent and reconfigures the tooltip
 * compartment to mount there instead of inside `.cm-editor`.
 *
 * The lookup is deferred to a microtask rather than run in the plugin's own
 * constructor: a `ViewPlugin`'s constructor runs DURING `new EditorView({
 * parent })`, before that call has returned — `view.dom` exists but
 * `closest()` against its EVENTUAL ancestry is only reliable once the
 * constructor that attaches it has finished. `queueMicrotask` is the same
 * idiom `hir-overlay.ts` and `play-from-here.ts` already use for
 * view-plugin-safe post-mount work; dispatching into a destroyed view is
 * itself harmless (`EditorView.update` no-ops on `this.destroyed`), so no
 * extra guard is needed if the view unmounts before the microtask runs.
 */
const tooltipReparentPlugin = ViewPlugin.fromClass(
  class {
    constructor(view: EditorView) {
      queueMicrotask(() => {
        view.dispatch({
          effects: tooltipParentCompartment.reconfigure(
            tooltips({ parent: resolveTooltipParent(view) }),
          ),
        });
      });
    }
  },
);

/**
 * The editor's tooltip layer: hover cards, lint popups, and the
 * autocomplete list all render through CM6's single core tooltip view
 * plugin, so reconfiguring `tooltips({ parent })` once here covers all
 * three surfaces at once.
 */
export function tooltipPortalExtension(): Extension {
  return [tooltipParentCompartment.of(tooltips()), tooltipReparentPlugin];
}
