/**
 * Inline creation (#3039 — compare `docs/design/binder-v2/CreateRow.dc.html`):
 * the 50/50 icon-button groups per container + root, the in-place name
 * input (bare name, .ink implied), inline validation, and folder creation
 * through the sidecar's empty-folder registry.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "main.ink", symbols: [], mounted: false },
  { path: "scenes/harbour.ink", symbols: [], mounted: false },
];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function seeded() {
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
  const addFile = vi.fn(() => Promise.resolve());
  store.setState({ addFile: addFile as never });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(Binder) }));
  });
  return { store, addFile };
}

function openRootCreate(kind: "New file" | "New folder"): HTMLInputElement {
  act(() => {
    container!
      .querySelector<HTMLButtonElement>(
        `.brink-create-group.big .brink-create-btn[title='${kind}']`,
      )
      ?.click();
  });
  const input = container!.querySelector<HTMLInputElement>(".brink-create-input");
  if (input === null) throw new Error("create input did not open");
  return input;
}

function commit(input: HTMLInputElement, value: string): void {
  act(() => {
    // React reads the value through its own tracker; set + input event.
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    setter?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
}

describe("Binder inline creation (#3039)", () => {
  it("root New file commits a bare name with .ink implied", () => {
    const { addFile } = seeded();
    commit(openRootCreate("New file"), "offcuts");
    expect(addFile).toHaveBeenCalledWith("offcuts.ink");
    // The input closed back to the idle group.
    expect(container!.querySelector(".brink-create-input")).toBeNull();
  });

  it("root New folder registers an empty folder that renders immediately", () => {
    const { store } = seeded();
    commit(openRootCreate("New folder"), "drafts");
    expect(store.getState().binderOrder.folders).toContain("drafts/");
    const keys = [...container!.querySelectorAll("[data-binder-row-key]")].map((el) =>
      el.getAttribute("data-binder-row-key"),
    );
    expect(keys).toContain("drafts/");
  });

  it("a duplicate name errors inline and keeps the input open", () => {
    const { addFile } = seeded();
    const input = openRootCreate("New file");
    commit(input, "main");
    expect(addFile).not.toHaveBeenCalled();
    expect(container!.querySelector(".brink-create-error")?.textContent).toContain(
      "already exists",
    );
    expect(container!.querySelector(".brink-create-input")).not.toBeNull();
  });

  it("a path-y name is rejected with a reason", () => {
    const { addFile } = seeded();
    commit(openRootCreate("New file"), "a/b");
    expect(addFile).not.toHaveBeenCalled();
    expect(container!.querySelector(".brink-create-error")?.textContent).toContain(
      "bare name",
    );
  });

  it("a folder's context menu opens an input that creates INTO that folder", () => {
    // The per-container idle buttons were removed after live use
    // (maintainer: noisy) — the context menu is the folder-scoped door,
    // and the input still renders in place inside the folder.
    const { addFile } = seeded();
    act(() => {
      container!
        .querySelector<HTMLElement>(".brink-binder-folder-row")
        ?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    });
    const item = [...container!.querySelectorAll(".brink-context-menu-item")].find(
      (el) => el.textContent === "New file here",
    );
    if (item === undefined) throw new Error("no 'New file here' item");
    act(() => {
      (item as HTMLElement).click();
    });
    const editing = container!.querySelector<HTMLInputElement>(
      "[data-create-container='scenes/'] .brink-create-input, .brink-create-editing[data-create-container='scenes/'] .brink-create-input",
    );
    const input = editing ?? container!.querySelector<HTMLInputElement>(".brink-create-input");
    if (input === null) throw new Error("folder create input did not open");
    commit(input, "docks");
    expect(addFile).toHaveBeenCalledWith("scenes/docks.ink");
  });

  it("no idle create buttons render inside folders — only the root group", () => {
    seeded();
    expect(
      container!.querySelectorAll(".brink-create-group:not(.big)"),
    ).toHaveLength(0);
    expect(container!.querySelector(".brink-create-group.big")).not.toBeNull();
  });
});
