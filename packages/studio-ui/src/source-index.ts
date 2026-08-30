/**
 * Byte-offset → line / UTF-16 conversion for one source file (#3339).
 *
 * The compiled lines table's `SourceLocation` ranges are UTF-8 BYTE
 * offsets — the right unit for the translator-facing wire, which is
 * locale-neutral and never sees an editor. The editor road is UTF-16:
 * diagnostics convert `byte_to_utf16` on the Rust side before they ever
 * reach `editor.reveal`, and `documents.revealAt` consumes code units.
 * Feeding bytes into that road works on pure-ASCII files and drifts by
 * one for every multibyte character before the target — an em-dash three
 * paragraphs up silently shifts every highlight after it.
 *
 * One checkpoint per LINE rather than per code point: `lineForByte` is a
 * binary search over line starts, and `utf16ForByte` scans only within
 * the target's own line — cheap for any file a person edits, without
 * per-code-point arrays that scale with file size.
 */

export interface SourceIndex {
  /** 1-based line number containing the byte offset. */
  lineForByte(byte: number): number;
  /** UTF-16 code-unit offset for a UTF-8 byte offset (clamped to EOF). */
  utf16ForByte(byte: number): number;
}

interface LineStart {
  byte: number;
  utf16: number;
}

export function buildSourceIndex(text: string): SourceIndex {
  const starts: LineStart[] = [{ byte: 0, utf16: 0 }];
  let byte = 0;
  for (let i = 0; i < text.length; ) {
    const cp = text.codePointAt(i) as number;
    byte += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
    i += cp > 0xffff ? 2 : 1;
    if (cp === 0x0a) starts.push({ byte, utf16: i });
  }
  const totalBytes = byte;
  const totalUnits = text.length;

  const lineIndexFor = (b: number): number => {
    let lo = 0;
    let hi = starts.length - 1;
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2);
      if (starts[mid].byte <= b) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  };

  return {
    lineForByte: (b) => lineIndexFor(b) + 1,
    utf16ForByte: (b) => {
      if (b >= totalBytes) return totalUnits;
      const start = starts[lineIndexFor(b)];
      let curByte = start.byte;
      let i = start.utf16;
      while (curByte < b && i < text.length) {
        const cp = text.codePointAt(i) as number;
        curByte += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
        i += cp > 0xffff ? 2 : 1;
      }
      return i;
    },
  };
}
