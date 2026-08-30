/**
 * Byte→line / byte→UTF-16 conversion for lines-table source links (#3339).
 *
 * The regression this guards: the table's `SourceLocation` ranges are
 * UTF-8 bytes, the editor reveal road is UTF-16 — fed raw, every
 * multibyte character above the target shifted the highlight ("i don't
 * think it's highlighting the correct byte ranges"). ASCII fixtures
 * cannot catch that, so every case here puts multibyte content BEFORE
 * the target.
 */
import { describe, expect, it } from "vitest";
// Internal by design — imported by path, not through the package index.
import { buildSourceIndex } from "../../../studio-ui/src/source-index.js";

// "first — line\n"  em-dash: 3 bytes / 1 unit → line 1 is 15 bytes, 13 units
// "The 𝄞 target.\n"  U+1D11E: 4 bytes / 2 units
const TEXT = "first — line\nThe 𝄞 target.\nlast\n";

describe("buildSourceIndex", () => {
  const index = buildSourceIndex(TEXT);

  it("numbers lines from byte offsets, 1-based", () => {
    expect(index.lineForByte(0)).toBe(1);
    expect(index.lineForByte(14)).toBe(1); // the \n byte itself
    expect(index.lineForByte(15)).toBe(2); // first byte of line 2
    expect(index.lineForByte(33)).toBe(3);
  });

  it("converts byte offsets to UTF-16 units across multibyte content", () => {
    // Start of line 2: 15 bytes in, 13 units in — the em-dash cost 3/1.
    expect(index.utf16ForByte(15)).toBe(13);
    // "The " after it: +4/+4.
    expect(index.utf16ForByte(19)).toBe(17);
    // Past the astral clef (4 bytes, 2 units): 19+4+1(space)=24 bytes → 17+2+1=20 units.
    expect(index.utf16ForByte(24)).toBe(20);
  });

  it("clamps past-EOF offsets instead of scanning forever", () => {
    expect(index.utf16ForByte(10_000)).toBe(TEXT.length);
  });

  it("is exact on a pure-ASCII file (bytes and units coincide)", () => {
    const ascii = buildSourceIndex("one\ntwo\nthree\n");
    expect(ascii.lineForByte(4)).toBe(2);
    expect(ascii.utf16ForByte(9)).toBe(9);
  });
});
