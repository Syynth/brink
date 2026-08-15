/**
 * `SymbolRenamePrompt` seeding (#2511) — the modal rename input must hold the
 * symbol's current name from its very first mounted frame.
 *
 * The prompt used to seed the input from inside a `requestAnimationFrame`
 * callback scheduled by its open-effect. That left a window in which the input
 * was mounted, visible and editable but still empty, and — worse — anything
 * typed into it during that window was overwritten when the frame finally ran.
 * Because the field is uncontrolled and `confirmName()` reads `input.value`,
 * a clobbered rename silently degrades to `name === currentName`, which closes
 * the prompt without renaming anything. That is the mechanism behind the
 * `e2e/symbol-rename.spec.ts` flake: the prompt closes (so the e2e wait on
 * `#brink-rename-input` being hidden succeeds) but the binder never shows the
 * new name.
 *
 * Both tests below fail against the `requestAnimationFrame` seeding and pass
 * once the value is seeded synchronously at mount.
 */

import { describe, expect, it, afterEach } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  BUILTIN_THEMES,
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
} from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import { StoreProvider, SymbolRenamePrompt } from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => void map.delete(key),
    setItem: (key: string, value: string) => void map.set(key, value),
  } as Storage;
}

let container: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  container = null;
  root = null;
});

/** Mount the prompt with an open rename request for `currentName`. */
function renderPrompt(currentName: string): { store: StudioStore } {
  const store = createStudioStore();
  store.getState().openRenamePrompt({ path: "main.ink", knot: currentName, currentName });

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);

  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        {
          commands: new CommandRegistry(),
          themes: new ThemeService(BUILTIN_THEMES, memoryStorage()),
          keymapOverrides: new KeymapOverridesService(memoryStorage()),
          isMac: false,
        } as never,
        createElement(StoreProvider, { store } as never, createElement(
          SymbolRenamePrompt,
        ) as ReactNode),
      ),
    );
  });

  return { store };
}

function input(): HTMLInputElement {
  const el = container?.querySelector("#brink-rename-input");
  expect(el, "the rename prompt mounted its name input").toBeInstanceOf(HTMLInputElement);
  return el as HTMLInputElement;
}

/** Run any pending `requestAnimationFrame` callbacks. */
async function flushFrames(): Promise<void> {
  await act(async () => {
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
  });
}

describe("SymbolRenamePrompt seeding (#2511)", () => {
  it("holds the current name on the first mounted frame", () => {
    renderPrompt("barter");

    // No frame has been allowed to run: the value must already be there, so a
    // consumer that sees the input at all sees it correctly seeded.
    expect(input().value).toBe("barter");
  });

  it("does not overwrite a name typed before the first frame runs", async () => {
    renderPrompt("barter");

    // Type into the freshly mounted input, exactly as a fast typist (or
    // Playwright's `fill`) does, before any animation frame has run.
    input().value = "haggle";
    await flushFrames();

    // The seeding must not have clobbered the typed name — otherwise
    // `confirmName()` reads "barter", sees `name === currentName`, and closes
    // the prompt without performing a rename.
    expect(input().value).toBe("haggle");
  });
});
