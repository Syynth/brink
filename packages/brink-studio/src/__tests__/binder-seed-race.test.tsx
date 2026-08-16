/**
 * `Binder.tsx`'s two §7.7.1 select-call sites, proven behaviourally (#2571
 * gap 3).
 *
 * `select-call-enrolment.test.ts` (#2542 / PR #2565) proves every
 * `.select()` / `.setSelectionRange(` call site in the workspace *carries* a
 * `SELECT-INVARIANT` marker. It cannot prove the marker's justification is
 * TRUE — the same hard gap #2515 left open for the `SAVE-PATH` markers.
 * `SearchView`, `SymbolRenamePrompt` and `InlineNameInput` each close that
 * gap with a behavioural seed-race test (`search-view-focus.test.tsx`,
 * `symbol-rename-prompt-seed.test.tsx`, `inline-name-input-seed.test.ts`).
 * `Binder.tsx`'s two sites had prose alone. This file is their test, and it
 * mirrors `inline-name-input-seed.test.ts`'s shape: race the seed with a
 * user keystroke, then assert the user's text survives.
 *
 * The two sites make DIFFERENT claims, so they get different tests:
 *
 *  - `Binder.renameInput.preSelectBasename` (`Binder.tsx` in `RenameInput`)
 *    claims there is *no deferred window at all*: the `setSelectionRange`
 *    runs synchronously in the mount effect. A synchronous call cannot be
 *    raced by a keystroke, so typing "before it lands" is not a reproducible
 *    scenario — the test instead pins the two things that MAKE it
 *    unraceable, either of which regressing would reopen the race:
 *      1. the selection is already applied with no timer or animation frame
 *         having run (§ "the seed lands with no deferral"), and
 *      2. `key={editing.initial}` at the call site forces a REMOUNT when the
 *         seed changes (§ "a changed seed remounts"). This is the load-
 *         bearing half: the effect is keyed `[initial]`, not mount-only, over
 *         an UNCONTROLLED `defaultValue` input, so without the key a
 *         same-instance `initial` change re-runs `setSelectionRange` over
 *         whatever the user had already typed — React does not re-apply
 *         `defaultValue` on rerender. PR #2565 added that key for exactly
 *         this reason and nothing failed if it were removed. Now something
 *         does.
 *
 *  - `Binder.newFileInput.cursorToEnd` (`Binder.tsx` in `openNewFileInput`)
 *    DOES run in a deferred `requestAnimationFrame`, so it is raceable and is
 *    raced here directly. Its claim is narrower: the range it sets is
 *    zero-width (`start === end`), i.e. a caret placement, so there is
 *    nothing for it to clobber even when the user has typed. The test types
 *    into the field inside the deferred window and asserts the range stays
 *    zero-width and the typed text survives.
 *
 * ── Determinism ───────────────────────────────────────────────────────
 *
 * `requestAnimationFrame` is stubbed with an explicit queue rather than left
 * to jsdom's clock: the whole point is to hold the frame open, type into the
 * field, and only then let the callback run. `vi.useFakeTimers()` is NOT used
 * — it does not fake `rAF` by default, and React 19's scheduler runs on
 * `MessageChannel`, so faking timers would buy nothing here while risking
 * `act()`'s effect flush.
 */

import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, BinderRow, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

/** One root-level file plus one inside a folder, so the context menu's
 *  "New file here" has a non-empty directory prefix to seed with. */
const OUTLINE: FileOutline[] = [
  { path: "chapter-one.ink", symbols: [], mounted: false },
  { path: "scenes/intro.ink", symbols: [], mounted: false },
];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

/** Animation-frame callbacks scheduled but not yet run. */
let frames: FrameRequestCallback[] = [];

beforeEach(() => {
  frames = [];
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback): number => {
    frames.push(cb);
    return frames.length;
  });
});

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.unstubAllGlobals();
});

/** Run every pending animation frame, in order. */
function flushFrames(): void {
  const pending = frames;
  frames = [];
  act(() => {
    for (const cb of pending) cb(0);
  });
}

function mountBinder(): void {
  const store = createStudioStore();
  store.setState({ outline: OUTLINE });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(Binder) }));
  });
}

function query<T extends Element>(selector: string): T {
  const el = container!.querySelector<T>(selector);
  if (el === null) throw new Error(`not found: ${selector}`);
  return el;
}

/** The Binder's scroll container — the element its keyboard handler is on. */
function binderRoot(): HTMLElement {
  return query<HTMLElement>(".brink-binder");
}

function press(key: string): void {
  act(() => {
    binderRoot().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
}

/** The row element carrying `key`, found by exact attribute compare. */
function row(key: string): HTMLElement {
  for (const el of container!.querySelectorAll("[data-binder-row-key]")) {
    if (el.getAttribute("data-binder-row-key") === key) return el as HTMLElement;
  }
  throw new Error(`row not found: ${JSON.stringify(key)}`);
}

/** Arrow down until `key`'s row is the focused one — the keyboard path a
 *  user takes to reach a row, without depending on the Binder's row order. */
function focusRow(key: string): void {
  for (let i = 0; i < 20; i += 1) {
    if (row(key).className.includes("brink-binder-focused")) return;
    press("ArrowDown");
  }
  throw new Error(`never focused ${JSON.stringify(key)} with ArrowDown`);
}

function click(el: Element): void {
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

/** Type `text` into `input` the way a real keystroke would leave it: value
 *  replaced, caret parked where the user stopped. */
function typeInto(input: HTMLInputElement, text: string): void {
  input.value = text;
  input.setSelectionRange(text.length, text.length);
}

// ── Binder.renameInput.preSelectBasename (the in-row rename field) ────

describe("Binder in-row rename pre-select (#2571, SELECT-INVARIANT Binder.renameInput.preSelectBasename)", () => {
  /** Focus the first row with the keyboard and open its rename field (F2) —
   *  the real user path: arrow to a row, press F2. */
  function openRename(key: string): HTMLInputElement {
    mountBinder();
    focusRow(key);
    press("F2");
    return query<HTMLInputElement>(".brink-binder-rename-input");
  }

  it("seeds the field and selects only the basename, with no deferral to race", () => {
    const input = openRename("chapter-one.ink");

    // Non-vacuity: if this scan ever stops finding the field, every
    // assertion below would be checking a stand-in rather than the real one.
    expect(input.value).toBe("chapter-one.ink");

    // No animation frame has been allowed to run — `flushFrames()` is
    // deliberately NOT called. The selection is nonetheless already applied,
    // which is the whole content of this call site's justification: it runs
    // synchronously in the mount effect, so there is no window in which the
    // user could type before it lands. Moving the call into a `setTimeout` /
    // `requestAnimationFrame` (the #2511 mechanism) fails right here.
    expect(frames, "the rename field must not schedule an animation frame").toHaveLength(0);
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe("chapter-one".length);
  });

  it("selects the whole name when it has no extension (folder rename)", () => {
    // Preservation guard for the folder-rename case: a folder name has no
    // extension, and the whole of it must be selected. Deleting the
    // `setSelectionRange` fails here (measured: the caret lands at 6, not a
    // 0–6 selection).
    //
    // Measured and recorded so nobody re-derives it: this does NOT distinguish
    // `dot > 0 ? dot : initial.length` from a bare `dot`. `setSelectionRange`'s
    // `end` is an unsigned long, so `-1` wraps and then clamps to the value's
    // length — the two spellings are observationally identical. The ternary is
    // clarity, not behaviour.
    const input = openRename("scenes/");
    expect(input.value).toBe("scenes");
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe("scenes".length);
  });

  it("re-seeding remounts the field rather than selecting over what the user typed", () => {
    // The `key={editing.initial}` guard, tested at the level that owns it:
    // `BinderRow` renders `<RenameInput key={editing.initial} …/>`, and
    // `RenameInput`'s effect is keyed `[initial]` over an uncontrolled
    // `defaultValue` input. Drive `BinderRow` directly — inside the full
    // Binder a row's key and its `initial` are both derived from the same
    // path, so they can never diverge there, which is precisely why this
    // regression would ship unnoticed without a test at this level.
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    const render = (initial: string): void => {
      act(() => {
        root!.render(
          createElement(BinderRow, {
            rowKey: "chapter-one.ink",
            depth: 0,
            kind: "file",
            label: initial,
            expandable: false,
            isExpanded: false,
            isActive: false,
            isSelected: false,
            isFocused: false,
            isDragging: false,
            isDropInto: false,
            dropLinePosition: null,
            draggable: false,
            editing: { initial, onCommit: () => {}, onCancel: () => {} },
            onChevronClick: () => {},
            onClick: () => {},
            onDoubleClick: () => {},
            onContextMenu: () => {},
            onDragStart: () => {},
            onDragEnd: () => {},
            onDragOver: () => {},
            onDrop: () => {},
          }),
        );
      });
    };

    render("chapter-one.ink");
    const first = query<HTMLInputElement>(".brink-binder-rename-input");
    expect(first.selectionEnd).toBe("chapter-one".length);

    // The user retypes the name. The field is uncontrolled, so this is the
    // only copy of what they wrote.
    typeInto(first, "renamed-by-hand");

    // Now the seed changes underneath them.
    render("chapter-two.ink");
    const second = query<HTMLInputElement>(".brink-binder-rename-input");

    // With the key: a brand-new element, freshly seeded — the effect runs on
    // a mount, never over user text. Without it: `second === first`, still
    // holding "renamed-by-hand" (React does not re-apply `defaultValue`),
    // with the effect's `setSelectionRange(0, 11)` now covering 11 characters
    // the USER typed, so their next keystroke destroys it. Both assertions
    // below go red in that case.
    expect(second, "the rename field must remount when its seed changes").not.toBe(first);
    expect(second.value).toBe("chapter-two.ink");
    expect(second.selectionStart).toBe(0);
    expect(second.selectionEnd).toBe("chapter-two".length);
  });
});

// ── Binder.newFileInput.cursorToEnd (the inline New File field) ───────

describe("Binder new-file caret placement (#2571, SELECT-INVARIANT Binder.newFileInput.cursorToEnd)", () => {
  /** Click "+ New file" — the plain, undebounced row handler. Returns the
   *  field with its animation frame still pending. */
  function openNewFile(): HTMLInputElement {
    mountBinder();
    click(query(".brink-binder-new"));
    return query<HTMLInputElement>(".brink-tab-input");
  }

  /** Open "New file here" from a folder row's context menu, which seeds the
   *  field with that folder's prefix (`target.prefix` in
   *  `BinderContextMenu.tsx`'s FOLDER branch). */
  function openNewFileInFolder(): HTMLInputElement {
    mountBinder();
    // Right-click the folder row; folders render expanded by default so no
    // expand step is needed.
    const folderRow = query<HTMLElement>(".brink-binder-folder-row");
    act(() => {
      folderRow.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    });
    const item = [...container!.querySelectorAll(".brink-context-menu-item")].find(
      (el) => el.textContent === "New file here",
    );
    if (item === undefined) throw new Error("no 'New file here' item in the context menu");
    click(item);
    return query<HTMLInputElement>(".brink-tab-input");
  }

  it("leaves text typed during the deferred frame alone, and never widens to a selection", () => {
    const input = openNewFile();

    // The frame is genuinely still pending — without this the race below
    // would be staged, not real, and the test would pass against a call site
    // that had been deleted outright.
    expect(frames, "openNewFileInput must schedule exactly one frame").toHaveLength(1);

    // The user types before the frame runs — the #2511 window.
    typeInto(input, "hello.ink");
    input.setSelectionRange(3, 3); // caret parked mid-word

    flushFrames();

    // The invariant: their text is intact, and the range is a caret (zero
    // width), not a selection. `setSelectionRange(0, end)` here — the shape
    // §7.7.1 rule 2 forbids — fails both of the last two assertions.
    expect(input.value).toBe("hello.ink");
    expect(
      input.selectionStart,
      "the new-file caret must stay zero-width, never becoming a selection",
    ).toBe(input.selectionEnd);
    // …and it lands at the end of what the user actually typed, because
    // `end` is read from `input.value` at fire time. A length captured before
    // the frame (the seeded prefix, "") would put it at 0.
    expect(input.selectionStart).toBe("hello.ink".length);
  });

  it("puts the caret after the seeded directory prefix on an untouched field", () => {
    // Preservation guard: the point of the deferred call is that a field
    // pre-filled with "scenes/" is ready to type a filename into, rather than
    // leaving the caret in front of the prefix. Deleting the
    // `setSelectionRange` must fail here — otherwise this call site could be
    // "fixed" by removing it and the invariant would hold vacuously.
    const input = openNewFileInFolder();
    expect(input.value).toBe("scenes/");
    expect(frames).toHaveLength(1);

    // jsdom parks the caret at the end of the value on every write to
    // `.value`, so a freshly seeded field arrives here already reading (7, 7)
    // — which would make the assertions below pass with the call site deleted
    // (measured: deleting it left this test green). A real browser puts the
    // caret at 0 in a field seeded through the `value` ATTRIBUTE, which is
    // what React's `defaultValue` writes and what the user actually faces.
    // Reset to 0 so this test observes the browser's starting state rather
    // than jsdom's.
    input.setSelectionRange(0, 0);

    flushFrames();

    expect(input.selectionStart).toBe("scenes/".length);
    expect(input.selectionEnd).toBe("scenes/".length);
  });
});
