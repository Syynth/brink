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

/**
 * Press a key the way a real one arrives: dispatched at `window`.
 *
 * The first version of these tests dispatched at the capture field, which
 * is what let the bug through — the field never had focus (React's
 * `autoFocus` is a no-op on a div), so in the real studio no key ever
 * reached it. Escape fell through to the Settings modal's dismisser and
 * closed the whole dialog; Enter fell through to nothing.
 */
const press = (init: KeyboardEventInit): void => {
  act(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
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
    press({ key: "k", metaKey: true });
    expect(container!.querySelector(".keymap-chord.is-capturing")).not.toBeNull();
    press({ key: "Enter" });
    expect(overrides.current["search.symbol"]).toContain("Mod-K");
  });

  it("names the command a chord would be taken from, before saving", () => {
    const { overrides } = render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    press({
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
    press({ key: "f", metaKey: true, shiftKey: true });
    press({ key: "Enter" });

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
    press({ key: "k", metaKey: true });
    press({ key: "Escape" });
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
    press({ key: "k", metaKey: true });
    press({ key: "Enter" });
    expect(row("Find symbol").querySelector(".keymap-source")!.textContent).toBe("Custom");

    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-reset")!.click();
    });
    expect(overrides.current["search.symbol"]).toBeUndefined();
    expect(row("Find symbol").querySelector(".keymap-source")!.textContent).toBe("Default");
  });

  it("swallows the keys it records, so nothing else acts on them", () => {
    // The bug this replaces: the capture field relied on React `autoFocus`,
    // which does nothing on a div, so every key went to the document
    // instead. Escape reached the Settings modal's dismisser and closed the
    // whole dialog; Enter reached nothing. Recording must own the keyboard —
    // and a chord being recorded must not also FIRE the command it is
    // currently bound to.
    render();
    const seen: string[] = [];
    const bystander = (e: Event) => seen.push((e as KeyboardEvent).key);
    window.addEventListener("keydown", bystander);
    const startCapture = () =>
      act(() => {
        row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
      });
    try {
      // Each control ENDS the recording, so each needs its own session —
      // keys arriving after one are legitimately not the recorder's.
      startCapture();
      press({ key: "Escape" });
      expect(seen, "Escape leaked to the Settings dismisser").toEqual([]);

      startCapture();
      press({ key: "f", metaKey: true, shiftKey: true });
      expect(seen, "a recorded chord leaked and could fire its command").toEqual([]);
      press({ key: "Enter" });
      expect(seen, "Enter leaked while recording").toEqual([]);

      // Recording over: keys reach the rest of the app again.
      press({ key: "Escape" });
      expect(seen).toEqual(["Escape"]);
    } finally {
      window.removeEventListener("keydown", bystander);
    }
  });

  it("records a modified Enter or Escape instead of treating it as a control", () => {
    // Only BARE Enter/Escape drive the recorder, so `Alt-Enter` stays
    // bindable rather than being swallowed as "save".
    const { overrides } = render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    press({ key: "Enter", altKey: true });
    expect(container!.querySelector(".keymap-chord.is-capturing")?.textContent).toContain("Enter");
    press({ key: "Enter" });
    expect(overrides.current["search.symbol"]).toContain("Alt-Enter");
  });

  it("ignores an event that reports no key", () => {
    // `chordFromEvent` only rejects bare MODIFIER presses; `key: ""` yields
    // a chord with an empty key, which renders as a blank chip and cannot
    // round-trip — `parseKeybinding` rejects it on the way back in.
    render();
    act(() => {
      row("Find symbol").querySelector<HTMLButtonElement>(".keymap-add")!.click();
    });
    press({ key: "" });
    expect(container!.querySelector(".keymap-chord.is-capturing")).toBeNull();
    expect(container!.querySelector(".keymap-capture-hint")?.textContent).toContain("Press keys");
  });
});
