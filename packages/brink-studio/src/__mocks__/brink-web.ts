/**
 * Mock wasm module for testing.
 *
 * Implements the same interface as the real brink-web wasm package
 * but stores files in memory and returns minimal JSON responses.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

export default function init(): Promise<void> {
  return Promise.resolve();
}

export class EditorSession {
  private files = new Map<string, string>();
  private activePath = "";

  update_source(_source: string): void {
    // no-op in mock
  }

  update_file(path: string, source: string): void {
    this.files.set(path, source);
  }

  remove_file(path: string): void {
    this.files.delete(path);
  }

  set_active_file(path: string): boolean {
    if (this.files.has(path)) {
      this.activePath = path;
      return true;
    }
    return false;
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
    for (const [path] of this.files) {
      outline.push({ path, symbols: [] });
    }
    return JSON.stringify(outline);
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
  file_symbols(_path: string): string { return "[]"; }
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
  continue_story(): string { return JSON.stringify({ status: "ended" }); }
  choose(_index: number): void { /* no-op */ }
  reset(): void { /* no-op */ }
  free(): void { /* no-op */ }
}
