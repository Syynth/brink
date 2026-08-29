/**
 * The three suppression gestures the Problems panel offers, as pure text
 * edits (#3148).
 *
 * They are the compiler's existing eslint-style directive channels — none
 * of this invents a mechanism, it just spells one:
 *
 * | Gesture | What it writes | Compiler side |
 * |---|---|---|
 * | this line | `// brink-disable Exxx` above the line | `brink-ir::suppressions` |
 * | this code, this file | `// brink-disable-file Exxx` at the top | same |
 * | everything, this file | `// brink-disable-file-all` at the top | same |
 * | this project | `[lints] Exxx = "allow"` | `brink-analyzer` + `brink.toml` |
 *
 * `// brink-expect` is deliberately NOT offered: it suppresses AND asserts
 * the diagnostic is there, so it is a test-writing tool rather than a
 * "make this go away" one, and offering it beside three that only silence
 * would invite it being picked by mistake.
 *
 * **Only warning-tier codes are suppressible.** An `Error`-default code is
 * refused by both channels (`E154` for the annotation, and `[lints]`' own
 * hard-error exemption), because an error means no correct artifact can be
 * produced. The menu must not offer what the compiler will refuse — see
 * `isSuppressible`.
 */

/** Line the directive attaches to, from a byte offset into `source`. */
function lineStartOf(source: string, offset: number): number {
  const clamped = Math.max(0, Math.min(offset, source.length));
  return source.lastIndexOf("\n", clamped - 1) + 1;
}

/** The indentation of the line containing `offset`, so the comment lines up. */
function indentAt(source: string, offset: number): string {
  const start = lineStartOf(source, offset);
  const line = source.slice(start, source.indexOf("\n", start) + 1 || undefined);
  return /^[ \t]*/.exec(line)?.[0] ?? "";
}

/**
 * Insert `// brink-disable <code>` on the line ABOVE the diagnostic.
 *
 * The directive targets the NEXT line — that is the compiler's rule
 * (`line_directives` is keyed by `line_idx + 1`), and it is why this
 * inserts above rather than appending to the offending line.
 *
 * An existing `brink-disable` directly above is EXTENDED rather than
 * duplicated: two directives on consecutive lines would leave the first
 * one targeting the second, silencing nothing.
 */
export function suppressOnLine(source: string, offset: number, code: string): string {
  const start = lineStartOf(source, offset);
  const indent = indentAt(source, offset);

  if (start > 0) {
    const prevStart = lineStartOf(source, start - 1);
    const prev = source.slice(prevStart, start - 1);
    const existing = /^\s*\/\/\s*brink-disable(\s+(.*))?$/.exec(prev);
    if (existing) {
      const codes = (existing[2] ?? "").trim().split(/\s+/).filter(Boolean);
      if (codes.length === 0) return source; // bare `brink-disable` already covers everything
      if (codes.includes(code)) return source;
      return (
        source.slice(0, prevStart) +
        `${indent}// brink-disable ${[...codes, code].join(" ")}` +
        source.slice(start - 1)
      );
    }
  }

  return `${source.slice(0, start)}${indent}// brink-disable ${code}\n${source.slice(start)}`;
}

/**
 * Put `// brink-disable-file <code>` at the top of the file.
 *
 * Goes above everything, including any existing leading comment: the
 * directive is file-scoped, so its position carries no meaning beyond
 * "this file", and burying it under a header comment makes it easy to miss
 * when wondering why a file reports nothing.
 *
 * NAMES THE CODE (#3259). This function used to take no code at all and
 * write a bare `// brink-disable-file`, while the menu item offering it read
 * "Suppress E157 in this file" — so one click silenced every diagnostic in
 * the file and the label said otherwise. Codes are whitespace-separated,
 * matching the line-scoped directive.
 *
 * An existing file directive is EXTENDED rather than duplicated: two of them
 * are legal but make the file's suppression state two things to read instead
 * of one.
 */
export function suppressInFile(source: string, code: string): string {
  if (/^\s*\/\/\s*brink-disable-(file-all|all)\s*$/m.test(source)) {
    // Already blanket-suppressed; naming one code would narrow nothing.
    return source;
  }
  const existing = /^\s*\/\/\s*brink-disable-file[ \t]+(.*)$/m.exec(source);
  if (existing) {
    const codes = (existing[1] ?? "").trim().split(/\s+/).filter(Boolean);
    if (codes.includes(code)) return source;
    return source.replace(
      existing[0],
      `// brink-disable-file ${[...codes, code].join(" ")}`,
    );
  }
  return `// brink-disable-file ${code}\n${source}`;
}

/**
 * Put `// brink-disable-file-all` at the top of the file — every diagnostic.
 *
 * Spelled `-all` since #3259. The bare `// brink-disable-file` used to mean
 * this, which is exactly why a code-scoped gesture could not be told apart
 * from a blanket one; the compiler now reports the bare form as `E192`.
 */
export function suppressAllInFile(source: string): string {
  if (/^\s*\/\/\s*brink-disable-(file-all|all)\s*$/m.test(source)) return source;
  return `// brink-disable-file-all\n${source}`;
}

/**
 * Whether the compiler will honour a suppression for this code.
 *
 * `Error`-default codes are refused by every channel. The severity here is
 * the code's DEFAULT, not its effective one — a `[lints]` entry cannot make
 * a warning unsuppressible, nor an error suppressible.
 */
export function isSuppressible(defaultSeverity: string | undefined): boolean {
  return defaultSeverity !== undefined && defaultSeverity !== "error";
}
