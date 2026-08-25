/**
 * Cmd-click on a *definition* runs Find References instead of a no-op
 * self-navigation (ruled 2026-08-24: "you're already there"). Cmd-click on
 * a use site keeps navigating to the definition, and the references path
 * falls back to navigation when there is nothing to show.
 *
 * Tested through `gotoOrReferencesAt` (the extracted click action) at
 * explicit offsets — jsdom has no layout, so the DOM handler's
 * `posAtCoords` is untestable here and stays a thin wrapper.
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Location } from "@brink/wasm-types";
import { gotoOrReferencesAt } from "../goto-definition.js";
import { brinkStudio } from "../extensions.js";

const DOC = "== barter ==\nSee the wares.\n-> barter\n";
const DECL: Location = { file: "a.ink", start: 3, end: 9 };
const USE_POS = DOC.indexOf("-> barter") + 4;

let view: EditorView | null = null;
afterEach(() => {
  view?.destroy();
  view = null;
});

function makeView(): EditorView {
  view = new EditorView({ state: EditorState.create({ doc: DOC }), parent: document.body });
  return view;
}

/** Both the decl token and the use site resolve to the declaration. */
const gotoDefinition = (): Location | null => DECL;

describe("gotoOrReferencesAt", () => {
  it("shows references when the clicked position is inside the definition span", async () => {
    const v = makeView();
    const shown: Array<{ symbol: string; count: number; declaration: Location | null }> = [];
    const ok = await gotoOrReferencesAt(v, 5, {
      gotoDefinition,
      getActiveFile: () => "a.ink",
      findReferences: () => [DECL, { file: "a.ink", start: USE_POS, end: USE_POS + 6 }],
      onShowReferences: (symbol, locations, declaration) =>
        shown.push({ symbol, count: locations.length, declaration: declaration ?? null }),
    });
    expect(ok).toBe(true);
    expect(shown).toEqual([{ symbol: "barter", count: 2, declaration: DECL }]);
    // No navigation happened: the cursor stays where it was.
    expect(v.state.selection.main.head).toBe(0);
  });

  it("navigates from a use site (position outside the definition span)", async () => {
    const v = makeView();
    const shown: string[] = [];
    const ok = await gotoOrReferencesAt(v, USE_POS, {
      gotoDefinition,
      getActiveFile: () => "a.ink",
      findReferences: () => [DECL],
      onShowReferences: (symbol) => {
        shown.push(symbol);
      },
    });
    expect(ok).toBe(true);
    expect(shown).toEqual([]);
    expect(v.state.selection.main.head).toBe(DECL.start);
  });

  it("navigates from the definition when it lives in a different file", async () => {
    const v = makeView();
    const navigated: Location[] = [];
    const ok = await gotoOrReferencesAt(v, 5, {
      gotoDefinition: () => ({ file: "other.ink", start: 3, end: 9 }),
      getActiveFile: () => "a.ink",
      onNavigateToFile: (location) => {
        navigated.push(location);
      },
      findReferences: () => [DECL],
      onShowReferences: () => {
        throw new Error("references must not run for a cross-file definition");
      },
    });
    expect(ok).toBe(true);
    expect(navigated).toEqual([{ file: "other.ink", start: 3, end: 9 }]);
  });

  it("falls back to navigation when references come back empty", async () => {
    const v = makeView();
    const ok = await gotoOrReferencesAt(v, 5, {
      gotoDefinition,
      getActiveFile: () => "a.ink",
      findReferences: () => [],
      onShowReferences: () => {
        throw new Error("nothing to show");
      },
    });
    expect(ok).toBe(true);
    expect(v.state.selection.main.head).toBe(DECL.start);
  });

  it("keeps plain navigation when no references callback is wired", async () => {
    const v = makeView();
    const ok = await gotoOrReferencesAt(v, 5, {
      gotoDefinition,
      getActiveFile: () => "a.ink",
    });
    expect(ok).toBe(true);
    expect(v.state.selection.main.head).toBe(DECL.start);
  });

  it("stays inert when nothing resolves", async () => {
    const v = makeView();
    expect(await gotoOrReferencesAt(v, 20, { gotoDefinition: () => null })).toBe(false);
  });
});

describe("gotoOrReferencesAt on INCLUDE lines", () => {
  // The INCLUDE fallback needs line classification (elementTypeField), so
  // these mount the full extension set with stub callbacks.
  function makeClassifiedView(doc: string): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [
          brinkStudio({
            compile: () => ({ ok: true, diagnostics: [] }) as never,
            getSemanticTokens: () => [],
            getTokenTypeNames: () => [],
          } as Parameters<typeof brinkStudio>[0]),
        ],
      }),
      parent: document.body,
    });
    return view;
  }

  const DOC2 = "INCLUDE scenes/intro.ink\nHello\n";

  it("cmd-click on the path text opens the file (ruled 2026-08-24)", async () => {
    const v = makeClassifiedView(DOC2);
    const nav = vi.fn();
    const ok = await gotoOrReferencesAt(v, DOC2.indexOf("intro"), {
      gotoDefinition: () => null,
      onNavigateToFile: nav,
    });
    expect(ok).toBe(true);
    expect(nav).toHaveBeenCalledWith({ file: "scenes/intro.ink", start: 0, end: 0 });
  });

  it("the INCLUDE keyword is not part of the clickable span", async () => {
    const v = makeClassifiedView(DOC2);
    const nav = vi.fn();
    expect(
      await gotoOrReferencesAt(v, 2, { gotoDefinition: () => null, onNavigateToFile: nav }),
    ).toBe(false);
    expect(nav).not.toHaveBeenCalled();
  });

  it("stays inert without an onNavigateToFile host", async () => {
    const v = makeClassifiedView(DOC2);
    expect(
      await gotoOrReferencesAt(v, DOC2.indexOf("intro"), { gotoDefinition: () => null }),
    ).toBe(false);
  });
});
