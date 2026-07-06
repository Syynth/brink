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

  /**
   * Mock of the real `rename_file` op (pure — computes edits, does not mutate
   * the session). Returns a `MoveResult`: `new_source` is the moved file's
   * content (outbound include rewriting is left to the real Rust op; the mock
   * keeps it verbatim), and `cross_file_edits` rewrite any other file whose
   * `INCLUDE` names the old basename to the new one — enough to exercise the
   * studio's apply/egress plumbing. The real inbound/outbound math is covered
   * by Rust unit tests in brink-ide.
   */
  rename_file(oldPath: string, newPath: string): string {
    const source = this.files.get(oldPath);
    if (source === undefined) {
      return JSON.stringify({ ok: false, error: "file not loaded" });
    }
    if (oldPath !== newPath && this.files.has(newPath)) {
      return JSON.stringify({ ok: false, error: `a file already exists at '${newPath}'` });
    }
    const oldBase = oldPath.split("/").pop()!;
    const newBase = newPath.split("/").pop()!;
    const escaped = oldBase.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const includeRe = new RegExp(`(INCLUDE\\s+\\S*?)${escaped}\\b`, "g");
    const crossFileEdits: { path: string; new_source: string }[] = [];
    if (oldPath !== newPath) {
      for (const [p, src] of this.files) {
        if (p === oldPath) continue;
        const rewritten = src.replace(includeRe, `$1${newBase}`);
        if (rewritten !== src) {
          crossFileEdits.push({ path: p, new_source: rewritten });
        }
      }
    }
    return JSON.stringify({
      ok: true,
      path: oldPath,
      new_source: source,
      cross_file_edits: crossFileEdits,
      // Unified StructuralResult gate (#316). The mock does not model the
      // INCLUDE-graph breakage, so a rename is reported safe.
      introduced_diagnostics: [],
      safe: true,
    });
  }

  /**
   * Mock of the real `delete_symbol` op (#316). Removes the named knot's whole
   * region (header + body + nested stitches) or a stitch's region, and reports
   * `E020`-style breakage when any other line still diverts/threads to the
   * removed symbol — enough to drive the studio's safe-by-default report. The
   * precise dangling-reference math is covered by Rust tests.
   */
  delete_symbol(path: string, knot: string, stitch: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return JSON.stringify({ ok: false, error: "file not loaded" });
    }
    const name = stitch || knot;
    const lines = source.split("\n");
    const headerRe = stitch
      ? new RegExp(`^\\s*=\\s+${name}\\b`)
      : new RegExp(`^\\s*={2,3}\\s*${name}\\b`);
    const start = lines.findIndex((l) => headerRe.test(l));
    if (start < 0) {
      return JSON.stringify({ ok: false, error: "symbol not found" });
    }
    // The region runs until the next header at the same-or-shallower level.
    const stopRe = stitch ? /^\s*={1,3}/ : /^\s*={2,3}/;
    let end = start + 1;
    while (end < lines.length && !stopRe.test(lines[end]!)) end++;
    const kept = [...lines.slice(0, start), ...lines.slice(end)];
    const newSource = kept.join("\n");

    // Breakage: any remaining `-> name` / `<- name` (here or in another file).
    const esc = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const refRe = new RegExp(`(?:->|<-)\\s*${esc}\\b`);
    const introduced: {
      severity: string;
      code: string;
      message: string;
      path: string;
      line: number;
      col: number;
    }[] = [];
    const scan = (p: string, src: string) => {
      src.split("\n").forEach((l, i) => {
        if (refRe.test(l)) {
          introduced.push({
            severity: "error",
            code: "E020",
            message: `unresolved divert to '${name}'`,
            path: p,
            line: i + 1,
            col: 1,
          });
        }
      });
    };
    scan(path, newSource);
    for (const [p, src] of this.files) {
      if (p !== path) scan(p, src);
    }

    return JSON.stringify({
      ok: true,
      path,
      new_source: newSource,
      cross_file_edits: [],
      introduced_diagnostics: introduced,
      safe: introduced.length === 0,
    });
  }

  /**
   * Mock of the real `extract_to_knot` op (#315 H): lift the selected lines
   * (snapped to whole lines) into a new top-level `=== name ===` knot ending
   * with a `->->` tunnel return, and replace them with `-> name ->`. Offsets are
   * whole-file UTF-16 (== byte offsets for the ASCII fixtures the studio tests
   * use). The precise scope-breakage gate is covered by Rust tests; the mock
   * always reports `safe: true`.
   */
  extract_to_knot(path: string, startOffset: number, endOffset: number, name: string): string {
    return this.extractImpl(path, startOffset, endOffset, name, "knot");
  }

  /**
   * Mock of the real `extract_to_function` op (#315 H): as {@link extract_to_knot}
   * but into a `=== function name() ===` decl, replacing the selection with
   * `~ name()`.
   */
  extract_to_function(path: string, startOffset: number, endOffset: number, name: string): string {
    return this.extractImpl(path, startOffset, endOffset, name, "function");
  }

  private extractImpl(
    path: string,
    startOffset: number,
    endOffset: number,
    name: string,
    kind: "knot" | "function",
  ): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return JSON.stringify({ ok: false, error: "file not loaded" });
    }
    const lo = Math.min(startOffset, endOffset);
    const hi = Math.max(startOffset, endOffset);
    if (lo === hi) {
      return JSON.stringify({ ok: false, error: "empty selection: nothing to extract" });
    }
    // Snap to whole lines.
    const selStart = source.lastIndexOf("\n", lo - 1) + 1;
    const nextNl = source.indexOf("\n", hi);
    const selEnd = nextNl < 0 ? source.length : nextNl + 1;
    const selected = source.slice(selStart, selEnd);

    const call = kind === "knot" ? `-> ${name} ->\n` : `~ ${name}()\n`;
    const header = kind === "knot" ? `=== ${name} ===\n` : `=== function ${name}() ===\n`;
    let body = selected.endsWith("\n") ? selected : `${selected}\n`;
    if (kind === "knot") body += "->->\n";

    let out = source.slice(0, selStart) + call + source.slice(selEnd);
    if (!out.endsWith("\n")) out += "\n";
    if (!out.endsWith("\n\n")) out += "\n";
    out += header + body;

    return JSON.stringify({
      ok: true,
      path,
      new_source: out,
      cross_file_edits: [],
      introduced_diagnostics: [],
      safe: true,
    });
  }

  /**
   * Mock of the real `rename_symbol` op (pure — computes edits, does not
   * mutate the session). Rewrites the symbol's header plus `->`/`<-` diverts
   * to it across every file, and flags an `E022` breakage when renaming a knot
   * onto an existing top-level knot name (the safe-by-default gate, #305). The
   * precise rename + diagnostic-diff math is covered by Rust tests; the mock is
   * enough to drive the studio prompt/report plumbing.
   */
  rename_symbol(path: string, knot: string, stitch: string, newName: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return JSON.stringify({ ok: false, error: "file not loaded" });
    }
    const oldName = stitch || knot;
    const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

    // Breakage: renaming a knot onto an existing top-level knot collides.
    const introduced: {
      severity: string;
      code: string;
      message: string;
      path: string;
      line: number;
      col: number;
    }[] = [];
    if (!stitch && newName !== knot) {
      const lines = source.split("\n");
      const collisionLine = lines.findIndex(
        (l) => new RegExp(`^\\s*={2,3}\\s*${esc(newName)}\\b`).test(l),
      );
      if (collisionLine >= 0) {
        introduced.push({
          severity: "error",
          code: "E022",
          message: "duplicate knot definition",
          path,
          line: collisionLine + 1,
          col: 1,
        });
      }
    }

    // Rewrite a file's references to `oldName` → `newName`: the header (knot
    // `=== name ===` or stitch `= name`) plus diverts/threads (`-> name`,
    // `<- name`, qualified `knot.name`).
    const rewrite = (src: string): string => {
      let out = src;
      if (stitch) {
        out = out.replace(new RegExp(`(^|\\n)(\\s*=\\s*)${esc(oldName)}\\b`, "g"), `$1$2${newName}`);
      } else {
        out = out.replace(
          new RegExp(`(={2,3}\\s*)${esc(oldName)}(\\s*={0,3})`, "g"),
          `$1${newName}$2`,
        );
      }
      out = out.replace(new RegExp(`((?:->|<-)\\s*)${esc(oldName)}\\b`, "g"), `$1${newName}`);
      return out;
    };

    const newSource = rewrite(source);
    const crossFileEdits: { path: string; new_source: string }[] = [];
    for (const [p, src] of this.files) {
      if (p === path) continue;
      const rewritten = rewrite(src);
      if (rewritten !== src) crossFileEdits.push({ path: p, new_source: rewritten });
    }

    return JSON.stringify({
      ok: true,
      path,
      new_source: newSource,
      cross_file_edits: crossFileEdits,
      introduced_diagnostics: introduced,
      safe: introduced.length === 0,
    });
  }

  /**
   * Offset-based rename (F2). Resolves the knot/stitch whose *declaration name*
   * the UTF-16 file `offset` lands in, then delegates to `rename_symbol`. The
   * mock only resolves declaration sites (enough for the plumbing); the real
   * wasm also resolves references and non-container symbols.
   */
  rename_symbol_at(path: string, offset: number, newName: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return JSON.stringify({ ok: false, error: "file not loaded" });
    }
    for (const knot of parseOutline(source)) {
      if (offset >= knot.start && offset <= knot.end) {
        return this.rename_symbol(path, knot.name, "", newName);
      }
      for (const st of knot.children) {
        if (offset >= st.start && offset <= st.end) {
          return this.rename_symbol(path, knot.name, st.name, newName);
        }
      }
    }
    return JSON.stringify({ ok: false, error: "cannot rename this symbol" });
  }

  // Host-capability manifest + value cache (#174) — no-ops in the mock.
  set_host_manifest(_json: string): void { /* no-op */ }
  clear_host_manifest(): void { /* no-op */ }
  set_host_values(_json: string): void { /* no-op */ }
  clear_host_values(): void { /* no-op */ }

  // Dialogue dialect (#368) — no-ops in the mock; `line_contexts_doc`/
  // `line_contexts` always return "[]" here, so there is no dialect facet
  // to populate either way. Mirrors the host-manifest no-op pattern above.
  set_dialect(_json: string): void { /* no-op */ }
  clear_dialect(): void { /* no-op */ }

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

  /**
   * Mock of `auto_import_include_doc` (#312 F): report whether `target` is
   * reachable from the file backing `doc` and, when not, the whole-file
   * INCLUDE-insertion edit. The mock's reachability is a plain substring check
   * for an `INCLUDE <target-basename>` line — enough to drive the studio
   * accept path. The edit always inserts at file top (offset 0).
   */
  auto_import_include_doc(doc: number, target: string): string {
    const d = this.docs.get(doc);
    if (!d) {
      return JSON.stringify({ ok: false, already_reachable: false, error: "unknown handle" });
    }
    const source = this.files.get(d.path) ?? "";
    const base = target.split("/").pop()!;
    const reachable = new RegExp(`^INCLUDE\\s+\\S*${base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "m").test(source);
    if (reachable) {
      return JSON.stringify({ ok: true, already_reachable: true });
    }
    return JSON.stringify({
      ok: true,
      already_reachable: false,
      edit: { from: 0, to: 0, insert: `INCLUDE ${base}\n` },
    });
  }

  /**
   * Mock of `auto_import_apply_include_doc` (#312 F, fragment-view path): apply
   * the INCLUDE to the whole file AND rebase every open fragment view on that
   * file that begins at/after the insertion point. This mirrors the real op —
   * without the rebase, the next `update_document` fragment splice would clobber
   * the INCLUDE line (the very bug under test). Returns the applied edit (as a
   * shift descriptor) with no expectation the caller re-applies it.
   */
  auto_import_apply_include_doc(doc: number, target: string): string {
    const d = this.docs.get(doc);
    if (!d) {
      return JSON.stringify({ ok: false, already_reachable: false, error: "unknown handle" });
    }
    const source = this.files.get(d.path) ?? "";
    const base = target.split("/").pop()!;
    const reachable = new RegExp(`^INCLUDE\\s+\\S*${base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "m").test(source);
    if (reachable) {
      return JSON.stringify({ ok: true, already_reachable: true });
    }
    const insert = `INCLUDE ${base}\n`;
    const at = 0;
    // Apply at file top.
    this.files.set(d.path, insert + source);
    // Rebase every open fragment view on this file whose range starts at/after
    // the insertion point.
    const delta = insert.length;
    for (const od of this.docs.values()) {
      if (od.path !== d.path) continue;
      if (od.viewStart != null && od.viewStart >= at) od.viewStart += delta;
      if (od.viewEnd != null && od.viewEnd >= at) od.viewEnd += delta;
    }
    return JSON.stringify({
      ok: true,
      already_reachable: false,
      edit: { from: at, to: at, insert },
    });
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

/** Deterministic stand-in for the source-identity checksum: a stable hash of
 * the bytes, formatted like the real `0x{:08x}` — distinct bytes → distinct
 * value, so degraded-mode comparisons behave. */
export function program_checksum(bytes: Uint8Array): string {
  let sum = 0;
  for (const b of bytes) sum = (sum + b) >>> 0;
  return "0x" + sum.toString(16).padStart(8, "0");
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
  // Shared-flow surface (#200): a minimal in-memory flow registry so the studio
  // multi-flow path is exercisable. Each flow ends immediately, like the mock
  // story.
  private flows = new Set<string>();
  spawn_flow(name: string, _path?: string): void { this.flows.add(name); }
  continue_flow(_name: string): string { return JSON.stringify({ type: "end", text: "", tags: [] }); }
  choose_flow(_name: string, _index: number): void { /* no-op */ }
  destroy_flow(name: string): void { this.flows.delete(name); }
  flow_names(): string { return JSON.stringify([...this.flows].sort()); }
  flow_debug_snapshot(_name: string): string {
    return JSON.stringify({
      status: "ended", current_location: null, turn_index: 0,
      globals: [], call_stack: [], visit_counts: [], pending_choices: [],
      rng: { seed: 0, previous: 0 },
    });
  }
}

/** Pure-diff stand-in for the real `diffSnapshots` wasm export — the mock
 * carries no snapshot state, so this exists only so `StorySessionHandle`'s
 * import resolves; nothing in the studio test suite calls it against real
 * snapshot data. */
export function diffSnapshots(_a: string, _b: string): string {
  return JSON.stringify({
    added_globals: {}, removed_globals: {}, changed_globals: {},
    list_deltas: {}, pushed_frames: [], popped_frames: [],
  });
}

/**
 * Minimal stand-in for the real `WebSession` (#390's `StorySessionHandle`
 * over `crates/brink-web`). Every journal-mutating call bumps an in-memory
 * event counter one-for-one — enough to exercise `StorySessionHandle`'s
 * TS-side deferred+debounced `onJournalDirty` hook (the behavior under test)
 * without reimplementing the Rust session/journal semantics. Story content is
 * a fixed two-line-then-`done` script; it does not parse `_storyBytes`.
 */
export class WebSession {
  private events = 0;
  private turn = 0;

  constructor(_storyBytes: Uint8Array, _seed?: number, _deferred?: string[]) { /* no-op */ }

  private bumpAndLine(): string {
    this.events += 1;
    this.turn += 1;
    if (this.turn === 1) {
      return JSON.stringify({
        type: "line",
        line: { type: "text", text: "Hello, world!\n", tags: [] },
      });
    }
    return JSON.stringify({
      type: "line",
      line: { type: "done", text: "", tags: [] },
    });
  }

  advance(): string { return this.bumpAndLine(); }
  continue_single(): string {
    const outcome = JSON.parse(this.bumpAndLine()) as { line: unknown };
    return JSON.stringify(outcome.line);
  }
  continue_to_pause(): string {
    const outcome = JSON.parse(this.bumpAndLine()) as { line: unknown };
    return JSON.stringify([outcome.line]);
  }
  choose(_index: number): void { this.events += 1; }
  resolve_external(_value: unknown): void { this.events += 1; }
  has_pending_external(): boolean { return false; }
  set_var(_name: string, _value: unknown): boolean { this.events += 1; return true; }
  go_to_path(_path: string, _args: unknown[]): void { this.events += 1; }
  save_state(): string { return JSON.stringify({ globals: {}, visited: [], turn_index: this.turn }); }
  load_state(_json: string): void { this.events += 1; }
  call_function(_name: string, _args: unknown[]): unknown { this.events += 1; return null; }
  snapshot(): string {
    return JSON.stringify({
      globals: {}, lists: {}, turn_index: this.turn, visit_counts: {},
      turn_counts: {}, call_stack: [], status: "active",
    });
  }
  diff(a: string, b: string): string { return diffSnapshots(a, b); }
  journal_event_count(): number { return this.events; }
  export_journal(): string {
    return JSON.stringify({
      version: 1, program_checksum: 0, events: [], truncated: false,
    });
  }
  static restore(
    _storyBytes: Uint8Array,
    _journalJson: string,
    _seed?: number,
    _deferred?: string[],
  ): WebSession {
    return new WebSession(_storyBytes, _seed, _deferred);
  }
  last_replay_outcome(): string | undefined { return undefined; }
  reload(_storyBytes: Uint8Array): string {
    this.events += 1;
    return JSON.stringify({ type: "replayed", warnings: [] });
  }
  continue_replay(): string {
    this.events += 1;
    return JSON.stringify({ type: "replayed", warnings: [] });
  }
  restart(): void { this.events = 0; this.turn = 0; }
  free(): void { /* no-op */ }
}
