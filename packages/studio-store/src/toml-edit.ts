/**
 * Minimal, comment-preserving structured edits for `brink.toml` (#3015).
 *
 * The round-trip trap the issue names — "rewriting the file from a parsed
 * model silently discards an author's comments" — is avoided by never
 * re-serializing: every edit is a TARGETED line operation on the original
 * text. Comments, blank lines, key order, and unrelated tables all
 * survive. The one accepted loss: replacing a key's value rewrites that
 * one line, so an inline comment ON THAT LINE goes with it.
 *
 * Scope is the `brink-project-config` schema this form edits — top-level
 * string keys in named tables (`[project] entry`, `conventions`,
 * `dialect`, `types`). Not a general TOML editor: multi-line strings,
 * arrays, and dotted keys are out of scope (the raw-text editor is the
 * escape hatch, by design).
 */

const isTableHeader = (line: string): boolean => /^\s*\[/.test(line.trim());

/** Whether `line` is `table`'s own header (`[table]`, whitespace-tolerant). */
function isHeaderOf(line: string, table: string): boolean {
  const m = /^\s*\[\s*([^\]]*?)\s*\]\s*(?:#.*)?$/.exec(line);
  return m !== null && m[1] === table;
}

/** The [start, end) line range of `table`'s body (excluding the header),
 *  or null when the table does not exist. */
function tableRange(lines: string[], table: string): { start: number; end: number } | null {
  let start = -1;
  for (const [i, line] of lines.entries()) {
    if (start < 0) {
      if (isHeaderOf(line, table)) start = i + 1;
    } else if (isTableHeader(line)) {
      return { start, end: i };
    }
  }
  return start < 0 ? null : { start, end: lines.length };
}

const keyLineRe = (key: string): RegExp => new RegExp(`^\\s*${key}\\s*=\\s*(.*?)\\s*$`);

/**
 * Read the string value of `[table] key`, or null when absent (or not a
 * simple single-line string). Display-only convenience — authoritative
 * parsing stays with `brink-project-config`; a value this reader cannot
 * model (multi-line, expression) reads as null and the form shows the raw
 * editor as the way to touch it.
 */
/**
 * Decode one TOML string literal, basic or literal, or null when `raw` is
 * not a single-line string.
 *
 * Shared by the scalar reader and the array reader so there is one answer
 * to "what counts as a string here" — an array member and a scalar value
 * are the same syntax, and two copies of this would drift.
 */
function decodeTomlString(raw: string): string | null {
  const basic = /^"((?:[^"\\]|\\.)*)"$/.exec(raw);
  if (basic !== null) return (basic[1] ?? "").replace(/\\(["\\])/g, "$1");
  const literal = /^'([^']*)'$/.exec(raw);
  if (literal !== null) return literal[1] ?? "";
  return null;
}

export function getTomlString(source: string, table: string, key: string): string | null {
  const lines = source.split("\n");
  const range = tableRange(lines, table);
  if (range === null) return null;
  const re = keyLineRe(key);
  for (let i = range.start; i < range.end; i++) {
    const m = re.exec(lines[i] ?? "");
    if (m === null) continue;
    return decodeTomlString((m[1] ?? "").replace(/\s*#.*$/, "").trim());
  }
  return null;
}

/** TOML basic-string encode (the two escapes simple values can need). */
function tomlString(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

/**
 * Set `[table] key` to `value`, or remove the key when `value` is null.
 * Targeted line edits only (see the module doc):
 *
 * - key exists → its one line is rewritten (indentation preserved);
 * - key absent, table exists → inserted after the table's last key line
 *   (or right under the header for an empty table);
 * - table absent → `[table]` + key appended at the end (removal of a key
 *   in a missing table is a no-op).
 */
/**
 * Write an already-rendered TOML value at `[table] key`, or remove the key
 * when `rendered` is null.
 *
 * The shared body behind {@link setTomlString} and {@link setTomlBool} —
 * they differ only in how the value is spelled, and duplicating the
 * table-location and insertion-point logic to vary a quoting rule would be
 * two places to get "insert after the table's last real line" wrong.
 */
function setTomlValue(
  source: string,
  table: string,
  key: string,
  rendered: string | null,
): string {
  const lines = source.split("\n");
  const range = tableRange(lines, table);
  const re = keyLineRe(key);

  if (range === null) {
    if (rendered === null) return source;
    const suffix = `[${table}]\n${key} = ${rendered}\n`;
    if (source === "") return suffix;
    return source.endsWith("\n") ? `${source}${suffix}` : `${source}\n${suffix}`;
  }

  for (let i = range.start; i < range.end; i++) {
    const line = lines[i] ?? "";
    if (!re.test(line)) continue;
    if (rendered === null) {
      lines.splice(i, 1);
    } else {
      const indent = /^\s*/.exec(line)?.[0] ?? "";
      lines[i] = `${indent}${key} = ${rendered}`;
    }
    return lines.join("\n");
  }

  if (rendered === null) return source;
  // Insert after the table's last non-blank, non-comment line so the key
  // joins the existing block rather than trailing the blank separator
  // before the next table.
  let insertAt = range.start;
  for (let i = range.start; i < range.end; i++) {
    const trimmed = (lines[i] ?? "").trim();
    if (trimmed !== "" && !trimmed.startsWith("#")) insertAt = i + 1;
  }
  lines.splice(insertAt, 0, `${key} = ${rendered}`);
  return lines.join("\n");
}

export function setTomlString(
  source: string,
  table: string,
  key: string,
  value: string | null,
): string {
  return setTomlValue(source, table, key, value === null ? null : tomlString(value));
}

/**
 * The keys present in `[table]`, in the order the file writes them (#3148).
 *
 * The Diagnostics section needs "which codes has this project decided
 * about", which is a question about key PRESENCE rather than any key's
 * value — and the answer is what puts a code in the configured list rather
 * than the unconfigured one.
 *
 * Same deliberate narrowness as the rest of this module: simple `key =`
 * lines only. A dotted or quoted key reads as absent, and the raw editor
 * remains the way to touch one.
 */
export function tomlTableKeys(source: string, table: string): string[] {
  const lines = source.split("\n");
  const range = tableRange(lines, table);
  if (range === null) return [];
  const keys: string[] = [];
  for (const line of lines.slice(range.start, range.end)) {
    const m = /^\s*([A-Za-z0-9_-]+)\s*=/.exec(line);
    if (m?.[1] !== undefined) keys.push(m[1]);
  }
  return keys;
}

/**
 * Read `[table] key` as a boolean, or null when absent or not literally
 * `true`/`false`.
 *
 * Separate from {@link getTomlString} because a bare `true` is not a TOML
 * string, and reading it as one would make `deny-warnings` invisible to the
 * form while it is plainly set in the file.
 */
export function getTomlBool(source: string, table: string, key: string): boolean | null {
  const lines = source.split("\n");
  const range = tableRange(lines, table);
  if (range === null) return null;
  const re = keyLineRe(key);
  for (const line of lines.slice(range.start, range.end)) {
    const m = re.exec(line);
    if (m) {
      const raw = (m[1] ?? "").replace(/\s*#.*$/, "").trim();
      if (raw === "true") return true;
      if (raw === "false") return false;
      return null;
    }
  }
  return null;
}

/** Write `[table] key = true|false`, or remove it when `value` is null. */
export function setTomlBool(
  source: string,
  table: string,
  key: string,
  value: boolean | null,
): string {
  return setTomlValue(source, table, key, value === null ? null : String(value));
}

/**
 * Read `[table] key` as an integer, or null when absent or not a bare
 * number.
 *
 * Separate from {@link getTomlString} for the reason {@link getTomlBool}
 * is: `indent = 4` is not a TOML string, so reading it as one would make
 * the key invisible to a form while it is plainly set in the file.
 */
export function getTomlInteger(source: string, table: string, key: string): number | null {
  const lines = source.split("\n");
  const range = tableRange(lines, table);
  if (range === null) return null;
  const re = keyLineRe(key);
  for (const line of lines.slice(range.start, range.end)) {
    const m = re.exec(line);
    if (m) {
      const raw = (m[1] ?? "").replace(/\s*#.*$/, "").trim();
      return /^-?\d+$/.test(raw) ? Number(raw) : null;
    }
  }
  return null;
}

/** Write `[table] key = <n>`, or remove it when `value` is null. */
export function setTomlInteger(
  source: string,
  table: string,
  key: string,
  value: number | null,
): string {
  return setTomlValue(source, table, key, value === null ? null : String(Math.trunc(value)));
}

// ── Arrays of strings ────────────────────────────────────────────────────
//
// The module doc above declares arrays out of scope, and for the settings
// form's scalar keys they still are. The prose dictionary forces the
// exception: it is an author-visible, author-editable, GROWING list, and
// the alternative — a sidecar file — was tried and rejected (decision log,
// "Prose dictionary lives in `brink.toml`").
//
// Growing is the operative word, and it is why these two functions cannot
// reuse `setTomlValue`. That function's whole model is "a key occupies one
// line", which a word list outgrows immediately: as a single line it
// becomes an unreadable, unmergeable smear the moment it has twenty
// entries. So the array is written one entry per line, and read back
// across however many lines it spans.

/** Split `body` on top-level commas — commas inside strings are content. */
function splitArrayItems(body: string): string[] {
  const items: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  let escaped = false;

  for (const ch of body) {
    if (quote !== null) {
      current += ch;
      // Only basic strings honour escapes; in a literal string a backslash
      // is a backslash, so `'C:\'` must not swallow the closing quote.
      if (escaped) escaped = false;
      else if (ch === "\\" && quote === '"') escaped = true;
      else if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      current += ch;
      continue;
    }
    if (ch === ",") {
      items.push(current);
      current = "";
      continue;
    }
    current += ch;
  }
  items.push(current);
  return items;
}

/** Strip a trailing `# comment` that is not inside a string. */
function stripComment(line: string): string {
  let quote: '"' | "'" | null = null;
  let escaped = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i] ?? "";
    if (quote !== null) {
      if (escaped) escaped = false;
      else if (ch === "\\" && quote === '"') escaped = true;
      else if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") quote = ch;
    else if (ch === "#") return line.slice(0, i);
  }
  return line;
}

/**
 * The `[start, end)` line range that `key`'s array value occupies, tracking
 * bracket depth so a multi-line array is covered to its closing `]`.
 *
 * Returns null when the key is absent or its value is not an array — the
 * same "reads as absent, raw editor is the escape hatch" contract the
 * scalar readers above use.
 */
function arrayLineRange(
  lines: string[],
  range: { start: number; end: number },
  key: string,
): { start: number; end: number } | null {
  const re = keyLineRe(key);
  for (let i = range.start; i < range.end; i++) {
    const line = lines[i] ?? "";
    const m = re.exec(line);
    if (m === null) continue;
    if (!(m[1] ?? "").trimStart().startsWith("[")) return null; // not an array
    let depth = 0;
    for (let j = i; j < range.end; j++) {
      const text = stripComment(lines[j] ?? "");
      let quote: '"' | "'" | null = null;
      let escaped = false;
      for (const ch of text) {
        if (quote !== null) {
          if (escaped) escaped = false;
          else if (ch === "\\" && quote === '"') escaped = true;
          else if (ch === quote) quote = null;
          continue;
        }
        if (ch === '"' || ch === "'") quote = ch;
        else if (ch === "[") depth++;
        else if (ch === "]") depth--;
      }
      if (depth <= 0) return { start: i, end: j + 1 };
    }
    // Unterminated array — the file is malformed. Reported as absent rather
    // than guessed at, so an edit never truncates a value we cannot see the
    // end of.
    return null;
  }
  return null;
}

/**
 * Read `[table] key` as an array of strings, or null when absent or not a
 * string array.
 *
 * Null and `[]` are different answers and both are load-bearing: null means
 * the project has never had a dictionary, `[]` means it has one and it is
 * empty. The settings view says different things for each.
 */
export function getTomlStringArray(source: string, table: string, key: string): string[] | null {
  const lines = source.split("\n");
  const range = tableRange(lines, table);
  if (range === null) return null;
  const arr = arrayLineRange(lines, range, key);
  if (arr === null) return null;

  const text = lines
    .slice(arr.start, arr.end)
    .map(stripComment)
    .join("\n");
  const open = text.indexOf("[");
  const close = text.lastIndexOf("]");
  if (open < 0 || close < open) return null;

  const out: string[] = [];
  for (const raw of splitArrayItems(text.slice(open + 1, close))) {
    const item = raw.trim();
    if (item === "") continue; // trailing comma, or an empty array
    const decoded = decodeTomlString(item);
    if (decoded === null) return null; // a non-string member: not ours to model
    out.push(decoded);
  }
  return out;
}

/**
 * Write `[table] key` as an array of strings, one entry per line, or remove
 * the key when `values` is null.
 *
 * Always multi-line, even for one entry, and never re-flowed to fit: the
 * list only grows, and a format that changes shape at some threshold
 * produces a diff where the whole array moved when one word was added.
 */
export function setTomlStringArray(
  source: string,
  table: string,
  key: string,
  values: readonly string[] | null,
): string {
  const lines = source.split("\n");
  const range = tableRange(lines, table);

  const rendered =
    values === null
      ? null
      : values.length === 0
        ? `${key} = []`
        : `${key} = [\n${values.map((v) => `  ${tomlString(v)},`).join("\n")}\n]`;

  if (range === null) {
    if (rendered === null) return source;
    const suffix = `[${table}]\n${rendered}\n`;
    if (source === "") return suffix;
    return source.endsWith("\n") ? `${source}${suffix}` : `${source}\n${suffix}`;
  }

  const existing = arrayLineRange(lines, range, key);
  if (existing !== null) {
    lines.splice(existing.start, existing.end - existing.start, ...(rendered === null ? [] : rendered.split("\n")));
    return lines.join("\n");
  }

  // No array there — but a SCALAR of the same name may be, and replacing it
  // is the right move (the key is the key). `setTomlValue`'s remove path
  // handles finding and deleting it.
  const cleared = setTomlValue(source, table, key, null);
  if (rendered === null) return cleared;

  const clearedLines = cleared.split("\n");
  const clearedRange = tableRange(clearedLines, table);
  if (clearedRange === null) return cleared;
  let insertAt = clearedRange.start;
  for (let i = clearedRange.start; i < clearedRange.end; i++) {
    const trimmed = (clearedLines[i] ?? "").trim();
    if (trimmed !== "" && !trimmed.startsWith("#")) insertAt = i + 1;
  }
  clearedLines.splice(insertAt, 0, ...rendered.split("\n"));
  return clearedLines.join("\n");
}
