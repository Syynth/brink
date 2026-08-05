import { describe, it, expect } from "vitest";
import { completionType, toCompletionOption } from "@brink-lang/editor";
import type { CompletionItem } from "@brink/wasm-types";

const item = (partial: Partial<CompletionItem>): CompletionItem =>
  ({ name: "", kind: "text", ...partial }) as CompletionItem;

// The keys must match the wasm `symbol_kind_str` output (snake_case). A casing
// drift here silently mis-icons completions AND disables auto-open-on-completion,
// which keys off the `"function"`/`"method"` types (#229).
describe("completionType", () => {
  it("maps callable kinds to function/method (auto-open depends on this)", () => {
    expect(completionType("knot")).toBe("function");
    expect(completionType("external")).toBe("function");
    expect(completionType("stitch")).toBe("method");
  });

  it("maps the remaining wasm kinds to their CM types", () => {
    expect(completionType("variable")).toBe("variable");
    expect(completionType("constant")).toBe("constant");
    expect(completionType("list")).toBe("enum");
    expect(completionType("list_item")).toBe("enumMember");
    expect(completionType("label")).toBe("property");
    expect(completionType("param")).toBe("variable");
    expect(completionType("temp")).toBe("variable");
    expect(completionType("value")).toBe("enum");
    // Cue-name completions (#2134) — matches the LSP side's
    // `CompletionItemKind::CONSTANT`. Without this KIND_MAP entry a cue row
    // silently falls back to "text", mis-icons it, and disables
    // auto-open-on-completion the same way a missing #229 entry would.
    expect(completionType("cue")).toBe("constant");
  });

  it("falls back to text for unknown kinds", () => {
    expect(completionType("nope")).toBe("text");
  });
});

// #211 — value-list items: filter by name OR id OR detail, insert the id, show
// only the name.
describe("toCompletionOption", () => {
  it("makes a value item matchable by name, value, and detail; displays the name", () => {
    const opt = toCompletionOption(
      item({ name: "Harbor", insert: "1", detail: "Map #1", kind: "value" }),
    );
    expect(opt.label).toBe("Harbor 1 Map #1"); // CM filters on this
    expect(opt.displayLabel).toBe("Harbor"); // …but the row shows the name
    expect(opt.apply).toBe("1"); // inserts the id
    expect(opt.detail).toBe("Map #1");
    expect(opt.type).toBe("enum");
  });

  it("omits absent detail from the match terms", () => {
    const opt = toCompletionOption(item({ name: "Harbor", insert: "1", kind: "value" }));
    expect(opt.label).toBe("Harbor 1");
    expect(opt.displayLabel).toBe("Harbor");
    expect(opt.apply).toBe("1");
  });

  it("leaves plain completions matching/displaying their name", () => {
    const opt = toCompletionOption(item({ name: "start", kind: "knot" }));
    expect(opt.label).toBe("start");
    expect(opt.displayLabel).toBeUndefined();
    expect(opt.apply).toBeUndefined();
    expect(opt.type).toBe("function");
  });
});
