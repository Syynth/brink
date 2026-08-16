/**
 * `InlineNameInput` deferred focus/select (#2535) — the in-editor name prompt
 * must not select text the user has already typed.
 *
 * `packages/ink-editor/src/inline-name-input.ts` focuses its input from a
 * `setTimeout(…, 0)` scheduled while the widget DOM is still detached. That
 * deferral is load-bearing for `focus()` (see the third test), but the
 * `select()` that used to ride along with it was unguarded: a user who typed
 * during the deferred window had their text selected, and the next keystroke
 * would replace it. `docs/studio-shell-spec.md` §7.7.1 rule 2 forbids exactly
 * that shape, and PR #2523 fixed the same defect in
 * `packages/studio-ui/src/SymbolRenamePrompt.tsx`.
 *
 * This surface is worse than #2511's original mechanism rather than milder:
 * there the clobbered value degraded the rename to a no-op, whereas here the
 * rename still *happens* — to the wrong string, silently.
 *
 * `e2e/symbol-rename.spec.ts` cannot catch this: it asserts
 * `toHaveValue("barter")` before filling, i.e. it waits for the seeded value
 * instead of racing the timer. These tests race it, with fake timers.
 *
 * The first test fails against the unguarded `select()`. The second and third
 * are preservation guards — they hold both before and after the fix, and they
 * are what stops the guard being "fixed" by deleting the `select()` or the
 * `setTimeout` outright.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Location, StructuralResult } from "@brink/wasm-types";
import { renameExtension, startInlineRename } from "@brink-lang/editor";

const DOC = "=== hello ===\nHi.\n-> hello\n";
const SYMBOL = "hello";

const safe = (): StructuralResult => ({
  ok: true,
  path: "main.ink",
  new_source: "=== hello ===\n",
  cross_file_edits: [],
  introduced_diagnostics: [],
  safe: true,
});

/** A view wired with the inline rename extension, opened on `hello`. The
 *  rename verdict is always safe — these tests are about the input's focus
 *  and selection, not about breakage. */
function openRename(): EditorView {
  const prepareRename = (source: string, offset: number): Location | null => {
    const start = source.indexOf(SYMBOL);
    if (offset < start || offset > start + SYMBOL.length) return null;
    return { file: "main.ink", start, end: start + SYMBOL.length };
  };
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        renameExtension({
          prepareRename,
          renameSymbolAt: () => safe(),
          commitRename: () => {},
        }),
      ],
    }),
    parent: document.body,
  });
  startInlineRename(view, DOC.indexOf(SYMBOL));
  return view;
}

function inputEl(view: EditorView): HTMLInputElement {
  const el = view.dom.querySelector<HTMLInputElement>(".brink-inline-rename-input");
  if (el === null) throw new Error("inline rename input not mounted");
  return el;
}

describe("InlineNameInput deferred focus/select (#2535)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it("leaves the selection alone on a field the user has already typed into", () => {
    const view = openRename();
    const input = inputEl(view);

    // Type into the freshly mounted input before the deferred callback runs —
    // exactly what a fast typist does, and what a loaded main thread makes
    // likely. The caret sits where the last keystroke left it.
    input.value = "haggle";
    input.setSelectionRange(6, 6);

    vi.advanceTimersByTime(0);

    // With the unguarded `select()` this is (0, 6): the whole typed name is
    // selected, so the next keystroke replaces it and the rename commits the
    // wrong string.
    expect(input.selectionStart).toBe(6);
    expect(input.selectionEnd).toBe(6);
    expect(input.value).toBe("haggle");
    view.destroy();
  });

  it("still selects the seeded name on an untouched field", () => {
    const view = openRename();
    const input = inputEl(view);

    vi.advanceTimersByTime(0);

    // Preservation guard: the point of the deferred `select()` is that typing
    // over an untouched prompt replaces the current name. Deleting the
    // `select()` instead of guarding it would fail here.
    expect(input.value).toBe(SYMBOL);
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(SYMBOL.length);
    view.destroy();
  });

  it("needs the deferral: the input is not focusable until CM has mounted it", () => {
    const view = openRename();
    const input = inputEl(view);

    // Preservation guard for the `setTimeout(…, 0)` itself. `render()` runs
    // inside `WidgetType.toDOM()`, which hands CM a *detached* element;
    // focusing it there is a no-op, and CM6's widget lifecycle
    // (`toDOM`/`updateDOM`/`coordsAt`/`destroy`) has no post-mount hook to
    // move the call into. Hence: focus arrives only once the timer fires.
    expect(document.activeElement).not.toBe(input);
    vi.advanceTimersByTime(0);
    expect(document.activeElement).toBe(input);
    view.destroy();
  });
});
