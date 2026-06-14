/**
 * Mock wasm module for testing.
 *
 * Implements the same interface as the real brink-web wasm package
 * but stores files in memory and returns minimal JSON responses.
 * Parses `=== knot ===` and `= stitch` headers to produce outlines.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

export default function init(): Promise<void> {
  return Promise.resolve();
}

interface MockSymbol {
  name: string;
  kind: string;
  start: number;
  end: number;
  full_start: number;
  full_end: number;
  children: MockSymbol[];
}

/** Parse knot/stitch headers from ink source for outline generation. */
function parseOutline(source: string): MockSymbol[] {
  const symbols: MockSymbol[] = [];
  const lines = source.split("\n");
  let offset = 0;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    const knotMatch = line.match(/^===\s+(\w+)\s*===/);
    if (knotMatch) {
      const name = knotMatch[1]!;
      const nameStart = offset + line.indexOf(name);
      const nameEnd = nameStart + name.length;
      symbols.push({
        name,
        kind: "knot",
        start: nameStart,
        end: nameEnd,
        full_start: offset,
        full_end: 0, // filled in below
        children: [],
      });
    }

    const stitchMatch = line.match(/^=\s+(\w+)/);
    if (stitchMatch && !knotMatch) {
      const name = stitchMatch[1]!;
      const nameStart = offset + line.indexOf(name);
      const nameEnd = nameStart + name.length;
      const parent = symbols[symbols.length - 1];
      if (parent) {
        parent.children.push({
          name,
          kind: "stitch",
          start: nameStart,
          end: nameEnd,
          full_start: offset,
          full_end: 0,
          children: [],
        });
      }
    }

    offset += line.length + 1; // +1 for \n
  }

  // Fill in full_end for each symbol
  for (let i = 0; i < symbols.length; i++) {
    const next = symbols[i + 1];
    symbols[i]!.full_end = next ? next.full_start : source.length;

    const knot = symbols[i]!;
    for (let j = 0; j < knot.children.length; j++) {
      const nextChild = knot.children[j + 1];
      knot.children[j]!.full_end = nextChild ? nextChild.full_start : knot.full_end;
    }
  }

  return symbols;
}

interface MockDoc {
  path: string;
  viewStart: number | null;
  viewEnd: number | null;
}

export class EditorSession {
  private files = new Map<string, string>();
  private activePath = "";
  private docs = new Map<number, MockDoc>();
  private nextDocId = 1;

  update_source(source: string): void {
    if (this.viewStart != null && this.viewEnd != null) {
      const full = this.files.get(this.activePath) ?? "";
      const before = full.slice(0, this.viewStart);
      const after = full.slice(this.viewEnd);
      this.files.set(this.activePath, before + source + after);
      this.viewEnd = this.viewStart + source.length;
    } else {
      this.files.set(this.activePath, source);
    }
  }

  update_file(path: string, source: string): void {
    this.files.set(path, source);
  }

  remove_file(path: string): void {
    this.files.delete(path);
  }

  private viewStart: number | null = null;
  private viewEnd: number | null = null;

  set_active_file(path: string): boolean {
    if (this.files.has(path)) {
      this.activePath = path;
      this.viewStart = null;
      this.viewEnd = null;
      return true;
    }
    return false;
  }

  set_view_context(start: number, end: number): void {
    this.viewStart = start;
    this.viewEnd = end;
  }

  clear_view_context(): void {
    this.viewStart = null;
    this.viewEnd = null;
  }

  get_view_source(): string {
    const content = this.files.get(this.activePath);
    if (content == null) return JSON.stringify(null);
    if (this.viewStart != null && this.viewEnd != null) {
      return JSON.stringify(content.slice(this.viewStart, this.viewEnd));
    }
    return JSON.stringify(content);
  }

  active_file(): string {
    return this.activePath;
  }

  list_files(): string {
    return JSON.stringify([...this.files.keys()].map((p) => ({ path: p })));
  }

  get_file_source(path: string): string {
    const content = this.files.get(path);
    return JSON.stringify(content ?? null);
  }

  compile_project(_entry: string): string {
    return JSON.stringify({ ok: true });
  }

  project_outline(): string {
    const outline = [];
    for (const [path, source] of this.files) {
      outline.push({ path, symbols: parseOutline(source) });
    }
    return JSON.stringify(outline);
  }

  /**
   * Story graph (#96): nodes derived from the same header parse as the
   * outline (knots + stitches with parent ids), no edges. The real edge
   * extraction is covered by Rust tests in brink-ide/brink-web.
   */
  story_graph(): string {
    const nodes = [];
    for (const [path, source] of this.files) {
      for (const sym of parseOutline(source)) {
        nodes.push({
          id: sym.name,
          name: sym.name,
          kind: "knot",
          file: path,
          start: sym.start,
          end: sym.end,
        });
        for (const child of sym.children) {
          const id = `${sym.name}.${child.name}`;
          nodes.push({
            id,
            name: id,
            kind: "stitch",
            file: path,
            start: child.start,
            end: child.end,
            parent: sym.name,
          });
        }
      }
    }
    nodes.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    return JSON.stringify({ nodes, edges: [] });
  }

  // ── Document handles (mirrors brink-web's multi-document API) ──

  open_document(path: string): number {
    if (!this.files.has(path)) return 0;
    const id = this.nextDocId++;
    this.docs.set(id, { path, viewStart: null, viewEnd: null });
    return id;
  }

  open_fragment(path: string, start: number, end: number): number {
    if (!this.files.has(path)) return 0;
    const id = this.nextDocId++;
    this.docs.set(id, { path, viewStart: start, viewEnd: end });
    return id;
  }

  close_document(doc: number): boolean {
    return this.docs.delete(doc);
  }

  update_document(doc: number, source: string): string {
    const d = this.docs.get(doc);
    if (!d) return "null";
    const full = this.files.get(d.path) ?? "";
    if (d.viewStart != null && d.viewEnd != null) {
      const start = d.viewStart;
      const end = d.viewEnd;
      const before = full.slice(0, start);
      const after = full.slice(end);
      // The real splice maintains a "\n" separator after the fragment when
      // the original view boundary sat on one and the new text doesn't end
      // with it; the simple mock just splices verbatim.
      this.files.set(d.path, before + source + after);
      d.viewEnd = start + source.length;
      return JSON.stringify({ path: d.path, start, end });
    }
    const prevLength = full.length;
    this.files.set(d.path, source);
    return JSON.stringify({ path: d.path, start: 0, end: prevLength });
  }

  get_view_source_doc(doc: number): string {
    const d = this.docs.get(doc);
    if (!d) return JSON.stringify(null);
    const content = this.files.get(d.path);
    if (content == null) return JSON.stringify(null);
    if (d.viewStart != null && d.viewEnd != null) {
      return JSON.stringify(content.slice(d.viewStart, d.viewEnd));
    }
    return JSON.stringify(content);
  }

  line_contexts_doc(_doc: number): string { return "[]"; }
  semantic_tokens_doc(_doc: number): string { return "[]"; }
  completions_doc(_doc: number, _offset: number): string { return "[]"; }
  hover_doc(_doc: number, _offset: number): string { return "null"; }
  goto_definition_doc(_doc: number, _offset: number): string { return "null"; }
  find_references_doc(_doc: number, _offset: number): string { return "[]"; }
  prepare_rename_doc(_doc: number, _offset: number): string { return "null"; }
  rename_doc(_doc: number, _offset: number, _name: string): string { return "[]"; }
  code_actions_doc(_doc: number, _offset: number): string { return "[]"; }
  inlay_hints_doc(_doc: number, _start: number, _end: number): string { return "[]"; }
  signature_help_doc(_doc: number, _offset: number): string { return "null"; }
  folding_ranges_doc(_doc: number): string { return "[]"; }
  document_symbols_doc(_doc: number): string { return "[]"; }
  convert_element_doc(_doc: number, _offset: number, _target: string): string { return "null"; }
  format_document_doc(_doc: number): string { return '""'; }

  /** Outline-shaped symbols for one file (used by symbol-range resolution). */
  file_symbols(path: string): string {
    const source = this.files.get(path);
    return JSON.stringify(source == null ? [] : parseOutline(source));
  }

  semantic_tokens(): string { return "[]"; }
  completions(_offset: number): string { return "[]"; }
  hover(_offset: number): string { return "null"; }
  goto_definition(_offset: number): string { return "null"; }
  find_references(_offset: number): string { return "[]"; }
  prepare_rename(_offset: number): string { return "null"; }
  rename(_offset: number, _name: string): string { return "[]"; }
  code_actions(_offset: number): string { return "[]"; }
  inlay_hints(_start: number, _end: number): string { return "[]"; }
  signature_help(_offset: number): string { return "null"; }
  folding_ranges(): string { return "[]"; }
  document_symbols(): string { return "[]"; }
  file_includes(_path: string): string { return "[]"; }
  line_contexts(): string { return "[]"; }
  format_document(): string { return '""'; }
  convert_element(_offset: number, _target: string): string { return "null"; }
  free(): void { /* no-op */ }
}

export function compile(_source: string): string {
  return JSON.stringify({ ok: true });
}

export function token_type_names(): string {
  return JSON.stringify(["comment", "keyword", "string", "number", "function", "variable"]);
}

export function token_modifier_names(): string {
  return JSON.stringify([]);
}

export class StoryRunner {
  constructor(_bytes: Uint8Array) { /* no-op */ }
  continue_story(): string { return JSON.stringify([{ type: "end", text: "", tags: [] }]); }
  continue_single(): string { return JSON.stringify({ type: "end", text: "", tags: [] }); }
  choose(_index: number): void { /* no-op */ }
  reset(): void { /* no-op */ }
  free(): void { /* no-op */ }
  // Replay-recording surface (mirrors the real StoryRunner; #173/#189): the
  // mock records nothing, so has_recording() is always false → the studio's
  // post-reload re-walk runs live, exactly as before this feature.
  reload(_bytes: Uint8Array): void { /* no-op */ }
  begin_replay(): void { /* no-op */ }
  end_replay(): void { /* no-op */ }
  has_recording(): boolean { return false; }
}
