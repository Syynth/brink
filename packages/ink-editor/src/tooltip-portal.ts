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
 *   - The mount point is `.brink-tooltip-layer`, a dedicated element the
 *     shell renders inside `.brink-studio` (`packages/studio-ui/src/App.tsx`)
 *     that is explicitly taken out of flow (`position: absolute; width: 0;
 *     height: 0` — see `editor.css`). CM6's own tooltip container div is
 *     `position: relative`, NOT `fixed`/`absolute` (`createContainer()`,
 *     `@codemirror/view`), so mounting it as a direct flex child of
 *     `.brink-studio` (`display: flex; flex-direction: column`,
 *     `packages/studio-shell/src/styles/frame.css`) makes it an in-flow flex
 *     item that participates in — and disrupts — the shell's layout. Every
 *     mounted editor adds one such container, so the Continuous view (many
 *     editors) compounded it. Routing through the zero-size layer keeps the
 *     container out of the shell's flex flow while individual tooltips still
 *     position correctly (CM6 measures and places them with `position:
 *     fixed`, which is unaffected by an ancestor's size).
 *   - The layer still has to live inside `.brink-studio` (found via
 *     `closest`, the same lookup `widget-popover.ts` uses for the same
 *     reason) so tooltips stay inside the DOM subtree the theme scope
 *     actually lives on — `--bs-*` tokens are declared on
 *     `.brink-studio[data-theme="…"]` (see
 *     `packages/studio-shell/src/styles/themes/*.css`), a class+attribute
 *     selector that requires the tooltip to still be a descendant of that
 *     exact element, not merely of `document.body`.
 *   - A headless embed (no `.brink-studio` root at all, `theme: false`, or
 *     any host that doesn't render the layer div) falls back to
 *     `document.body` — still escaping every clip/stack/flex trap, and
 *     correct, since there's no theme scope (or no layer) to route through.
 *
 * `packages/studio-ui/src/styles/editor.css`'s tooltip rules drop the
 * `.cm-editor` ancestor they used to require (`.brink-studio .cm-editor
 * .cm-tooltip`, …) in the same change — CM6 still adds `.cm-tooltip` /
 * `.cm-tooltip-hover` / `.cm-tooltip-autocomplete` to the reparented node,
 * but it is no longer inside `.cm-editor`, so a selector requiring that
 * ancestor would silently stop matching. The extra `.brink-tooltip-layer`
 * wrapper does not change this: `.brink-studio .cm-tooltip` is a descendant
 * selector, indifferent to what sits between the two elements.
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

/** The out-of-flow mount point the shell renders inside `.brink-studio`
 *  (`App.tsx`) — see the module doc for why tooltips can't mount directly
 *  on the `.brink-studio` flex root itself. */
const TOOLTIP_LAYER_SELECTOR = ".brink-tooltip-layer";

/** Reconfigured once per view — see the module doc for why this can't be a
 *  plain (non-compartmentalized) part of the extension list. */
const tooltipParentCompartment = new Compartment();

function resolveTooltipParent(view: EditorView): HTMLElement {
  return (
    view.dom
      .closest<HTMLElement>(STUDIO_ROOT_SELECTOR)
      ?.querySelector<HTMLElement>(TOOLTIP_LAYER_SELECTOR) ?? document.body
  );
}

/**
 * Resolves `view.dom`'s eventual parent and reconfigures the tooltip
 * compartment to mount there instead of inside `.cm-editor`.
 *
 * The lookup is deferred to a microtask rather than run in the plugin's own
 * constructor — NOT because DOM ancestry is unsettled there. It already is
 * settled: `EditorView`'s constructor calls `config.parent.appendChild(
 * this.dom)` (`@codemirror/view@6.43.9/dist/index.js:7906`) before it builds
 * any `ViewPlugin` (`this.plugins = this.state.facet(viewPlugin).map(...)`,
 * :7916), so `view.dom` is already attached and `closest()` against it is
 * reliable inside the constructor. The real constraint is dispatch safety:
 * the constructor sets `this.updateState = UpdateState.Updating` up front
 * (:7885) and only resets it to `Idle` after every plugin has been
 * constructed (:7925), and `EditorView.update` throws
 * ("Calls to EditorView.update are not allowed while an update is in
 * progress", :7948-7949) whenever `updateState !== Idle`. A synchronous
 * `view.dispatch(...)` from inside this `ViewPlugin`'s own constructor would
 * land in that window and throw. `queueMicrotask` escapes it by running
 * after the constructor (and its `updateState` reset) has returned — the
 * same "defer past this synchronous lifecycle callback" idiom
 * `hir-overlay.ts` (batching perf spans past a synchronous rebuild) and
 * `play-from-here.ts` (deferring a callback out of a `ViewPlugin.update`)
 * already use, though for different callbacks and different restrictions.
 * Dispatching into a destroyed view is itself harmless (`EditorView.update`
 * no-ops on `this.destroyed`, :7957-7960), so no extra guard is needed if
 * the view unmounts before the microtask runs.
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
