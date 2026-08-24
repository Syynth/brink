/**
 * The order sidecar's rendered half (#3038): the Binder honors
 * `.binder.json` display order (files and folders interleaved), renders
 * registered EMPTY folders, and a reorder action persists through the
 * host seam.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider } from "@brink/studio-ui";
import { createStudioStore, parseBinderOrder } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "a.ink", symbols: [], mounted: false },
  { path: "codetta.ink", symbols: [], mounted: false },
  { path: "menus/title.ink", symbols: [], mounted: false },
];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mountBinder(store: ReturnType<typeof createStudioStore>) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(Binder) }));
  });
}

function topLevelKeys(): string[] {
  return [...container!.querySelectorAll("[data-binder-row-key]")]
    .map((el) => el.getAttribute("data-binder-row-key") ?? "")
    .filter((k) => !k.includes("::"));
}

describe("Binder order sidecar rendering (#3038)", () => {
  it("interleaves files and folders per the sidecar; unlisted follow the fallback", () => {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
    store.getState().setEntryFile("codetta.ink");
    store
      .getState()
      .setBinderOrder(parseBinderOrder('{"order": {"": ["a.ink", "menus/"]}, "folders": []}'));
    mountBinder(store);
    const keys = topLevelKeys();
    // Listed order first (a file BEFORE a folder — authored placement),
    // then the fallback (entry codetta.ink).
    expect(keys.indexOf("a.ink")).toBeLessThan(keys.indexOf("menus/"));
    expect(keys.indexOf("menus/")).toBeLessThan(keys.indexOf("codetta.ink"));
  });

  it("renders a registered empty folder", () => {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
    store
      .getState()
      .setBinderOrder(parseBinderOrder('{"order": {}, "folders": ["drafts/"]}'));
    mountBinder(store);
    expect(topLevelKeys()).toContain("drafts/");
  });

  it("reorderBinderSiblings updates state and persists through the host seam", () => {
    const store = createStudioStore();
    const persisted: string[] = [];
    store.setState({
      _persistBinderOrder: (text: string) => {
        persisted.push(text);
        return Promise.resolve();
      },
    });
    store.getState().reorderBinderSiblings("", ["b.ink", "a.ink"]);
    expect(store.getState().binderOrder.order[""]).toEqual(["b.ink", "a.ink"]);
    expect(persisted).toHaveLength(1);
    expect(JSON.parse(persisted[0] ?? "{}")).toEqual({
      order: { "": ["b.ink", "a.ink"] },
      folders: [],
    });
  });

  it("rekey and remove keep the sidecar aligned with file ops", () => {
    const store = createStudioStore();
    store.getState().setBinderOrder(parseBinderOrder('{"order": {"": ["old.ink"]}, "folders": []}'));
    store.getState().rekeyBinderPaths("old.ink", "new.ink");
    expect(store.getState().binderOrder.order[""]).toEqual(["new.ink"]);
    store.getState().removeBinderPath("new.ink");
    expect(store.getState().binderOrder.order[""]).toEqual([]);
  });
});
