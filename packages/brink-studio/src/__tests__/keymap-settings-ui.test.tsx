/**
 * Settings ▸ Keymap, the table.
 *
 * These cover the parts a model test cannot reach: that a key PRESS is what
 * records the binding (not typed text), that the conflict is stated before
 * the save rather than discovered after it, and that the live keymap
 * actually changes — the last one being the point of the whole surface.
 */
import { afterEach, describe, expect, it } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  Keymap,
  KeymapOverridesService,
  ShellProvider,
  chordId,
  parseKeybinding,
  useShell,
} from "@brink/studio-shell";
import { KeymapSettings } from "@brink/studio-ui";

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function memoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
  };
}

let latest: Keymap | null = null;
function KeymapProbe() {
  latest = useShell().keymap;
  return null;
}

function render() {
  const commands = new CommandRegistry();
  commands.register({
    id: "search.find",
    title: "Search: Find in files",
    keybinding: "Mod-Shift-F",
    run: () => {},
  });
  commands.register({
    id: "search.symbol",
    title: "Search: Find symbol",
    keybinding: "Mod-T",
    run: () => {},
  });
  const overrides = new KeymapOverridesService(memoryStorage());

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands, keymapOverrides: overrides, isMac: true } as never,
        createElement(KeymapSettings) as ReactNode,
        createElement(KeymapProbe),
      ),
    );
  });
  return { commands, overrides };
}

/** The row whose command name is `name`. */
function row(name: string): HTMLElement {
  const found = [...container!.querySelectorAll(".keymap-row")].find((el) =>
    el.querySelector(".keymap-name")?.textContent?.includes(name),
  );
  expect(found, `no row for ${name}`).toBeDefined();
  return found as HTMLElement;
}

const press = (el: Element, init: KeyboardEventInit): void => {
  act(() => {
    el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
  });
};

describe("the keymap table", () => {
  it("lists commands with their bindings, grouped by category", () => {
    render();
    expect(container!.querySelector(".keymap-heading")?.textContent).toBe("Search");
    expect(row("Find in files").querySelector(".keymap-chord")?.textContent).toContain("F");
    expect(row("Find in files").querySelector(".keymap-source")?.textContent).toBe("Default");
  });

  it("records a binding from a key PRESS, not typed text", () => {
    // The whole reason for capture: `chordFromEvent` is what dispatch uses,
    // so what the author pressed is what will fire.
    const { overrides } = render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    const field = container!.querySelector(".keymap-capture")!;
    press(field, { key: "k", metaKey: true });
    expect(container!.querySelector(".keymap-chord.is-capturing")).not.toBeNull();
    press(field, { key: "Enter" });
    expect(overrides.current["search.symbol"]).toContain("Mod-K");
  });

  it("names the command a chord would be taken from, before saving", () => {
    const { overrides } = render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    press(container!.querySelector(".keymap-capture")!, {
      key: "f",
      metaKey: true,
      shiftKey: true,
    });
    const warning = container!.querySelector(".keymap-conflict");
    expect(warning).not.toBeNull();
    expect(warning!.textContent).toContain("Find in files");
    // Stated BEFORE the commit — nothing is written until Enter.
    expect(overrides.current).toEqual({});
  });

  it("displaces the previous owner on save, and the live keymap follows", () => {
    const { overrides } = render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    const field = container!.querySelector(".keymap-capture")!;
    press(field, { key: "f", metaKey: true, shiftKey: true });
    press(field, { key: "Enter" });

    expect(overrides.current["search.find"]).toBeNull();
    const chord = parseKeybinding("Mod-Shift-F")!;
    // The resolution table the key handler actually consults.
    expect(latest!.resolveChord(chord)).toBe("search.symbol");
    expect(chordId(chord)).toBe("mod+shift+f");
  });

  it("cancels on Escape without writing", () => {
    const { overrides } = render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    const field = container!.querySelector(".keymap-capture")!;
    press(field, { key: "k", metaKey: true });
    press(field, { key: "Escape" });
    expect(container!.querySelector(".keymap-capture")).toBeNull();
    expect(overrides.current).toEqual({});
  });

  it("filters by rendered key, so what is printed on the keys finds the row", () => {
    render();
    const search = container!.querySelector<HTMLInputElement>(".keymap-search")!;
    act(() => {
      container!.querySelector<HTMLButtonElement>(".keymap-bykey")!.click();
    });
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    act(() => {
      setter.call(search, "⌘T");
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const names = [...container!.querySelectorAll(".keymap-name")].map((n) => n.textContent);
    expect(names).toEqual(["Find symbol"]);
  });

  it("resets a customised row back to its default", () => {
    const { overrides } = render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    const field = container!.querySelector(".keymap-capture")!;
    press(field, { key: "k", metaKey: true });
    press(field, { key: "Enter" });
    expect(row("Find symbol").querySelector(".keymap-source")!.textContent).toBe("Custom");

    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-reset")!.click();
    });
    expect(overrides.current["search.symbol"]).toBeUndefined();
    expect(row("Find symbol").querySelector(".keymap-source")!.textContent).toBe("Default");
  });
});
