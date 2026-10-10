/**
 * Settings ▸ Project ▸ Cast (decision log 2026-10-10): `[cast]` in
 * `brink.toml`, one colour per speaker. Driven through the real component
 * and store; the assertions are on the text written to the file, which is
 * what the Player reads back.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { CastSettings, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "brink.toml", symbols: [], mounted: false },
  { path: "main.ink", symbols: [], mounted: false },
];

const CONFIG = `[project]
entry = "main.ink"

[cast.Mara]
color = "#d97757" # warm
`;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

async function mount(initial: string | null = CONFIG) {
  let source = initial;
  const project = {
    getSession: () => ({
      getFileSource: (p: string) => (p === "brink.toml" ? source : null),
    }),
    applyEdit: (_path: string, next: string) => {
      source = next;
      return true;
    },
  };
  const store = createStudioStore();
  const outline = initial === null ? OUTLINE.filter((f) => f.path !== "brink.toml") : OUTLINE;
  store.getState().setCompileResult(outline, { errors: 0, warnings: 0 }, [], null);
  store.setState({
    _project: project as never,
    _documents: { refreshExternal: vi.fn(), triggerCompile: vi.fn() } as never,
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root!.render(createElement(StoreProvider, { store, children: createElement(CastSettings) }));
  });
  return { current: () => source, el: container };
}

/** React suppresses a bypassed setter; see `prose-dictionary.test.tsx`. */
function setValue(input: HTMLInputElement, value: string, event: "input" | "change"): void {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event(event, { bubbles: true }));
}

const rowTitles = (el: HTMLElement): string[] =>
  Array.from(el.querySelectorAll(".settings-row-title")).map((t) => t.textContent ?? "");

describe("Settings ▸ Cast", () => {
  it("lists the declared speakers with the colour the file holds", async () => {
    const { el } = await mount();
    expect(rowTitles(el)).toEqual(["Mara"]);
    expect(el.querySelector(".settings-row code")?.textContent).toBe("#d97757");
    const well = el.querySelector<HTMLInputElement>('input[aria-label="Colour for Mara"]')!;
    expect(well.value).toBe("#d97757");
  });

  it("writes a picked colour only when the pick is committed, keeping the comment", async () => {
    const { el, current } = await mount();
    const well = el.querySelector<HTMLInputElement>('input[aria-label="Colour for Mara"]')!;
    await act(async () => setValue(well, "#123456", "input"));
    // Dragging the picker writes nothing.
    expect(current()).toBe(CONFIG);
    await act(async () => setValue(well, "#123456", "change"));
    expect(current()).toContain('[cast.Mara]\ncolor = "#123456" # warm');
  });

  it("adds a speaker, refuses one already cast in another case, and removes", async () => {
    const { el, current } = await mount();
    const name = el.querySelector<HTMLInputElement>('input[aria-label="Add a speaker"]')!;
    const add = Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "Add")!;

    await act(async () => setValue(name, "MARA", "input"));
    expect(add.disabled).toBe(true);
    expect(el.querySelector(".cast-add-problem")?.textContent).toContain("already in the cast");

    await act(async () => setValue(name, "Old Tom", "input"));
    await act(async () => add.click());
    expect(current()).toContain('[cast."Old Tom"]\ncolor = "#5b8def"');
    expect(rowTitles(el)).toEqual(["Mara", "Old Tom"]);

    const remove = el.querySelector<HTMLButtonElement>('button[aria-label="Remove Mara from the cast"]')!;
    await act(async () => remove.click());
    expect(current()).not.toContain("[cast.Mara]");
    expect(rowTitles(el)).toEqual(["Old Tom"]);
  });

  it("says so when there is no brink.toml to write to", async () => {
    const { el } = await mount(null);
    expect(el.querySelector(".settings-empty")?.textContent).toContain("no brink.toml");
  });
});
