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
 * The first two tests below fail against the `requestAnimationFrame` seeding
 * and pass once the value is seeded synchronously at mount. The third guards
 * a sibling defect in the same callback: the deferred `focus()`/`select()`
 * pair used to run unconditionally, so `select()` clobbered the caret of a
 * field the user had already typed into — the same defect class, one step
 * later. It fails unless `select()` is skipped once the field no longer
 * holds the seeded name.
 *
 * The fourth is a preservation guard added by #2580's deletion-mutation
 * audit. The first three are all *negative* about the selection — they say
 * where it must NOT go — so deleting `SymbolRenamePrompt`'s `input.select()`
 * outright (rather than un-guarding it) left this whole suite green, i.e. the
 * call site had no assertion that could go red for it. That is the same
 * vacuity `binder-seed-race.test.tsx` closes with its two "preservation
 * guard" tests. The fourth test pins the positive half: an untouched prompt
 * comes back with the whole seeded name selected, so typing replaces it.
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

  it("leaves the selection alone on a field the user has already typed into", async () => {
    renderPrompt("barter");

    // Same window as above, but this time the typist also moves the caret,
    // as a real keystroke would — the deferred callback must not know or
    // care where the caret lands.
    const el = input();
    el.value = "haggle";
    el.setSelectionRange(3, 3);
    await flushFrames();

    // Before the fix, the deferred frame called `input.select()`
    // unconditionally, which would move the selection to the whole value
    // (0, "haggle".length) and prime the next keystroke to replace it —
    // the same defect class this PR closes, one step later in the same
    // callback. `select()` must run only when the field still holds the
    // seeded name.
    expect(el.selectionStart).toBe(3);
    expect(el.selectionEnd).toBe(3);
  });

  it("still selects the seeded name on an untouched field", async () => {
    renderPrompt("barter");
    const el = input();

    // A write to `.value` parks the caret at the end of the value, and React
    // seeds an uncontrolled field by writing that property — so this input
    // arrives here already reading (6, 6). The park cannot fake
    // `selectionStart === 0`, so this test would go red for a deleted
    // `select()` either way — but it *can* hand `selectionEnd === 6` over for
    // free, so without this reset half the pair below would be unearned. Same
    // trap PR #2574 hit in `binder-seed-race.test.tsx`, where an end-caret
    // assertion made it vacuous outright.
    //
    // The reset is a vacuity guard only (#2595). It does NOT restore "what a
    // real browser would show": (0, 0) is what the seed leaves, before the
    // prompt's own `select()` runs. This component's rAF effect calls
    // `input.select()` while the field still holds the seeded name
    // (`SymbolRenamePrompt.tsx`, the SELECT-INVARIANT site this suite
    // guards), so an author actually faces the fully-selected (0, 6) reading
    // asserted below — neither (0, 0) nor the unguarded (6, 6) this reset
    // undoes. Measured on the real `#brink-rename-input` in Chromium 145 by the
    // e2e "a defaultValue-seeded field parks the caret at the end in a real
    // browser" (`e2e/symbol-rename.spec.ts`) — the park is HTML-standard
    // behaviour for the `.value` setter, which jsdom reproduces faithfully.
    // The (0, 0) reading belongs to the `value` ATTRIBUTE path, which React
    // does not take; the earlier comment here asserted the opposite and was
    // never observed in a browser.
    el.setSelectionRange(0, 0);

    await flushFrames();

    // Preservation guard (#2580): the deferred frame's whole positive purpose
    // is that an untouched prompt is ready to be typed over. Without this,
    // deleting `input.select()` from `SymbolRenamePrompt.tsx` left every
    // other test in this file green — measured in the deletion-mutation
    // audit. Now it goes red here.
    expect(el.value).toBe("barter");
    expect(el.selectionStart).toBe(0);
    expect(el.selectionEnd).toBe("barter".length);

    // B5 (#2580 follow-up): `input.focus()` sits directly above the guarded
    // `select()` in the same deferred frame and was pinned by nothing —
    // deleting it left this whole suite green, this test included, while
    // the rename prompt opened unfocused and the author's first keystroke
    // went nowhere. `search-view-focus.test.tsx` (A1) and
    // `inline-name-input-seed.test.ts` (C3) already assert
    // `document.activeElement`; this was the one sibling suite missing it.
    expect(document.activeElement).toBe(el);
  });
});
