/**
 * @brink/wasm — Wasm FFI bindings to brink-web.
 *
 * Wraps the raw wasm module classes in ergonomic TypeScript wrappers
 * that parse JSON responses into typed interfaces from @brink/wasm-types.
 */

import init, {
  compile as wasmCompile,
  token_type_names,
  token_modifier_names,
  EditorSession as WasmEditorSession,
  StoryRunner,
} from "brink-web";

import type {
  CompileResult,
  SemanticToken,
  CompletionItem,
  HoverInfo,
  Location,
  FileEdit,
  InlayHint,
  SignatureInfo,
  FoldRange,
  DocumentSymbol,
  CodeAction,
  ProjectFile,
  FileOutline,
  LineContext,
  ConvertTarget,
  TextEdit,
  IncludeInfo,
  StepResult,
} from "@brink/wasm-types";

// ── Wasm initialization ─────────────────────────────────────────

export async function initWasm(): Promise<void> {
  await init();
}

// ── Compilation ─────────────────────────────────────────────────

export function compile(source: string): CompileResult {
  const json = wasmCompile(source);
  return JSON.parse(json) as CompileResult;
}

// ── Token legend (stateless) ────────────────────────────────────

let cachedTypeNames: string[] | null = null;
let cachedModifierNames: string[] | null = null;

export function getTokenTypeNames(): string[] {
  if (!cachedTypeNames) {
    cachedTypeNames = JSON.parse(token_type_names()) as string[];
  }
  return cachedTypeNames;
}

export function getTokenModifierNames(): string[] {
  if (!cachedModifierNames) {
    cachedModifierNames = JSON.parse(token_modifier_names()) as string[];
  }
  return cachedModifierNames;
}

// ── EditorSession wrapper ───────────────────────────────────────

export class EditorSessionHandle {
  private session: WasmEditorSession;

  constructor() {
    this.session = new WasmEditorSession();
  }

  updateSource(source: string): void {
    this.session.update_source(source);
  }

  updateFile(path: string, source: string): void {
    this.session.update_file(path, source);
  }

  removeFile(path: string): void {
    this.session.remove_file(path);
  }

  setActiveFile(path: string): boolean {
    return this.session.set_active_file(path);
  }

  getActiveFile(): string {
    return this.session.active_file();
  }

  /** Scope IDE queries to a sub-region `[start, end)` of the active file. */
  setViewContext(start: number, end: number): void {
    this.session.set_view_context(start, end);
  }

  /** Return to full-file mode. */
  clearViewContext(): void {
    this.session.clear_view_context();
  }

  /** Get the source text for the current view context (fragment or full file). */
  getViewSource(): string | null {
    const json = this.session.get_view_source();
    const result = JSON.parse(json);
    return result ?? null;
  }

  listFiles(): ProjectFile[] {
    const json = this.session.list_files();
    return JSON.parse(json) as ProjectFile[];
  }

  getFileSource(path: string): string | null {
    const json = this.session.get_file_source(path);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getFileSymbols(path: string): DocumentSymbol[] {
    const json = this.session.file_symbols(path);
    return JSON.parse(json) as DocumentSymbol[];
  }

  compileProject(entry: string): CompileResult {
    const json = this.session.compile_project(entry);
    return JSON.parse(json) as CompileResult;
  }

  getProjectOutline(): FileOutline[] {
    const json = this.session.project_outline();
    return JSON.parse(json) as FileOutline[];
  }

  getLineContexts(): LineContext[] {
    const json = this.session.line_contexts();
    return JSON.parse(json) as LineContext[];
  }

  getSemanticTokens(): SemanticToken[] {
    const json = this.session.semantic_tokens();
    return JSON.parse(json) as SemanticToken[];
  }

  getCompletions(offset: number): CompletionItem[] {
    const json = this.session.completions(offset);
    return JSON.parse(json) as CompletionItem[];
  }

  getHover(offset: number): HoverInfo | null {
    const json = this.session.hover(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  gotoDefinition(offset: number): Location | null {
    const json = this.session.goto_definition(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  findReferences(offset: number): Location[] {
    const json = this.session.find_references(offset);
    return JSON.parse(json) as Location[];
  }

  prepareRename(offset: number): Location | null {
    const json = this.session.prepare_rename(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  doRename(offset: number, newName: string): FileEdit[] {
    const json = this.session.rename(offset, newName);
    return JSON.parse(json) as FileEdit[];
  }

  getCodeActions(offset: number): CodeAction[] {
    const json = this.session.code_actions(offset);
    return JSON.parse(json) as CodeAction[];
  }

  getInlayHints(start: number, end: number): InlayHint[] {
    const json = this.session.inlay_hints(start, end);
    return JSON.parse(json) as InlayHint[];
  }

  getSignatureHelp(offset: number): SignatureInfo | null {
    const json = this.session.signature_help(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getFoldingRanges(): FoldRange[] {
    const json = this.session.folding_ranges();
    return JSON.parse(json) as FoldRange[];
  }

  getDocumentSymbols(): DocumentSymbol[] {
    const json = this.session.document_symbols();
    return JSON.parse(json) as DocumentSymbol[];
  }

  getFileIncludes(path: string): IncludeInfo[] {
    const json = this.session.file_includes(path);
    return JSON.parse(json) as IncludeInfo[];
  }

  formatDocument(): string {
    const json = this.session.format_document();
    return JSON.parse(json) as string;
  }

  convertElement(offset: number, target: ConvertTarget): TextEdit | null {
    const json = this.session.convert_element(offset, target);
    const result = JSON.parse(json);
    return result ?? null;
  }

  free(): void {
    this.session.free();
  }
}

// ── Story runner ────────────────────────────────────────────────

export class StoryRunnerHandle {
  private runner: StoryRunner;

  constructor(storyBytes: Uint8Array) {
    this.runner = new StoryRunner(storyBytes);
  }

  continueStory(): StepResult {
    const json = this.runner.continue_story();
    return JSON.parse(json) as StepResult;
  }

  choose(index: number): void {
    this.runner.choose(index);
  }

  reset(): void {
    this.runner.reset();
  }

  free(): void {
    this.runner.free();
  }
}
