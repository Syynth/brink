import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { liveArgRange } from "@brink/ink-editor";

// liveArgRange resolves the source range of an argument literal at `from`, used
// to replace a host-widget's value in place. Quote-only matching left an Edit on
// a non-string (e.g. int `item_id`) literal a silent no-op (#242).
function rangeIn(doc: string, from: number): { from: number; to: number } | null {
  const state = EditorState.create({ doc });
  return liveArgRange({ state } as unknown as EditorView, from);
}

describe("liveArgRange", () => {
  it("covers a quoted string up to its closing quote", () => {
    const doc = `~ go_region("harbor")`;
    const from = doc.indexOf(`"harbor"`);
    expect(rangeIn(doc, from)).toEqual({ from, to: from + `"harbor"`.length });
  });

  it("covers a bare int literal up to the next delimiter (#242)", () => {
    const doc = `~ give_item(1, 2)`;
    const from = doc.indexOf("1");
    expect(rangeIn(doc, from)).toEqual({ from, to: from + 1 }); // just "1"
  });

  it("covers a multi-char bare literal, stopping at ) ", () => {
    const doc = `~ teleport(42)`;
    const from = doc.indexOf("42");
    expect(rangeIn(doc, from)).toEqual({ from, to: from + 2 });
  });

  it("returns null past the end of the document", () => {
    expect(rangeIn("ab", 5)).toBeNull();
  });
});
