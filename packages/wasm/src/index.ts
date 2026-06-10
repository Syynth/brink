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
  DocumentId,
  DocumentChangeSpec,
  Line,
  MoveResult,
  DebugState,
  ProgramModel,
  SaveState,
  LoadReport,
  HostManifest,
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

  /**
   * Register (or replace) the host-capability manifest, then re-analyze.
   * Describes the host's external-function vocabulary for author-time
   * validation and richer hover/completion. Throws on an invalid manifest.
   */
  setHostManifest(manifest: HostManifest): void {
    this.session.set_host_manifest(JSON.stringify(manifest));
  }

  /** Clear any registered host manifest, then re-analyze. */
  clearHostManifest(): void {
    this.session.clear_host_manifest();
  }

  /**
   * Set the severity of manifest-driven external diagnostics: `"error"`
   * (default — a registered manifest is binding) or `"off"`.
   */
  setExternalCheck(level: "error" | "off"): void {
    this.session.set_external_check(level);
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

  // ── Document handles (multi-document API) ─────────────────────
  //
  // Each handle pairs a file path with an optional fragment view, so N
  // live editor views can issue IDE queries independently of the legacy
  // active-file/view-context singleton above. Offsets are UTF-16 and
  // view-relative per handle, like the singleton queries.

  /** Open a full-file document handle. Returns null if the file is not loaded. */
  openDocument(path: string): DocumentId | null {
    const id = this.session.open_document(path);
    return id === 0 ? null : id;
  }

  /**
   * Open a fragment document handle scoping `path` to `[start, end)` (UTF-16
   * offsets, like setViewContext). Returns null if the file is not loaded.
   */
  openFragment(path: string, start: number, end: number): DocumentId | null {
    const id = this.session.open_fragment(path, start, end);
    return id === 0 ? null : id;
  }

  /** Close a document handle. Returns false if the handle was unknown. */
  closeDocument(doc: DocumentId): boolean {
    return this.session.close_document(doc);
  }

  /**
   * Replace a document's content: full-file replace for file handles,
   * fragment splice for fragment handles. Returns a change spec describing
   * what actually changed in the file (UTF-16 file coordinates) for
   * live-mirroring sibling views, or null for an unknown handle.
   */
  updateDocument(doc: DocumentId, source: string): DocumentChangeSpec | null {
    const json = this.session.update_document(doc, source);
    const result = JSON.parse(json);
    return result ?? null;
  }

  /** Get the source text for a handle's view (fragment or full file). */
  getViewSourceDoc(doc: DocumentId): string | null {
    const json = this.session.get_view_source_doc(doc);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getLineContextsDoc(doc: DocumentId): LineContext[] {
    const json = this.session.line_contexts_doc(doc);
    return JSON.parse(json) as LineContext[];
  }

  getSemanticTokensDoc(doc: DocumentId): SemanticToken[] {
    const json = this.session.semantic_tokens_doc(doc);
    return JSON.parse(json) as SemanticToken[];
  }

  getCompletionsDoc(doc: DocumentId, offset: number): CompletionItem[] {
    const json = this.session.completions_doc(doc, offset);
    return JSON.parse(json) as CompletionItem[];
  }

  getHoverDoc(doc: DocumentId, offset: number): HoverInfo | null {
    const json = this.session.hover_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  gotoDefinitionDoc(doc: DocumentId, offset: number): Location | null {
    const json = this.session.goto_definition_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  findReferencesDoc(doc: DocumentId, offset: number): Location[] {
    const json = this.session.find_references_doc(doc, offset);
    return JSON.parse(json) as Location[];
  }

  prepareRenameDoc(doc: DocumentId, offset: number): Location | null {
    const json = this.session.prepare_rename_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  doRenameDoc(doc: DocumentId, offset: number, newName: string): FileEdit[] {
    const json = this.session.rename_doc(doc, offset, newName);
    return JSON.parse(json) as FileEdit[];
  }

  getCodeActionsDoc(doc: DocumentId, offset: number): CodeAction[] {
    const json = this.session.code_actions_doc(doc, offset);
    return JSON.parse(json) as CodeAction[];
  }

  getInlayHintsDoc(doc: DocumentId, start: number, end: number): InlayHint[] {
    const json = this.session.inlay_hints_doc(doc, start, end);
    return JSON.parse(json) as InlayHint[];
  }

  getSignatureHelpDoc(doc: DocumentId, offset: number): SignatureInfo | null {
    const json = this.session.signature_help_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getFoldingRangesDoc(doc: DocumentId): FoldRange[] {
    const json = this.session.folding_ranges_doc(doc);
    return JSON.parse(json) as FoldRange[];
  }

  getDocumentSymbolsDoc(doc: DocumentId): DocumentSymbol[] {
    const json = this.session.document_symbols_doc(doc);
    return JSON.parse(json) as DocumentSymbol[];
  }

  convertElementDoc(doc: DocumentId, offset: number, target: ConvertTarget): TextEdit | null {
    const json = this.session.convert_element_doc(doc, offset, target);
    const result = JSON.parse(json);
    return result ?? null;
  }

  formatDocumentDoc(doc: DocumentId): string {
    const json = this.session.format_document_doc(doc);
    return JSON.parse(json) as string;
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

  /** Reorder a stitch within its knot. direction: 1 = down, -1 = up. */
  reorderStitch(path: string, knot: string, stitch: string, direction: number): MoveResult {
    const json = this.session.reorder_stitch(path, knot, stitch, direction);
    return JSON.parse(json) as MoveResult;
  }

  /** Reorder a knot within the top-level knot list. direction: 1 = down, -1 = up. */
  reorderKnot(path: string, knot: string, direction: number): MoveResult {
    const json = this.session.reorder_knot(path, knot, direction);
    return JSON.parse(json) as MoveResult;
  }

  /**
   * Reorder all stitches in a knot to match `order` (a permutation of the
   * knot's stitch names). Used by drag-and-drop, which knows the full
   * destination order, and by multi-select moves.
   */
  reorderStitches(path: string, knot: string, order: string[]): MoveResult {
    const json = this.session.reorder_stitches(path, knot, order);
    return JSON.parse(json) as MoveResult;
  }

  /** Reorder all top-level knots to match `order` (a permutation of the knot names). */
  reorderKnots(path: string, order: string[]): MoveResult {
    const json = this.session.reorder_knots(path, order);
    return JSON.parse(json) as MoveResult;
  }

  /** Move a stitch from one knot to another. */
  moveStitch(path: string, srcKnot: string, stitch: string, destKnot: string): MoveResult {
    const json = this.session.move_stitch(path, srcKnot, stitch, destKnot);
    return JSON.parse(json) as MoveResult;
  }

  /** Promote a stitch to a top-level knot. */
  promoteStitch(path: string, knot: string, stitch: string): MoveResult {
    const json = this.session.promote_stitch(path, knot, stitch);
    return JSON.parse(json) as MoveResult;
  }

  /** Demote a top-level knot to a stitch inside another knot. */
  demoteKnot(path: string, knot: string, destKnot: string): MoveResult {
    const json = this.session.demote_knot(path, knot, destKnot);
    return JSON.parse(json) as MoveResult;
  }

  free(): void {
    this.session.free();
  }
}

// ── Story runner ────────────────────────────────────────────────

/** A value that can cross the ink↔JS external-binding boundary. */
export type ExternalValue = number | boolean | string | null;

/** An external-function binding: receives the call arguments as native JS
 * values and returns a value (or nothing) back to the story. May be async —
 * return a Promise and the story suspends until it resolves (drive with
 * `continueAsync`/`continueSingleAsync`). */
export type ExternalFn = (
  ...args: ExternalValue[]
) => ExternalValue | void | Promise<ExternalValue | void>;

export class StoryRunnerHandle {
  private runner: StoryRunner;

  constructor(storyBytes: Uint8Array) {
    this.runner = new StoryRunner(storyBytes);
  }

  /** Bind an ink `EXTERNAL <name>(...)` to a synchronous JS callback.
   * Re-binding the same name replaces the previous callback. */
  bindExternal(name: string, fn: ExternalFn): void {
    this.runner.bind_external(name, fn);
  }

  /** Remove a previously registered external binding. */
  unbindExternal(name: string): void {
    this.runner.unbind_external(name);
  }

  /** When `true`, an unbound external resolves to `null` instead of falling
   * through to its ink fallback body / erroring. Default `false`. */
  setLenientUnbound(lenient: boolean): void {
    this.runner.set_lenient_unbound(lenient);
  }

  /** Read a global ink variable by name. `undefined` if no such variable is
   * declared, `null` if it exists and holds null. */
  getVar(name: string): ExternalValue | undefined {
    return this.runner.get_var(name) as ExternalValue | undefined;
  }

  /** Set a global ink variable by name. Returns `false` if no such variable
   * is declared. */
  setVar(name: string, value: ExternalValue): boolean {
    return this.runner.set_var(name, value);
  }

  /** Set the RNG seed for reproducible `RANDOM`/shuffle output. Applies now
   * and is re-applied across `reset()`. Set before the first continue for a
   * fully deterministic playthrough. */
  setSeed(seed: number): void {
    this.runner.set_seed(seed);
  }

  /** Capture durable game state as a typed object (dev/inspectable). */
  save(): SaveState {
    return JSON.parse(this.runner.save()) as SaveState;
  }

  /** Capture durable game state as a compact MessagePack blob (release). */
  saveBytes(): Uint8Array {
    return this.runner.save_bytes();
  }

  /** Reconcile a saved state into the running story; returns what couldn't be
   * applied (empty `unknown_globals` = clean). Tolerant of story patches. */
  load(state: SaveState): LoadReport {
    return JSON.parse(this.runner.load(JSON.stringify(state))) as LoadReport;
  }

  /** Reconcile a MessagePack blob from `saveBytes()`. */
  loadBytes(bytes: Uint8Array): LoadReport {
    return JSON.parse(this.runner.load_bytes(bytes)) as LoadReport;
  }

  /** Evaluate an ink function from the host (engine→ink), out-of-band: the
   * visible story is untouched. Externals it calls resolve through registered
   * synchronous bindings. Returns the function's value. */
  callFunction(name: string, ...args: ExternalValue[]): ExternalValue {
    return this.runner.call_function(name, args) as ExternalValue;
  }

  continueStory(): Line[] {
    const json = this.runner.continue_story();
    return JSON.parse(json) as Line[];
  }

  continueSingle(): Line {
    const json = this.runner.continue_single();
    return JSON.parse(json) as Line;
  }

  /** Continue maximally, awaiting any async (Promise-returning) bindings. Use
   * this instead of `continueStory` when bindings may be async. */
  async continueStoryAsync(): Promise<Line[]> {
    const lines: Line[] = [];
    for (;;) {
      const line = await this.advanceAwaiting();
      if (line.type === "text") {
        lines.push(line);
        continue;
      }
      lines.push(line); // terminal: done | choices | end
      return lines;
    }
  }

  /** Produce one line, awaiting any async binding hit along the way. */
  async continueSingleAsync(): Promise<Line> {
    return this.advanceAwaiting();
  }

  // ── Low-level async primitives (for custom drive loops) ──────────
  // `continueStoryAsync`/`continueSingleAsync` are the ergonomic path; these
  // expose the raw park/resolve so a host can drive it manually.

  /** Advance one step; the line may be `{ type: "awaiting_external" }`. */
  advanceOne(): Line {
    return JSON.parse(this.runner.advance_one()) as Line;
  }

  /** Take the suspended async binding's Promise to await; `undefined` if none. */
  takePendingPromise(): Promise<ExternalValue> | undefined {
    const p = this.runner.take_pending_promise();
    return p === undefined ? undefined : (p as Promise<ExternalValue>);
  }

  /** Resolve the parked external with a value (the awaited Promise result). */
  resolveExternal(value: ExternalValue): void {
    this.runner.resolve_external(value);
  }

  /** Step until a real line, transparently awaiting+resolving any suspended
   * async binding (a Promise returned by a `bindExternal` callback). On a
   * rejected Promise, resolves the external with `null` to unstick the flow,
   * then rethrows so the host sees the failure. */
  private async advanceAwaiting(): Promise<Line> {
    for (;;) {
      const line = JSON.parse(this.runner.advance_one()) as Line;
      if (line.type !== "awaiting_external") {
        return line;
      }
      const promise = this.runner.take_pending_promise() as Promise<ExternalValue>;
      let value: ExternalValue;
      try {
        value = await promise;
      } catch (err) {
        this.runner.resolve_external(null); // unstick the parked flow
        throw err;
      }
      this.runner.resolve_external(value ?? null);
    }
  }

  choose(index: number): void {
    this.runner.choose(index);
  }

  reset(): void {
    this.runner.reset();
  }

  /** Structured, name-resolved snapshot of the runtime's current state. */
  debugSnapshot(): DebugState {
    return JSON.parse(this.runner.debug_snapshot()) as DebugState;
  }

  /** The compiled program as `.inkt` text (Program Explorer raw toggle). */
  programInkt(): string {
    return this.runner.program_inkt();
  }

  /** Structured model of the compiled program (Program Explorer). */
  programModel(): ProgramModel {
    return JSON.parse(this.runner.program_model()) as ProgramModel;
  }

  free(): void {
    this.runner.free();
  }
}
