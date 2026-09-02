/**
 * Tooltip portal (#3349): editor tooltips must escape `.cm-editor` so a
 * sibling pane's own stacking context or `overflow` (the Player split,
 * `player.css`'s `z-index: 30`) can't clip them.
 *
 * These tests drive the mechanism directly through `showTooltip` — the one
 * facet CM6's core tooltip view plugin reads regardless of which producer
 * (hover, lint, or autocomplete) put the value there — rather than each
 * producer's own async/mouse-driven path, which jsdom can't measure
 * reliably (`posAtCoords` needs real layout metrics). Reconfiguring
 * `tooltips({ parent })` is a single, shared piece of wiring: proving the
 * mechanism once here is proving it for all three surfaces the issue names.
 */
import { describe, expect, it, afterEach } from "vitest";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, showTooltip, type Tooltip } from "@codemirror/view";
import { tooltipPortalExtension } from "../tooltip-portal.js";

/** Flush the `queueMicrotask` the extension defers its DOM lookup into. */
function flushMicrotasks(): Promise<void> {
  return new Promise((resolve) => queueMicrotask(resolve));
}

/** A tooltip fixed at doc position 0, tagged so it's easy to find in the DOM. */
function fixedTooltip(): Tooltip {
  return {
    pos: 0,
    create: () => {
      const dom = document.createElement("div");
      dom.setAttribute("data-testid", "probe-tooltip");
      return { dom };
    },
  };
}

function tooltipExtension(): Extension {
  return showTooltip.of(fixedTooltip());
}

describe("tooltipPortalExtension", () => {
  let view: EditorView | null = null;
  let container: HTMLElement | null = null;

  afterEach(() => {
    view?.destroy();
    view = null;
    container?.remove();
    container = null;
  });

  it("reparents the tooltip into the .brink-tooltip-layer, out of .cm-editor", async () => {
    container = document.createElement("div");
    container.className = "brink-studio";
    container.setAttribute("data-theme", "mocha");
    // The shell renders this layer inside `.brink-studio` (`App.tsx`) as the
    // out-of-flow mount point — without it, `resolveTooltipParent` falls
    // through to `document.body` (see the fallback test below).
    const layer = document.createElement("div");
    layer.className = "brink-tooltip-layer";
    container.appendChild(layer);
    document.body.appendChild(container);

    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [tooltipPortalExtension(), tooltipExtension()],
      }),
      parent: container,
    });

    await flushMicrotasks();

    const tip = layer.querySelector('[data-testid="probe-tooltip"]');
    expect(tip).not.toBeNull();
    // Not inside `.cm-editor` (== `view.dom`) any more — that's the clip.
    expect(view.dom.contains(tip)).toBe(false);
    // Mounted through the out-of-flow layer, not directly on the flex root
    // (that was the layout-breaking shape the E2E regression caught).
    expect(layer.contains(tip)).toBe(true);
    // Still inside the theme-scoped root, so `.brink-studio[data-theme]`
    // token selectors still match it.
    expect(tip?.closest(".brink-studio")).toBe(container);
  });

  it("falls back to document.body with no .brink-studio ancestor (headless)", async () => {
    container = document.createElement("div");
    document.body.appendChild(container);

    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [tooltipPortalExtension(), tooltipExtension()],
      }),
      parent: container,
    });

    await flushMicrotasks();

    const tip = document.body.querySelector('[data-testid="probe-tooltip"]');
    expect(tip).not.toBeNull();
    // CM6 wraps the tooltip in its own positioning container, so this checks
    // ancestry (mounted somewhere under document.body) rather than direct
    // parentage — the same shape the `.brink-studio` case above mounts into.
    expect(document.body.contains(tip)).toBe(true);
    expect(view.dom.contains(tip)).toBe(false);
  });

  it("falls back to .brink-studio's own root with no .brink-tooltip-layer inside it", async () => {
    // A `.brink-studio` ancestor that doesn't render the layer div (a stale
    // host, or a test harness building its own container) must not fall
    // back to mounting directly on the `.brink-studio` flex root — that is
    // the exact in-flow-flex-item shape that broke the shell layout. It
    // falls all the way through to `document.body` instead.
    container = document.createElement("div");
    container.className = "brink-studio";
    document.body.appendChild(container);

    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [tooltipPortalExtension(), tooltipExtension()],
      }),
      parent: container,
    });

    await flushMicrotasks();

    const tip = document.body.querySelector('[data-testid="probe-tooltip"]');
    expect(tip).not.toBeNull();
    // Not a direct/eventual child of the bare `.brink-studio` root.
    expect(container.contains(tip)).toBe(false);
    expect(document.body.contains(tip)).toBe(true);
  });

  it("leaves no stray tooltip DOM when the view is destroyed before the microtask settles", async () => {
    // Guards the failure mode #3266 worried about: a view destroyed (e.g. a
    // fast unmount) before the deferred reparent has run. `EditorView.update`
    // no-ops on `this.destroyed` (`@codemirror/view`), so the reconfigure
    // dispatch below must be silently dropped rather than leaking a tooltip
    // or a reparented container into `document.body`.
    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [tooltipPortalExtension(), tooltipExtension()],
      }),
    });
    view.destroy();

    await flushMicrotasks();

    expect(document.body.querySelector('[data-testid="probe-tooltip"]')).toBeNull();
    expect(document.body.querySelector(".cm-tooltip")).toBeNull();
  });
});
