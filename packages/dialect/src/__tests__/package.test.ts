/**
 * The package's own smoke gate (#3393): the pure-TS surface an engine
 * imports — validate a preset, parse emitted text, fold runs — works with
 * no editor, no wasm, no DOM.
 */
import { describe, expect, it } from "vitest";
import { AT_CUE_DIALECT, DialectParser, runsOf, validateDialect } from "../index.js";

describe("@brink-lang/dialect", () => {
  it("validates the shipped preset and parses emitted text without any editor", () => {
    expect(validateDialect(AT_CUE_DIALECT)).toEqual([]);
    const parser = new DialectParser(AT_CUE_DIALECT);
    const segs = parser.parseEmitted("@Alice: (softly) hello");
    expect(segs.map((s) => s.kind)).toEqual(["character", "parenthetical", null]);
    const runs = runsOf(
      [{ segments: segs }, { segments: parser.parseEmitted("still Alice") }],
      AT_CUE_DIALECT,
    );
    expect(runs).toEqual([{ kind: "character", attrs: { speaker: "Alice" }, lines: [0, 1] }]);
  });
});
