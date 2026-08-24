/**
 * SearchCardList — the card surface over the frozen snapshot
 * (docs/search-results-cards-spec.md, PR C).
 *
 * Mounts the real component over a store seeded through the slice
 * (runSearch/showReferences against a fake session), and verifies: one
 * card per match with the file:line header and containing-knot lookup,
 * collapse (chevron → store override; the all-flag as default; collapsed
 * cards show the truncated preview instead of a buffer), the `edited`
 * badge on stale matches, reveal through `editor.reveal`, write-through
 * from a card's CM buffer via the apply-edits seam WITHOUT replacing the
 * snapshot (frozen semantics), and the per-file token cache (one
 * highlighting call per file, not per card).
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
  EDITOR_REVEAL_COMMAND_ID,
} from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import { SearchCardList, StoreProvider } from "@brink/studio-ui";
import type { EditorView } from "@codemirror/view";
import type { FileOutline } from "@brink/wasm-types";

// ── Harness ─────────────────────────────────────────────────────────

let container: HTMLElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container?.remove();
  container = null;
  (window as unknown as Record<string, unknown>).__brinkSearchCardViews = undefined;
});

interface Harness {
  store: StudioStore;
  commands: CommandRegistry;
  sources: Map<string, string>;
  highlightCalls: string[];
  /** Deliver the compile seam (remap), like an edit debounce would. */
  compile(): void;
  rerender(): void;
}

function knotOutline(
  path: string,
  knots: Array<{ name: string; start: number; end: number }>,
): FileOutline {
  return {
    path,
    symbols: knots.map(({ name, start, end }) => ({
      name,
      kind: "knot",
      start,
      end: start + name.length,
      full_start: start,
      full_end: end,
      children: [],
    })),
  } as unknown as FileOutline;
}

function mountList(
  files: Record<string, string>,
  seed: (store: StudioStore) => void,
  outline: FileOutline[] = [],
): Harness {
  const sources = new Map(Object.entries(files));
  const highlightCalls: string[] = [];
  const session = {
    listFiles: () => [...sources.keys()].map((path) => ({ path })),
    getFileSource: (path: string) => sources.get(path) ?? null,
    updateFile: (path: string, source: string) => sources.set(path, source),
    openDocument: (path: string) => {
      highlightCalls.push(path);
      return 1;
    },
    getSemanticTokensDoc: () => [],
    closeDocument: () => true,
  };
  const project = {
    getSession: () => session,
    applyEdit: (path: string, source: string) => {
      sources.set(path, source);
      return true;
    },
  };
  const store = createStudioStore();
  const compile = (): void => {
    store.getState().setCompileResult(outline, { errors: 0, warnings: 0 }, [], null);
  };
  store.setState({
    _project: project as never,
    _documents: { invalidateFile: vi.fn(), triggerCompile: vi.fn(compile) } as never,
    outline,
  });
  seed(store);

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  const themes = new ThemeService();
  const overrides = new KeymapOverridesService();
  const render = (): void => {
    act(() => {
      root?.render(
        createElement(
          ShellProvider,
          { commands, themes, keymapOverrides: overrides, isMac: true } as never,
          createElement(StoreProvider, { store } as never, createElement(SearchCardList)),
        ),
      );
    });
  };
  render();
  return { store, commands, sources, highlightCalls, compile, rerender: render };
}

function cardEls(): HTMLElement[] {
  return [...(container?.querySelectorAll<HTMLElement>(".search-card") ?? [])];
}

function cardView(id: string): EditorView | undefined {
  const views = (window as unknown as { __brinkSearchCardViews?: Record<string, EditorView> })
    .__brinkSearchCardViews;
  return views?.[id];
}

const DOC = "== intro ==\nYour torch is lit.\nOnward.\n== outro ==\nNo torch here.";

function seedQuery(store: StudioStore, query = "torch"): void {
  store.getState().setSearchQuery(query);
  store.getState().runSearch();
}

// ── Tests ───────────────────────────────────────────────────────────

describe("SearchCardList", () => {
  it("renders one card per match with file:line and the containing knot", () => {
    mountList(
      { "a.ink": DOC },
      (store) => seedQuery(store),
      [
        knotOutline("a.ink", [
          { name: "intro", start: 0, end: DOC.indexOf("== outro") },
          { name: "outro", start: DOC.indexOf("== outro"), end: DOC.length },
        ]),
      ],
    );
    const cards = cardEls();
    expect(cards).toHaveLength(2);
    expect(cards[0]?.querySelector(".search-card-loc")?.textContent).toBe("a.ink:2");
    expect(cards[0]?.querySelector(".search-card-container")?.textContent).toBe("intro");
    expect(cards[1]?.querySelector(".search-card-loc")?.textContent).toBe("a.ink:5");
    expect(cards[1]?.querySelector(".search-card-container")?.textContent).toBe("outro");
    // Every expanded card mounts an editable buffer (jsdom: no
    // IntersectionObserver, all cards count as visible).
    expect(cards[0]?.querySelector(".search-card-editor .cm-content")).not.toBeNull();
  });

  it("shows the card's context window with the file-offset gutter", () => {
    const h = mountList({ "a.ink": DOC }, (store) => seedQuery(store));
    const view = cardView("a.ink#0");
    // Default 1↑ 2↓ around line 2.
    expect(view?.state.doc.toString()).toBe("== intro ==\nYour torch is lit.\nOnward.\n== outro ==");
    h.store.getState().setSearchContextLines({ before: 0, after: 0 });
    h.rerender();
    expect(cardView("a.ink#0")?.state.doc.toString()).toBe("Your torch is lit.");
  });

  it("collapses via the chevron: preview instead of buffer, store override recorded", () => {
    const h = mountList({ "a.ink": DOC }, (store) => seedQuery(store));
    const chevron = cardEls()[0]?.querySelector<HTMLButtonElement>(".search-card-chevron");
    act(() => chevron?.click());
    expect(h.store.getState().searchCardCollapsed).toEqual({ "a.ink#0": true });
    const card = cardEls()[0];
    expect(card?.querySelector(".search-card-editor")).toBeNull();
    expect(card?.querySelector(".search-card-preview")?.textContent).toBe("Your torch is lit.");
    expect(card?.querySelector(".search-card-preview .brink-search-hit")?.textContent).toBe("torch");
  });

  it("the all-flag is the default for cards without an override", () => {
    const h = mountList({ "a.ink": DOC }, (store) => seedQuery(store));
    act(() => h.store.getState().setAllSearchCardsCollapsed(true));
    expect(cardEls().every((c) => c.querySelector(".search-card-editor") === null)).toBe(true);
    // A per-card override re-expands just that card.
    act(() => h.store.getState().setSearchCardCollapsed("a.ink#0", false));
    expect(cardEls()[0]?.querySelector(".search-card-editor")).not.toBeNull();
    expect(cardEls()[1]?.querySelector(".search-card-editor")).toBeNull();
  });

  it("badges a stale match `edited` and keeps its card (frozen snapshot)", () => {
    const h = mountList({ "a.ink": DOC }, (store) => seedQuery(store));
    // Break the second match through a regular-editor edit + compile seam.
    h.sources.set("a.ink", DOC.replace("No torch here.", "No lantern here."));
    act(() => h.compile());
    const cards = cardEls();
    expect(cards).toHaveLength(2);
    expect(cards[1]?.querySelector(".search-card-badge.edited")?.textContent).toBe("edited");
    expect(cards[0]?.querySelector(".search-card-badge")).toBeNull();
  });

  it("reveals through editor.reveal with the match span", () => {
    const h = mountList({ "a.ink": DOC }, (store) => seedQuery(store));
    const revealed: unknown[] = [];
    h.commands.register({
      id: EDITOR_REVEAL_COMMAND_ID,
      title: "Reveal",
      run: (payload: unknown) => {
        revealed.push(payload);
        return true;
      },
    } as never);
    act(() => cardEls()[0]?.querySelector<HTMLButtonElement>(".search-card-reveal")?.click());
    expect(revealed).toEqual([
      {
        kind: "source",
        file: "a.ink",
        span: { start: DOC.indexOf("torch"), end: DOC.indexOf("torch") + 5 },
      },
    ]);
  });

  it("card edits write through the apply seam without replacing the snapshot", () => {
    const h = mountList({ "a.ink": DOC }, (store) => seedQuery(store));
    const before = h.store.getState().searchResults;
    expect(before).not.toBeNull();

    const view = cardView("a.ink#0");
    expect(view).toBeDefined();
    const doc = view?.state.doc.toString() ?? "";
    const at = doc.indexOf("torch");
    vi.useFakeTimers();
    try {
      act(() => {
        view?.dispatch({ changes: { from: at, to: at + 5, insert: "lantern" } });
      });
      act(() => {
        vi.runAllTimers();
      });
    } finally {
      vi.useRealTimers();
    }

    // The source took the edit through applyEdit…
    expect(h.sources.get("a.ink")).toContain("Your lantern is lit.");
    // …and the snapshot was REMAPPED (compile seam), never re-searched:
    // both rows survive, the edited one flagged.
    const after = h.store.getState().searchResults;
    expect(after?.totalMatches).toBe(2);
    expect(after?.origin).toMatchObject({ kind: "query", query: "torch" });
    expect(after?.files[0]?.matches[0]).toMatchObject({ edited: true, stale: true });
    expect(after?.files[0]?.matches[1]).toMatchObject({ edited: false, stale: false });
  });

  it("tokenizes each file once for all its cards (per-file cache)", () => {
    const h = mountList({ "a.ink": DOC }, (store) => seedQuery(store));
    expect(cardEls()).toHaveLength(2);
    expect(h.highlightCalls).toEqual(["a.ink"]);
  });

  it("replace previews: pending cards show old→new with Accept/skip (PR D)", () => {
    const h = mountList({ "a.ink": DOC }, (store) => {
      seedQuery(store);
      store.getState().setSearchReplaceOpen(true);
      store.getState().setSearchReplace("lantern");
    });
    const card = cardEls()[0];
    // Display-only preview replaces the editable buffer.
    expect(card?.querySelector(".search-card-editor")).toBeNull();
    expect(card?.querySelector(".search-card-del")?.textContent).toBe("torch");
    expect(card?.querySelector(".search-card-ins")?.textContent).toBe("lantern");
    expect(card?.querySelector(".search-card-accept")).not.toBeNull();
    expect(card?.querySelector(".search-card-skip")).not.toBeNull();
  });

  it("per-card Accept applies through the seam and receipts the card", () => {
    const h = mountList({ "a.ink": DOC }, (store) => {
      seedQuery(store);
      store.getState().setSearchReplaceOpen(true);
      store.getState().setSearchReplace("lantern");
    });
    act(() => cardEls()[0]?.querySelector<HTMLButtonElement>(".search-card-accept")?.click());
    expect(h.sources.get("a.ink")).toContain("Your lantern is lit.");
    const card = cardEls()[0];
    expect(card?.classList.contains("replaced")).toBe(true);
    expect(card?.querySelector(".search-card-replaced-badge")?.textContent).toBe("✓ replaced");
    // The second match is untouched and still pending.
    expect(h.sources.get("a.ink")).toContain("No torch here.");
    expect(cardEls()[1]?.querySelector(".search-card-accept")).not.toBeNull();
  });

  it("skip badges the card and undo-skip restores it", () => {
    const h = mountList({ "a.ink": DOC }, (store) => {
      seedQuery(store);
      store.getState().setSearchReplaceOpen(true);
      store.getState().setSearchReplace("lantern");
    });
    act(() => cardEls()[0]?.querySelector<HTMLButtonElement>(".search-card-skip")?.click());
    const card = cardEls()[0];
    expect(card?.classList.contains("skipped")).toBe(true);
    expect(card?.querySelector(".search-card-badge.skipped-badge")).not.toBeNull();
    act(() =>
      cardEls()[0]?.querySelector<HTMLButtonElement>(".search-card-undo-skip")?.click(),
    );
    expect(cardEls()[0]?.querySelector(".search-card-accept")).not.toBeNull();
  });

  it("references-mode cards never preview (replace is inert there)", () => {
    mountList({ "a.ink": DOC }, (store) => {
      const at = DOC.indexOf("torch");
      store.getState().showReferences("torch", [{ file: "a.ink", start: at, end: at + 5 }]);
      store.getState().setSearchReplaceOpen(true);
      store.getState().setSearchReplace("lantern");
    });
    const card = cardEls()[0];
    expect(card?.querySelector(".search-card-del")).toBeNull();
    expect(card?.querySelector(".search-card-accept")).toBeNull();
    expect(card?.querySelector(".search-card-editor")).not.toBeNull();
  });

  it("references snapshots render through the same cards", () => {
    mountList({ "a.ink": DOC }, (store) => {
      const at = DOC.indexOf("torch");
      store
        .getState()
        .showReferences("torch", [{ file: "a.ink", start: at, end: at + 5 }]);
    });
    const cards = cardEls();
    expect(cards).toHaveLength(1);
    expect(cards[0]?.querySelector(".search-card-loc")?.textContent).toBe("a.ink:2");
  });
});
