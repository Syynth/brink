/**
 * `[cast]` edits for `brink.toml` (decision log 2026-10-10): the web
 * studio's twin of `brink-project-config`'s `ConfigDocument::{cast_members,
 * set_cast_color, remove_cast_member}`, with the same rules —
 *
 * - one `[cast.<name>]` table per speaker, `color` its one key;
 * - names match ignoring case, so editing `MARA` updates `[cast.Mara]`;
 * - targeted line edits, never a re-serialize: comments, blank lines and
 *   every other table survive (the `toml-edit.ts` contract);
 * - removing the last speaker gives back the text as it was.
 *
 * Only the table form is read and written. The parser also accepts an
 * inline `Mara = { color = "…" }` row under a `[cast]` header; the form
 * leaves those to the text editor, like every shape `toml-edit.ts` does not
 * model.
 */

import { tomlString } from "./toml-edit.js";

/** One declared speaker, as written. */
export interface CastEntry {
  name: string;
  /** `color` as written (unvalidated), or null when unset. */
  color: string | null;
}

interface Located extends CastEntry {
  /** The header's line index. */
  header: number;
  /** One past the last non-blank line of the body. */
  end: number;
}

const HEADER = /^\s*\[\s*cast\s*\.\s*(.+?)\s*\]\s*(?:#.*)?$/;
const ANY_HEADER = /^\s*\[/;
/** `color = <value> # comment`: a quoted value may itself hold a `#`. */
const COLOR = /^(\s*)color\s*=\s*("(?:[^"\\]|\\.)*"|'[^']*'|[^\s#]*)\s*(#.*)?$/;
const BARE_KEY = /^[A-Za-z0-9_-]+$/;

/** A one-line TOML string, basic or literal; null otherwise. */
function decodeString(raw: string): string | null {
  const basic = /^"((?:[^"\\]|\\.)*)"$/.exec(raw);
  if (basic !== null) return (basic[1] ?? "").replace(/\\(["\\])/g, "$1");
  const literal = /^'([^']*)'$/.exec(raw);
  return literal === null ? null : (literal[1] ?? "");
}

/** A table key as written: bare, or a quoted string; null otherwise. */
function decodeKey(raw: string): string | null {
  return BARE_KEY.test(raw) ? raw : decodeString(raw);
}

function locate(lines: string[]): Located[] {
  const found: Located[] = [];
  for (const [i, line] of lines.entries()) {
    const m = HEADER.exec(line);
    if (m === null) continue;
    const name = decodeKey(m[1] ?? "");
    if (name === null) continue;
    let end = i + 1;
    let color: string | null = null;
    for (let j = i + 1; j < lines.length && !ANY_HEADER.test(lines[j] ?? ""); j++) {
      const text = lines[j] ?? "";
      if (text.trim() !== "") end = j + 1;
      const c = COLOR.exec(text);
      if (c !== null) color = decodeString(c[2] ?? "");
    }
    found.push({ name, color, header: i, end });
  }
  return found;
}

const same = (a: string, b: string): boolean => a.toLowerCase() === b.toLowerCase();

/** The declared speakers, in the order written. */
export function castEntries(source: string): CastEntry[] {
  return locate(source.split("\n")).map(({ name, color }) => ({ name, color }));
}

/** `name`'s table key: bare when it can be, quoted otherwise. */
function renderKey(name: string): string {
  return BARE_KEY.test(name) ? name : tomlString(name);
}

/**
 * Set `name`'s colour (matched ignoring case). A speaker not yet in the
 * cast gets a new `[cast.<name>]` table at the end of the file.
 */
export function setCastColor(source: string, name: string, color: string): string {
  const lines = source.split("\n");
  const entry = locate(lines).find((e) => same(e.name, name));
  const rendered = `color = ${tomlString(color)}`;
  if (entry === undefined) {
    const body = source.length === 0 || source.endsWith("\n") ? source : `${source}\n`;
    const gap = body.length === 0 ? "" : "\n";
    return `${body}${gap}[cast.${renderKey(name)}]\n${rendered}\n`;
  }
  for (let i = entry.header + 1; i < entry.end; i++) {
    const c = COLOR.exec(lines[i] ?? "");
    if (c === null) continue;
    const comment = c[3] === undefined ? "" : ` ${c[3]}`;
    lines[i] = `${c[1] ?? ""}${rendered}${comment}`;
    return lines.join("\n");
  }
  lines.splice(entry.end, 0, rendered);
  return lines.join("\n");
}

/** Remove `name` (matched ignoring case); the text unchanged when absent. */
export function removeCastMember(source: string, name: string): string {
  const lines = source.split("\n");
  const entry = locate(lines).find((e) => same(e.name, name));
  if (entry === undefined) return source;
  lines.splice(entry.header, entry.end - entry.header);
  // Take one separating blank line with it, when that leaves no doubled
  // gap — so a table added at the end and removed again leaves no trace.
  const before = entry.header - 1;
  if (before >= 0 && (lines[before] ?? "").trim() === "" && (lines[entry.header] ?? "").trim() === "") {
    lines.splice(before, 1);
  }
  return lines.join("\n");
}
