/**
 * Tooltip portal wiring (#3349, #3357 review): `tooltip-portal.test.ts` in
 * ink-editor drives `tooltipPortalExtension()` directly — the internal
 * mechanism, not what a real consumer receives. The user-visible effect
 * rides on one line in `packages/ink-editor/src/extensions.ts` wiring
 * `tooltipPortalExtension()` into `brinkStudio()`'s own extension array; if
 * that line were ever dropped, every test driving `tooltipPortalExtension()`
 * directly would still pass while the shipped editor regressed. This suite
 * builds a real `brinkStudio(...)` bundle (the same shape
 * `headless-theme.test.ts` and `fold-kinds.test.ts` already construct) and
 * asserts the consumer-visible outcome: a tooltip reparents out of the
 * editor's own DOM when mounted under the shell shape `App.tsx` renders
 * (`.brink-studio` > `.brink-tooltip-layer`).
 */

import { afterEach, describe, expect, it } from "vitest";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, showTooltip, type Tooltip } from "@codemirror/view";
import { brinkStudio } from "@brink-lang/editor";

const minimal = {
  compile: () => ({ ok: true, diagnostics: [] }) as never,
  getSemanticTokens: () => [],
  getTokenTypeNames: () => [],
};

/** Flush the `queueMicrotask` `tooltipPortalExtension()` defers its DOM
 *  lookup into (see `tooltip-portal.ts`). */
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

describe("brinkStudio() tooltip reparenting", () => {
  let view: EditorView | undefined;
  let root: HTMLElement | undefined;

  afterEach(() => {
    view?.destroy();
    view = undefined;
    root?.remove();
    root = undefined;
  });

  it("reparents a brinkStudio() editor's tooltip into .brink-tooltip-layer", async () => {
    // The exact shape `App.tsx` renders: `.brink-tooltip-layer` as a sibling
    // inside `.brink-studio`, alongside (not inside) the editor's own mount
    // point.
    root = document.createElement("div");
    root.className = "brink-studio";
    root.setAttribute("data-theme", "mocha");
    const layer = document.createElement("div");
    layer.className = "brink-tooltip-layer";
    root.appendChild(layer);
    const editorMount = document.createElement("div");
    root.appendChild(editorMount);
    document.body.appendChild(root);

    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [brinkStudio(minimal), tooltipExtension()],
      }),
      parent: editorMount,
    });

    await flushMicrotasks();

    const tip = layer.querySelector('[data-testid="probe-tooltip"]');
    expect(tip).not.toBeNull();
    // The regression this guards: if `tooltipPortalExtension()` were ever
    // dropped from `brinkStudio()`'s extension array, this would stay
    // inside `view.dom` (== `.cm-editor`) instead.
    expect(view.dom.contains(tip)).toBe(false);
    expect(layer.contains(tip)).toBe(true);
  });
});
