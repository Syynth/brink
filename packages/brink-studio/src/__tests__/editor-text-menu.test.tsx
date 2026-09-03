/**
 * The editor text context menu (docs/editor-context-menu-spec.md): the store
 * request renders Cut/Copy/Paste/Select All with the editor-bound actions,
 * Cut/Copy disable without a selection, a click runs the action and closes,
 * and the symbol menu and text menu are mutually exclusive.
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
} from "@brink/studio-shell";
import { createStudioStore, type EditorTextMenuRequest } from "@brink/studio-store";
import { EditorTextMenuHost, StoreProvider } from "@brink/studio-ui";

let root: Root | null = null;
let lastCommands: CommandRegistry | null = null;
let container: HTMLElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  lastCommands = null;
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
  const commands = new CommandRegistry();
  lastCommands = commands;
  const themes = new ThemeService();
  const overrides = new KeymapOverridesService();
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands, themes, keymapOverrides: overrides, isMac: true } as never,
        createElement(StoreProvider, { store } as never, createElement(EditorTextMenuHost)),
      ),
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
  it("offers Break on Write for a known-global identity (W18 follow-up)", () => {
    const store = mount();
    store.setState({
      programModel: {
        checksum: "0x1",
        globals: [{ name: "gold", ty: "int", default: "2", mutable: true }],
        lists: [],
        externals: [],
        knots: [],
      } as never,
    });
    // A global identifier: the verb appears and toggles the slice.
    act(() =>
      store.getState().openTextMenu({
        ...request(false),
        identity: { name: "gold", gotoDefinition: vi.fn() },
      }),
    );
    let entry = items().find((i) => i.label.includes("Break on Write 'gold'"));
    expect(entry, "the verb appears for a known global").toBeDefined();
    act(() => {
      [...container!.querySelectorAll(".brink-context-menu-item")]
        .find((el) => el.textContent?.includes("Break on Write 'gold'"))
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(store.getState().dataBreakpoints).toEqual([{ name: "gold", enabled: true }]);

    // Re-open: already watched → the Remove form.
    act(() =>
      store.getState().openTextMenu({
        ...request(false),
        identity: { name: "gold", gotoDefinition: vi.fn() },
      }),
    );
    entry = items().find((i) => i.label.includes("Remove Break on Write 'gold'"));
    expect(entry, "watched → Remove form").toBeDefined();
    act(() => store.getState().closeTextMenu());

    // A NON-global identity (a knot name): no verb.
    act(() =>
      store.getState().openTextMenu({
        ...request(false),
        identity: { name: "some_knot", gotoDefinition: vi.fn() },
      }),
    );
    expect(items().some((i) => i.label.includes("Break on Write"))).toBe(false);
  });


  it("renders nothing until a request opens it", () => {
    mount();
    expect(container!.querySelector(".brink-text-menu")).toBeNull();
  });

  it("does NOT hijack Escape while closed (the four-E2E-red regression)", () => {
    // The dismiss contract must mount WITH the menu: an always-mounted
    // capture-phase Escape listener swallowed drag-cancel, maximize
    // restore, and keymap defaults across the app.
    mount();
    const ev = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    act(() => {
      document.dispatchEvent(ev);
    });
    expect(ev.defaultPrevented).toBe(false);
  });

  it("Escape closes an open menu", () => {
    const store = mount();
    act(() => store.getState().openTextMenu(request(false)));
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    });
    expect(store.getState().textMenu).toBeNull();
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
      "Hide Gutters",
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

  it("identity group renders in spec order above the text group; gaps collapse", () => {
    const store = mount();
    const req = request(false);
    const gotoDef = vi.fn();
    const rename = vi.fn();
    act(() =>
      store.getState().openTextMenu({
        ...req,
        identity: { name: "gold", gotoDefinition: gotoDef, rename },
      }),
    );
    // findReferences absent -> omitted; order: Navigate, Rename, then text.
    expect(items().map((i) => i.label)).toEqual([
      "Go to Definition⌘Click",
      "Rename 'gold'…F2",
      "Cut⌘X",
      "Copy⌘C",
      "Paste⌘V",
      "Select All⌘A",
      "Hide Gutters",
    ]);
    const g = [...container!.querySelectorAll(".brink-context-menu-item")].find((el) =>
      el.textContent?.startsWith("Go to Definition"),
    )!;
    act(() => {
      g.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(gotoDef).toHaveBeenCalledOnce();
    expect(container!.querySelector(".brink-text-menu")).toBeNull();
  });

  // Adversarial review on PR #3454 (finding 2): the fix group
  // (`docs/autofix-spec.md` §7, `EditorTextMenuHost`'s `fixItems`) shipped
  // with no pin on its ordering or its "Fix all safe in this file" trailer
  // — deleting the `fixItems` block turned nothing red. Placed alongside the
  // identity-ordering test above, which already pins this component's group
  // ordering the same way.
  it("fix entries render above the identity group, and the trailer dispatches fix.allSafeInFile", () => {
    const store = mount();
    const runFix = vi.fn();
    const dispatchSpy = vi.spyOn(lastCommands!, "dispatch");
    act(() =>
      store.getState().openTextMenu({
        ...request(false),
        identity: { name: "gold", gotoDefinition: vi.fn() },
        fixActions: [
          {
            label: "Import `haggle` from `story::market::barter`",
            code: "E025",
            tier: "suggested",
            run: runFix,
          },
        ],
      }),
    );
    expect(items().map((i) => i.label)).toEqual([
      "Import `haggle` from `story::market::barter` — Suggested",
      "Fix all safe in this file",
      "Go to Definition⌘Click",
      "Cut⌘X",
      "Copy⌘C",
      "Paste⌘V",
      "Select All⌘A",
      "Hide Gutters",
    ]);

    const els = [...container!.querySelectorAll(".brink-context-menu-item")];
    const fixEl = els.find((el) => el.textContent?.startsWith("Import `haggle`"))!;
    act(() => fixEl.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(runFix).toHaveBeenCalledOnce();

    // Re-open: clicking a stale element from the closed menu is invalid, so
    // reopen fresh before exercising the trailer.
    act(() =>
      store.getState().openTextMenu({
        ...request(false),
        fixActions: [
          {
            label: "Import `haggle` from `story::market::barter`",
            code: "E025",
            tier: "suggested",
            run: runFix,
          },
        ],
      }),
    );
    const trailer = [...container!.querySelectorAll(".brink-context-menu-item")].find(
      (el) => el.textContent === "Fix all safe in this file",
    )!;
    act(() => trailer.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(dispatchSpy).toHaveBeenCalledWith("fix.allSafeInFile");
  });

  it("no fix entries and no trailer when the diagnostic under the pointer has no offered fix", () => {
    const store = mount();
    act(() =>
      store.getState().openTextMenu({
        ...request(false),
        identity: { name: "gold", gotoDefinition: vi.fn() },
        fixActions: [],
      }),
    );
    expect(items().map((i) => i.label)).not.toContain("Fix all safe in this file");
    expect(items().map((i) => i.label)[0]).toBe("Go to Definition⌘Click");
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
