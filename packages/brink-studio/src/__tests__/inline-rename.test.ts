/**
 * Inline rename badge/report logic (#323/#324).
 *
 * Two layers:
 *  - Pure logic — `isSafeRename` / `breakageCount` / `breakageEntries` and the
 *    `(path,offset,name)` query cache, exercised directly.
 *  - The in-editor widget — a real CM6 `EditorView` (jsdom) with
 *    `renameExtension`, driven through F2 → input → debounced badge → inline
 *    report → commit/cancel, with a stubbed `renameSymbolAt` so the test owns
 *    the safe/unsafe verdict.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Location, StructuralResult } from "@brink/wasm-types";
import {
  renameExtension,
  startInlineRename,
  isSafeRename,
  breakageCount,
  breakageEntries,
  RenameQueryCache,
} from "@brink-lang/editor";

const safe = (): StructuralResult => ({
  ok: true,
  path: "main.ink",
  new_source: "=== greeting ===\n",
  cross_file_edits: [],
  introduced_diagnostics: [],
  safe: true,
});

const unsafe = (n: number): StructuralResult => ({
  ok: true,
  path: "main.ink",
  new_source: "=== greeting ===\n",
  cross_file_edits: [{ path: "other.ink", new_source: "-> greeting\n" }],
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

describe("inline-rename pure logic", () => {
  it("isSafeRename: true only when safe and no introduced diagnostics", () => {
    expect(isSafeRename(safe())).toBe(true);
    expect(isSafeRename(unsafe(2))).toBe(false);
  });

  it("breakageCount counts introduced diagnostics (0 when safe)", () => {
    expect(breakageCount(safe())).toBe(0);
    expect(breakageCount(unsafe(3))).toBe(3);
  });

  it("breakageEntries lists file:line + message per diagnostic", () => {
    const entries = breakageEntries(unsafe(2));
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({ file: "other.ink", line: 1, message: "unresolved divert 0" });
  });

  it("breakageEntries falls back to sorted cross-file paths when no diagnostics", () => {
    const result: StructuralResult = {
      ...safe(),
      safe: false,
      cross_file_edits: [
        { path: "z.ink", new_source: "" },
        { path: "a.ink", new_source: "" },
      ],
    };
    const entries = breakageEntries(result);
    expect(entries.map((e) => e.file)).toEqual(["a.ink", "z.ink"]);
    expect(entries[0].line).toBeUndefined();
  });

  it("RenameQueryCache keys by (path, offset, name)", () => {
    const cache = new RenameQueryCache();
    const r = unsafe(1);
    cache.set("main.ink", 5, "greeting", r);
    expect(cache.get("main.ink", 5, "greeting")).toBe(r);
    expect(cache.get("main.ink", 6, "greeting")).toBeUndefined();
    expect(cache.get("main.ink", 5, "other")).toBeUndefined();
    cache.clear();
    expect(cache.get("main.ink", 5, "greeting")).toBeUndefined();
  });
});

// ── Widget integration ──────────────────────────────────────────────

const DOC = "=== hello ===\nHi.\n-> hello\n";
const SYMBOL = "hello";

/** A view wired with the inline rename extension; `renameSymbolAt` is stubbed
 *  so the test controls the verdict. */
function mountRename(verdict: (newName: string) => StructuralResult) {
  const queries: Array<{ offset: number; newName: string }> = [];
  const commits: Array<{ newName: string; currentName: string }> = [];
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
          renameSymbolAt: (offset, newName) => {
            queries.push({ offset, newName });
            return verdict(newName);
          },
          commitRename: (_result, newName, currentName) => {
            commits.push({ newName, currentName });
          },
        }),
      ],
    }),
    parent: document.body,
  });
  return { view, queries, commits };
}

function inputEl(view: EditorView): HTMLInputElement {
  const el = view.dom.querySelector<HTMLInputElement>(".brink-inline-rename-input");
  if (el === null) throw new Error("inline rename input not mounted");
  return el;
}

function typeName(view: EditorView, input: HTMLInputElement, value: string): void {
  input.value = value;
  input.dispatchEvent(new Event("input"));
}

describe("inline-rename widget", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it("F2 mounts the inline input anchored at the symbol", () => {
    const { view } = mountRename(() => safe());
    startInlineRename(view, DOC.indexOf(SYMBOL));
    expect(inputEl(view).value).toBe("hello");
    view.destroy();
  });

  it("shows '⚠ breaks N' on a colliding name and hides it when safe", () => {
    const { view } = mountRename((name) => (name === "bad" ? unsafe(2) : safe()));
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);

    typeName(view, input, "bad");
    vi.advanceTimersByTime(300);
    const badge = view.dom.querySelector<HTMLButtonElement>(".brink-inline-rename-badge")!;
    expect(badge.hidden).toBe(false);
    expect(badge.textContent).toBe("⚠ breaks 2");

    // A safe name hides the badge again.
    typeName(view, input, "fine");
    vi.advanceTimersByTime(300);
    expect(badge.hidden).toBe(true);
    view.destroy();
  });

  it("debounces the breakage query (one call per settle, cached)", () => {
    const { view, queries } = mountRename(() => unsafe(1));
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);

    typeName(view, input, "a");
    typeName(view, input, "ab");
    typeName(view, input, "abc");
    expect(queries).toHaveLength(0); // not yet — debounced
    vi.advanceTimersByTime(300);
    expect(queries).toHaveLength(1);
    expect(queries[0].newName).toBe("abc");

    // Re-typing the same name replays from cache (no second query).
    typeName(view, input, "");
    typeName(view, input, "abc");
    vi.advanceTimersByTime(300);
    expect(queries).toHaveLength(1);
    view.destroy();
  });

  it("reaches an interactive pending state as soon as the debounce settles, without waiting for the analysis (#722)", () => {
    const { view, queries } = mountRename(() => unsafe(1));
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);

    typeName(view, input, "bad");
    vi.advanceTimersByTime(250); // the debounce settles...

    // ...but the (deferred) breakage analysis has not run yet: this is the
    // "rename UI reaches interactive-pending state without waiting for
    // analysis" regression the issue asks for. The badge already reflects a
    // pending/checking state and is disabled (no apply until it resolves);
    // the input itself stays live so typing more isn't blocked.
    expect(queries).toHaveLength(0);
    const badge = view.dom.querySelector<HTMLButtonElement>(".brink-inline-rename-badge")!;
    expect(badge.hidden).toBe(false);
    expect(badge.disabled).toBe(true);
    expect(badge.classList.contains("brink-inline-rename-badge--pending")).toBe(true);
    expect(badge.getAttribute("aria-busy")).toBe("true");
    expect(input.disabled).toBe(false);

    // The deferred analysis then runs (idle-scheduled) and the badge settles
    // into the real verdict.
    vi.advanceTimersByTime(1);
    expect(queries).toHaveLength(1);
    expect(badge.disabled).toBe(false);
    expect(badge.classList.contains("brink-inline-rename-badge--pending")).toBe(false);
    expect(badge.textContent).toBe("⚠ breaks 1");
    view.destroy();
  });

  it("expands the badge into the inline report (no modal) with the affected list", () => {
    const { view } = mountRename(() => unsafe(2));
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);
    typeName(view, input, "bad");
    vi.advanceTimersByTime(300);

    const badge = view.dom.querySelector<HTMLButtonElement>(".brink-inline-rename-badge")!;
    badge.click();
    const report = view.dom.querySelector<HTMLElement>(".brink-inline-rename-report")!;
    expect(report.hidden).toBe(false);
    const items = report.querySelectorAll(".brink-inline-rename-report-item");
    expect(items).toHaveLength(2);
    expect(report.querySelector(".brink-inline-rename-force")?.textContent).toBe("Rename anyway");
    view.destroy();
  });

  it("commits a safe rename on Enter (no popover), after the deferred analysis resolves", () => {
    const { view, queries, commits } = mountRename(() => safe());
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);
    typeName(view, input, "greeting");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));

    // Enter does not force the (uncached) analysis to run synchronously —
    // nothing has committed yet, and the widget is still mounted.
    expect(queries).toHaveLength(0);
    expect(commits).toHaveLength(0);
    expect(view.dom.querySelector(".brink-inline-rename-input")).not.toBeNull();

    vi.advanceTimersByTime(1); // flush the deferred (idle-scheduled) query
    expect(commits).toEqual([{ newName: "greeting", currentName: "hello" }]);
    // The widget tears down on commit.
    expect(view.dom.querySelector(".brink-inline-rename-input")).toBeNull();
    view.destroy();
  });

  it("does not commit a safe rename to the unchanged name", () => {
    const { view, commits } = mountRename(() => safe());
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(commits).toHaveLength(0);
    view.destroy();
  });

  it("Enter on an unsafe name surfaces the report instead of committing", () => {
    const { view, commits } = mountRename(() => unsafe(1));
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);
    typeName(view, input, "bad");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));

    // Pending: the deferred analysis hasn't resolved yet, so the report isn't
    // up yet either — but the widget is already interactive (not blocked).
    const report = view.dom.querySelector<HTMLElement>(".brink-inline-rename-report")!;
    expect(report.hidden).toBe(true);
    vi.advanceTimersByTime(1);

    expect(commits).toHaveLength(0);
    expect(report.hidden).toBe(false);

    // "Rename anyway" commits the unsafe rename.
    report.querySelector<HTMLButtonElement>(".brink-inline-rename-force")!.click();
    expect(commits).toEqual([{ newName: "bad", currentName: "hello" }]);
    view.destroy();
  });

  it("Esc cancels and tears the widget down without committing", () => {
    const { view, commits } = mountRename(() => safe());
    startInlineRename(view, DOC.indexOf(SYMBOL));
    const input = inputEl(view);
    typeName(view, input, "greeting");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));

    expect(commits).toHaveLength(0);
    expect(view.dom.querySelector(".brink-inline-rename-input")).toBeNull();
    view.destroy();
  });

  it("destroying the editor tears the widget down (no leaked DOM)", () => {
    const { view } = mountRename(() => unsafe(1));
    startInlineRename(view, DOC.indexOf(SYMBOL));
    typeName(view, inputEl(view), "bad");
    vi.advanceTimersByTime(300);
    view.destroy();
    expect(document.querySelector(".brink-inline-rename-input")).toBeNull();
  });
});
