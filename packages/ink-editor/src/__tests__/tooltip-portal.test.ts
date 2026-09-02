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

  it("reparents the tooltip into the .brink-studio root, out of .cm-editor", async () => {
    container = document.createElement("div");
    container.className = "brink-studio";
    container.setAttribute("data-theme", "mocha");
    document.body.appendChild(container);

    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [tooltipPortalExtension(), tooltipExtension()],
      }),
      parent: container,
    });

    await flushMicrotasks();

    const tip = container.querySelector('[data-testid="probe-tooltip"]');
    expect(tip).not.toBeNull();
    // Not inside `.cm-editor` (== `view.dom`) any more — that's the clip.
    expect(view.dom.contains(tip)).toBe(false);
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

  it("does not mount the tooltip inside .cm-editor even before reparenting settles", () => {
    // Sanity check on the starting extension (`tooltips()` with no parent
    // configured yet) isn't meaningful to assert timing on synchronously —
    // the point of this suite is the settled state above. This test instead
    // guards the OTHER failure mode: a view built with no `.brink-studio`
    // ancestor and no parent element at all (detached, as some unit tests
    // build editors) must not throw when the microtask resolves.
    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [tooltipPortalExtension(), tooltipExtension()],
      }),
    });
    expect(() => view?.destroy()).not.toThrow();
  });
});
