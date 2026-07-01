import { describe, it, expect, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { Completion } from "@codemirror/autocomplete";
import { toCompletionOption } from "@brink-lang/editor";
import type { AutoImportResult, CompletionItem } from "@brink/wasm-types";

const item = (partial: Partial<CompletionItem>): CompletionItem =>
  ({ name: "", kind: "text", ...partial }) as CompletionItem;

// A real CM view so a completion's `apply(view, …)` mutates a live document —
// mirrors the inline-rename test harness.
function viewWith(doc: string): EditorView {
  return new EditorView({
    state: EditorState.create({ doc }),
    parent: document.body,
  });
}

// Accept a completion at `name`'s position: locate the partially-typed word and
// invoke the option's `apply` over [from, to).
function accept(view: EditorView, option: Completion, typed: string): void {
  const from = view.state.doc.toString().indexOf(typed);
  const to = from + typed.length;
  const apply = option.apply;
  if (typeof apply !== "function") {
    // Plain string apply (or none): insert the literal ourselves.
    view.dispatch({ changes: { from, to, insert: apply ?? typed } });
    return;
  }
  apply(view, option, from, to);
}

// ── (a) The "from <file>" affordance ────────────────────────────────
describe("out-of-scope completion tagging (#312 F)", () => {
  it("tags an out-of-scope symbol with a 'from <file>' detail", () => {
    const opt = toCompletionOption(
      item({ name: "harbor", kind: "knot", out_of_scope: true, source_file: "scenes/economy.ink" }),
    );
    expect(opt.detail).toBe("from economy.ink");
    expect(opt.label).toBe("harbor");
  });

  it("appends the source-file affordance after an existing typed detail", () => {
    const opt = toCompletionOption(
      item({
        name: "trade",
        kind: "knot",
        detail: "(gold: int)",
        out_of_scope: true,
        source_file: "economy.ink",
      }),
    );
    expect(opt.detail).toBe("(gold: int) · from economy.ink");
  });

  it("leaves an in-scope symbol untagged (no 'from …')", () => {
    const opt = toCompletionOption(item({ name: "start", kind: "knot" }));
    expect(opt.detail).toBeUndefined();
    expect(opt.apply).toBeUndefined();
  });
});

// ── (b) Auto-import on accept ───────────────────────────────────────
describe("auto-import on completion accept (#312 F)", () => {
  const importEdit = (from: number): AutoImportResult => ({
    ok: true,
    already_reachable: false,
    edit: { from, to: from, insert: "INCLUDE economy.ink\n" },
  });

  it("inserts the symbol AND exactly one INCLUDE when out of scope", () => {
    const view = viewWith("=== start ===\nThe har goes here.\n");
    const autoImport = vi.fn(() => importEdit(0));
    const opt = toCompletionOption(
      item({ name: "harbor", kind: "knot", out_of_scope: true, source_file: "economy.ink" }),
      autoImport,
    );

    accept(view, opt, "har");

    const text = view.state.doc.toString();
    expect(autoImport).toHaveBeenCalledTimes(1);
    expect(autoImport).toHaveBeenCalledWith("economy.ink");
    // Exactly one INCLUDE line, at the top.
    const includes = text.match(/^INCLUDE economy\.ink$/gm) ?? [];
    expect(includes).toHaveLength(1);
    expect(text.startsWith("INCLUDE economy.ink\n")).toBe(true);
    // The symbol text was inserted at the cursor.
    expect(text).toContain("The harbor goes here.");
    view.destroy();
  });

  it("adds NO INCLUDE when the symbol is already in scope (idempotent)", () => {
    const view = viewWith("=== start ===\nThe har goes here.\n");
    // Reachable ⇒ the wasm op reports already_reachable and returns no edit.
    const autoImport = vi.fn<() => AutoImportResult>(() => ({
      ok: true,
      already_reachable: true,
    }));
    const opt = toCompletionOption(
      item({ name: "harbor", kind: "knot", out_of_scope: true, source_file: "economy.ink" }),
      autoImport,
    );

    accept(view, opt, "har");

    const text = view.state.doc.toString();
    expect(autoImport).toHaveBeenCalledTimes(1);
    expect(text).not.toContain("INCLUDE");
    expect(text).toContain("The harbor goes here.");
    view.destroy();
  });

  it("accepting a plain in-scope symbol never queries auto-import", () => {
    const view = viewWith("=== start ===\nThe sta goes here.\n");
    const autoImport = vi.fn<() => AutoImportResult>(() => ({
      ok: false,
      already_reachable: false,
    }));
    const opt = toCompletionOption(item({ name: "start", kind: "knot" }), autoImport);

    accept(view, opt, "sta");

    expect(autoImport).not.toHaveBeenCalled();
    expect(view.state.doc.toString()).not.toContain("INCLUDE");
    view.destroy();
  });
});
