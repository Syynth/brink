/**
 * The editor text context menu (docs/editor-context-menu-spec.md): the store
 * request renders Cut/Copy/Paste/Select All with the editor-bound actions,
 * Cut/Copy disable without a selection, a click runs the action and closes,
 * and the symbol menu and text menu are mutually exclusive.
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createStudioStore, type EditorTextMenuRequest } from "@brink/studio-store";
import { EditorTextMenuHost, StoreProvider } from "@brink/studio-ui";

let root: Root | null = null;
let container: HTMLElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function request(hasSelection: boolean): EditorTextMenuRequest & {
  spies: Record<string, ReturnType<typeof vi.fn>>;
} {
  const spies = { cut: vi.fn(), copy: vi.fn(), paste: vi.fn(), selectAll: vi.fn() };
  return { x: 40, y: 60, hasSelection, ...spies, spies };
}

function mount() {
  const store = createStudioStore();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(StoreProvider, { store } as never, createElement(EditorTextMenuHost)),
    );
  });
  return store;
}

function items(): { label: string; disabled: boolean }[] {
  return [...container!.querySelectorAll(".brink-context-menu-item")].map((el) => ({
    label: el.textContent ?? "",
    disabled: el.className.includes("is-disabled"),
  }));
}

describe("EditorTextMenuHost", () => {
  it("renders nothing until a request opens it", () => {
    mount();
    expect(container!.querySelector(".brink-text-menu")).toBeNull();
  });

  it("disables Cut/Copy without a selection; click runs the action and closes", () => {
    const store = mount();
    const req = request(false);
    act(() => store.getState().openTextMenu(req));

    expect(items().map((i) => `${i.label}${i.disabled ? "!" : ""}`)).toEqual([
      "Cut⌘X!",
      "Copy⌘C!",
      "Paste⌘V",
      "Select All⌘A",
    ]);

    const paste = [...container!.querySelectorAll(".brink-context-menu-item")].find((el) =>
      el.textContent?.startsWith("Paste"),
    )!;
    act(() => {
      paste.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(req.spies.paste).toHaveBeenCalledOnce();
    expect(container!.querySelector(".brink-text-menu")).toBeNull();
  });

  it("a disabled item neither runs nor closes", () => {
    const store = mount();
    const req = request(false);
    act(() => store.getState().openTextMenu(req));
    const cut = [...container!.querySelectorAll(".brink-context-menu-item")].find((el) =>
      el.textContent?.startsWith("Cut"),
    )!;
    act(() => {
      cut.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(req.spies.cut).not.toHaveBeenCalled();
    expect(container!.querySelector(".brink-text-menu")).not.toBeNull();
  });

  it("enables Cut/Copy with a selection", () => {
    const store = mount();
    act(() => store.getState().openTextMenu(request(true)));
    expect(items().every((i) => !i.disabled)).toBe(true);
  });

  it("symbol menu and text menu are mutually exclusive in the store", () => {
    const store = mount();
    act(() => store.getState().openTextMenu(request(false)));
    act(() =>
      store.getState().openSymbolMenu({ path: "a.ink", knot: "k", x: 0, y: 0, source: "editor" }),
    );
    expect(store.getState().textMenu).toBeNull();
    act(() => store.getState().openTextMenu(request(false)));
    expect(store.getState().symbolMenu).toBeNull();
  });
});
