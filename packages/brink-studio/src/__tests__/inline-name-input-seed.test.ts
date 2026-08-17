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
import { renameExtension, startInlineRename, InlineNameInput } from "@brink-lang/editor";

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

const unsafe = (n: number): StructuralResult => ({
  ok: true,
  path: "main.ink",
  new_source: "=== hello ===\n",
  cross_file_edits: [{ path: "other.ink", new_source: "-> hello\n" }],
  introduced_diagnostics: Array.from({ length: n }, (_, i) => ({
    severity: "error" as const,
    code: "E022",
    message: `unresolved divert ${i}`,
    path: "other.ink",
    line: i + 1,
    col: 1,
  })),
  safe: false,
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

  // ── #2557 regression: clear-before-set on the two deferred focus timers ──
  //
  // `render()`'s post-mount `focusTimer` and `renderReport()`'s
  // `forceFocusTimer` are stored on instance fields specifically so
  // `dispose()` can cancel them. Both `InlineRenameWidget.eq()` (rename.ts)
  // and `ExtractPromptWidget.eq()` (extract-actions.ts) return `false`
  // unconditionally, so CM6 is free to call `toDOM()` -> `render()` again on
  // the *same* `InlineNameInput` instance. Without a clear-before-set guard
  // at each assignment, a second call overwrites the field and orphans the
  // first handle: `dispose()` can only cancel whatever the field currently
  // holds, so the orphaned timer survives teardown and still fires. These
  // tests exercise `InlineNameInput` directly (rather than through
  // `renameExtension`) so they pin the guard as a property of the class
  // itself, independent of whether any *current* call site happens to avoid
  // re-invoking `render()`/the report a second time.

  it("clears an orphaned focusTimer when render() runs again on the same instance", () => {
    const focusSpy = vi.spyOn(HTMLInputElement.prototype, "focus");
    const controller = new InlineNameInput(
      {
        initialValue: SYMBOL,
        ariaLabel: "Rename hello",
        forceLabel: "Rename anyway",
        query: () => null,
        onCommit: () => {},
      },
      () => {},
    );

    // First mount (e.g. CM6's initial `toDOM()`) — schedules `focusTimer`.
    const first = controller.render();
    document.body.appendChild(first);

    // A decoration redraw before that timer fires calls `render()` again on
    // the same controller, producing a second (about-to-be-detached) root.
    first.remove();
    const second = controller.render();
    document.body.appendChild(second);

    controller.dispose();
    vi.runAllTimers();

    // No orphaned timer should survive teardown to call `.focus()` — with
    // the bug, the first render's timer is never cancelled and still fires.
    expect(focusSpy).not.toHaveBeenCalled();
    focusSpy.mockRestore();
    document.body.replaceChildren();
  });

  it("clears an orphaned forceFocusTimer when renderReport() runs again while the report is open", () => {
    const focusSpy = vi.spyOn(HTMLButtonElement.prototype, "focus");
    const results: Record<string, StructuralResult> = { bad: unsafe(1), worse: unsafe(2) };
    const controller = new InlineNameInput(
      {
        initialValue: SYMBOL,
        ariaLabel: "Rename hello",
        forceLabel: "Rename anyway",
        liveBadge: true,
        query: (name) => results[name] ?? null,
        onCommit: () => {},
      },
      () => {},
    );
    const root = controller.render();
    document.body.appendChild(root);
    vi.advanceTimersByTime(0); // settle the initial focus/select deferral

    const input = root.querySelector<HTMLInputElement>(".brink-inline-rename-input");
    if (input === null) throw new Error("input not mounted");
    input.value = "bad";
    input.dispatchEvent(new Event("input"));
    vi.advanceTimersByTime(251); // debounce settle + deferred query resolve

    const badge = root.querySelector<HTMLButtonElement>(".brink-inline-rename-badge");
    if (badge === null) throw new Error("badge not mounted");
    badge.click(); // opens the report -> schedules forceFocusTimer #1

    // A badge refresh while the report is already open re-renders it
    // (`updateBadge()`'s `if (this.reportOpen) this.renderReport(result)`
    // branch) before the first `forceFocusTimer` fires.
    input.value = "worse";
    input.dispatchEvent(new Event("input"));
    vi.advanceTimersByTime(251);

    controller.dispose();
    vi.runAllTimers();

    expect(focusSpy).not.toHaveBeenCalled();
    focusSpy.mockRestore();
    document.body.replaceChildren();
  });
});
