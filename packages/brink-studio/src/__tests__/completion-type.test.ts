import { describe, it, expect } from "vitest";
import { completionType } from "@brink/ink-editor";

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
  });

  it("falls back to text for unknown kinds", () => {
    expect(completionType("nope")).toBe("text");
  });
});
