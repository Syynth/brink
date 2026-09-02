/**
 * The `[dialogue]` section of `brink.toml` as one owned block (#3410).
 *
 * Key-level edits (`toml-edit.ts`) cannot write `[[dialogue.elements]]`,
 * an array of tables — and the Conventions editor owns the whole
 * `[dialogue]` table anyway: it is the resolution of what the author
 * taught, not a set of independent keys. So this road rewrites the
 * SECTION: from the `[dialogue]` header through every `[dialogue.*]` /
 * `[[dialogue.*]]` sub-table that follows, and nothing else — every other
 * byte of the file survives untouched.
 *
 * Round-trip rule (#3392: hand edits and the UI must round-trip): the
 * editor stamps the section with a marker carrying a hash of its body. A
 * section with no marker was written by hand; one whose hash no longer
 * matches was written by the editor and edited since. Both are `owner`
 * values the UI must ASK about before replacing — never overwrite a
 * hand-edited section silently.
 */

import { tomlString } from "./toml-edit.js";

/** One `[[dialogue.elements]]` row, keys as they read in TOML. */
export interface DialogueElementRow {
  kind: string;
  nature?: string;
  prefix?: string;
  suffix?: string;
  glued?: boolean;
  contentRole?: string;
  pattern?: string;
  template?: string;
}

/** The table form of `[dialogue]`. Structurally `@brink-lang/dialect`'s `DialogueConfig`. */
export interface DialogueTableSpec {
  preset?: string;
  runEndsAt?: string[];
  elements?: DialogueElementRow[];
}

/** What to write: the table form, or the file form pointing at an artifact. */
export type DialogueSpec = { form: "table"; table: DialogueTableSpec } | { form: "file"; file: string };

export const CONVENTIONS_MARKER = "# conventions-editor:";

/** Who last wrote the section, as far as the marker can tell. */
export type DialogueSectionOwner = "editor" | "hand" | "edited";

export interface DialogueSection {
  /** Line range [start, end) of the whole section, marker line included. */
  start: number;
  end: number;
  /** The section's text, marker line included, without a trailing newline. */
  text: string;
  owner: DialogueSectionOwner;
}

const headerName = (line: string): string | null => {
  const m = /^\s*\[\[?\s*([^\]]*?)\s*\]\]?\s*(?:#.*)?$/.exec(line);
  return m === null ? null : m[1];
};
const isDialogueHeader = (line: string): boolean => headerName(line) === "dialogue";
const belongsToDialogue = (line: string): boolean => {
  const name = headerName(line);
  return name !== null && (name === "dialogue" || name.startsWith("dialogue."));
};
const isAnyHeader = (line: string): boolean => headerName(line) !== null;

/** FNV-1a over the section body (everything but the marker line), with
 *  trailing whitespace per line ignored so an editor's trim never counts
 *  as an edit. */
export function sectionHash(body: string): string {
  let h = 0x811c9dc5;
  for (const line of body.split("\n")) {
    for (const ch of `${line.trimEnd()}\n`) {
      h ^= ch.codePointAt(0) ?? 0;
      h = Math.imul(h, 0x01000193) >>> 0;
    }
  }
  return h.toString(16).padStart(8, "0");
}

function markerLine(hash: string): string {
  return `${CONVENTIONS_MARKER} ${hash} — written by Settings › Conventions. Edit freely; the editor asks before rewriting this section.`;
}

/** Render `spec` as the section text, marker included, no trailing newline. */
export function renderDialogueSection(spec: DialogueSpec): string {
  const lines: string[] = ["[dialogue]"];
  if (spec.form === "file") {
    lines.push(`file = ${tomlString(spec.file)}`);
  } else {
    const t = spec.table;
    if (t.preset !== undefined) lines.push(`preset = ${tomlString(t.preset)}`);
    if (t.runEndsAt !== undefined && t.runEndsAt.length > 0) {
      lines.push(`run-ends-at = [${t.runEndsAt.map(tomlString).join(", ")}]`);
    }
    for (const el of t.elements ?? []) {
      lines.push("", "[[dialogue.elements]]", `kind = ${tomlString(el.kind)}`);
      if (el.nature !== undefined) lines.push(`nature = ${tomlString(el.nature)}`);
      if (el.prefix !== undefined) lines.push(`prefix = ${tomlString(el.prefix)}`);
      if (el.suffix !== undefined) lines.push(`suffix = ${tomlString(el.suffix)}`);
      if (el.glued) lines.push("glued = true");
      if (el.contentRole !== undefined) lines.push(`content-role = ${tomlString(el.contentRole)}`);
      if (el.pattern !== undefined) lines.push(`pattern = ${tomlString(el.pattern)}`);
      if (el.template !== undefined) lines.push(`template = ${tomlString(el.template)}`);
    }
  }
  const body = lines.join("\n");
  return `${markerLine(sectionHash(body))}\n${body}`;
}

/** Find the `[dialogue]` section, or null when the file has none. */
export function findDialogueSection(source: string): DialogueSection | null {
  const lines = source.split("\n");
  const headerAt = lines.findIndex(isDialogueHeader);
  if (headerAt < 0) return null;
  let end = headerAt + 1;
  while (end < lines.length && (!isAnyHeader(lines[end]) || belongsToDialogue(lines[end]))) end++;
  // Blank lines between the section and the next table stay with the file.
  while (end > headerAt + 1 && lines[end - 1].trim() === "") end--;
  let start = headerAt;
  const above = headerAt > 0 ? lines[headerAt - 1] : "";
  const marked = above.trimStart().startsWith(CONVENTIONS_MARKER);
  if (marked) start = headerAt - 1;
  const text = lines.slice(start, end).join("\n");
  let owner: DialogueSectionOwner = "hand";
  if (marked) {
    const stamped = /^\s*# conventions-editor:\s*([0-9a-f]{8})\b/.exec(above)?.[1] ?? "";
    const body = lines.slice(headerAt, end).join("\n");
    owner = stamped === sectionHash(body) ? "editor" : "edited";
  }
  return { start, end, text, owner };
}

/**
 * Replace the `[dialogue]` section with `block` (a rendered section, or
 * null to remove it). Absent → appended after a blank line. Every byte
 * outside the section is preserved.
 */
export function setDialogueSection(source: string, block: string | null): string {
  const lines = source.split("\n");
  const found = findDialogueSection(source);
  if (found === null) {
    if (block === null) return source;
    const trimmed = source.replace(/\n+$/, "");
    return trimmed === "" ? `${block}\n` : `${trimmed}\n\n${block}\n`;
  }
  const before = lines.slice(0, found.start);
  const after = lines.slice(found.end);
  if (block === null) {
    // Take one separating blank line with the section, not two.
    if (before.length > 0 && before[before.length - 1].trim() === "" && after.length > 0 && after[0].trim() === "") {
      after.shift();
    }
    return [...before, ...after].join("\n");
  }
  return [...before, ...block.split("\n"), ...after].join("\n");
}
