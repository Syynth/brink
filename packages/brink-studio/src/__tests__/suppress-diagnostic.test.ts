/**
 * The Problems panel's three suppression gestures, as text edits (#3148).
 *
 * They write the compiler's existing eslint-style directives, so the tests
 * are mostly about matching what `brink-ir::suppressions` actually parses —
 * above all that `// brink-disable` targets the NEXT line, which is the one
 * thing an implementation can get backwards and still look right.
 */
import { describe, expect, it } from "vitest";
import {
  isSuppressible,
  suppressAllInFile,
  suppressInFile,
  suppressOnLine,
} from "@brink/studio-ui";

const SRC = `=== intro ===
VAR roll = 0
Hello.
-> DONE
`;

/** Byte offset of the first occurrence of `needle`. */
const at = (src: string, needle: string): number => src.indexOf(needle);

describe("suppressOnLine", () => {
  it("inserts the directive ABOVE the offending line", () => {
    // The compiler keys line directives by `line_idx + 1` — the comment
    // applies to the line AFTER it. Appending to the offending line instead
    // would silence the following line and leave this one reported.
    const next = suppressOnLine(SRC, at(SRC, "VAR roll"), "E035");
    expect(next).toBe(`=== intro ===
// brink-disable E035
VAR roll = 0
Hello.
-> DONE
`);
  });

  it("matches the offending line's indentation", () => {
    const indented = "=== intro ===\n    VAR roll = 0\n";
    const next = suppressOnLine(indented, at(indented, "VAR roll"), "E035");
    expect(next).toContain("\n    // brink-disable E035\n    VAR roll");
  });

  it("extends an existing directive rather than stacking a second", () => {
    // Two directives on consecutive lines would leave the first targeting
    // the second — silencing nothing, and looking like it should work.
    const once = suppressOnLine(SRC, at(SRC, "VAR roll"), "E035");
    const twice = suppressOnLine(once, once.indexOf("VAR roll"), "E014");
    expect(twice).toContain("// brink-disable E035 E014");
    expect(twice.match(/brink-disable/g)).toHaveLength(1);
  });

  it("is idempotent for a code already listed", () => {
    const once = suppressOnLine(SRC, at(SRC, "VAR roll"), "E035");
    expect(suppressOnLine(once, once.indexOf("VAR roll"), "E035")).toBe(once);
  });

  it("leaves a bare directive alone — it already covers everything", () => {
    const src = "=== intro ===\n// brink-disable\nVAR roll = 0\n";
    expect(suppressOnLine(src, src.indexOf("VAR roll"), "E035")).toBe(src);
  });

  it("handles a diagnostic on the very first line", () => {
    const src = "VAR roll = 0\n";
    expect(suppressOnLine(src, 0, "E035")).toBe("// brink-disable E035\nVAR roll = 0\n");
  });
});

describe("suppressInFile", () => {
  it("names the code, rather than silencing the whole file", () => {
    // #3259: this used to write a bare `// brink-disable-file` while the
    // menu item read "Suppress E157 in this file" — one click silenced
    // everything and the label said otherwise.
    expect(suppressInFile(SRC, "E035")).toBe(`// brink-disable-file E035\n${SRC}`);
  });

  it("goes above an existing header comment rather than under it", () => {
    // File-scoped, so its position carries no meaning beyond "this file" —
    // and buried under a header it is easy to miss when wondering why a
    // file reports nothing.
    const src = "// A scene.\n=== intro ===\n";
    expect(suppressInFile(src, "E035")).toBe(`// brink-disable-file E035\n${src}`);
  });

  it("extends an existing file directive instead of adding a second", () => {
    const once = suppressInFile(SRC, "E035");
    expect(suppressInFile(once, "E027")).toBe(`// brink-disable-file E035 E027\n${SRC}`);
  });

  it("is idempotent for a code already named", () => {
    const once = suppressInFile(SRC, "E035");
    expect(suppressInFile(once, "E035")).toBe(once);
  });

  it("defers to a blanket directive, which already covers the code", () => {
    const all = "// brink-disable-all\n=== intro ===\n";
    expect(suppressInFile(all, "E035")).toBe(all);
    const fileAll = "// brink-disable-file-all\n=== intro ===\n";
    expect(suppressInFile(fileAll, "E035")).toBe(fileAll);
  });
});

describe("suppressAllInFile", () => {
  it("writes the -all spelling, not the bare form", () => {
    // The bare `// brink-disable-file` is now reported as E192: it names no
    // codes and is not the blanket spelling.
    expect(suppressAllInFile(SRC)).toBe(`// brink-disable-file-all\n${SRC}`);
  });

  it("is idempotent, and defers to a project-wide disable-all", () => {
    expect(suppressAllInFile(suppressAllInFile(SRC))).toBe(suppressAllInFile(SRC));
    const all = "// brink-disable-all\n=== intro ===\n";
    expect(suppressAllInFile(all)).toBe(all);
  });
});

describe("isSuppressible", () => {
  it("refuses an error-tier code", () => {
    // Both channels refuse it (E154 for the annotation, and [lints]' own
    // hard-error exemption): an error means no correct artifact can be
    // produced. Offering the gesture would be offering a no-op.
    expect(isSuppressible("error")).toBe(false);
  });

  it("allows the warning and advisory tiers", () => {
    expect(isSuppressible("warning")).toBe(true);
    expect(isSuppressible("info")).toBe(true);
  });

  it("refuses when the severity is unknown", () => {
    // A diagnostic with no registry entry — better to omit the gesture than
    // to write a directive the compiler may reject.
    expect(isSuppressible(undefined)).toBe(false);
  });
});
