/**
 * @brink-lang/web — the brink compiler, IDE session, and story runtime
 * compiled to WebAssembly, behind ergonomic TypeScript wrappers.
 *
 * Wraps the raw wasm module classes (brink-web, built with wasm-pack
 * `--target web`) in wrappers that parse JSON responses into the typed
 * interfaces re-exported below. Call {@link initWasm} once before using
 * anything else.
 */

import init, {
  compile as wasmCompile,
  program_checksum as wasmProgramChecksum,
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
  StoryGraph,
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
  ValueItem,
} from "@brink/wasm-types";

// Public surface: every interface the wasm boundary speaks is available
// from this package alone (the private @brink/wasm-types workspace package
// is rolled into the published declarations).
export type * from "@brink/wasm-types";

// ── Wasm initialization ─────────────────────────────────────────

/**
 * Initialize the wasm module. Must complete before any other export is used.
 * Safe to call more than once.
 *
 * By default the `.wasm` binary is located relative to this module
 * (`new URL("brink_web_bg.wasm", import.meta.url)`), which bundlers like
 * Vite resolve and emit automatically. Pass `wasmLocation` to load it from
 * somewhere else (a CDN URL, a string path, or a precompiled
 * `WebAssembly.Module`).
 */
export async function initWasm(
  wasmLocation?: string | URL | Request | WebAssembly.Module,
): Promise<void> {
  if (wasmLocation === undefined) {
    await init();
  } else {
    await init({ module_or_path: wasmLocation });
  }
}

// ── Compilation ─────────────────────────────────────────────────

export function compile(source: string): CompileResult {
  const json = wasmCompile(source);
  return JSON.parse(json) as CompileResult;
}

/**
 * The source-identity checksum of compiled `.inkb` bytes — identical to
 * `ProgramModel.checksum` (`"0x{:08x}"`), but computed without constructing a
 * `StoryRunnerHandle`. The studio compares a running program's identity to its
 * latest compile to detect "source out of sync" (live-inspector degraded mode).
 */
export function programChecksum(storyBytes: Uint8Array): string {
  return wasmProgramChecksum(storyBytes);
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
  private mutationCount = 0;

  constructor() {
    this.session = new WasmEditorSession();
  }

  /**
   * Monotonic counter bumped by every content-mutating call (file updates,
   * document pushes, structural moves, manifest changes). Lets consumers
   * cache derived results (e.g. a project compile) and invalidate exactly
   * when the session could have changed.
   */
  get generation(): number {
    return this.mutationCount;
  }

  /** Mark the session as (potentially) changed. */
  private bump(): void {
    this.mutationCount += 1;
  }

  updateSource(source: string): void {
    this.bump();
    this.session.update_source(source);
  }

  updateFile(path: string, source: string): void {
    this.bump();
    this.session.update_file(path, source);
  }

  removeFile(path: string): void {
    this.bump();
    this.session.remove_file(path);
  }

  /**
   * Register (or replace) the host-capability manifest, then re-analyze.
   * Describes the host's external-function vocabulary for author-time
   * validation and richer hover/completion. Throws on an invalid manifest.
   */
  setHostManifest(manifest: HostManifest): void {
    this.bump();
    this.session.set_host_manifest(JSON.stringify(manifest));
  }

  /** Clear any registered host manifest, then re-analyze. */
  clearHostManifest(): void {
    this.bump();
    this.session.clear_host_manifest();
  }

  /**
   * Push the host's current values for `host`-source semantic types (#174) —
   * a full snapshot keyed by semantic-type name that **replaces** the cache.
   * The attached host (e.g. RPG Maker MZ) calls this with its named switches /
   * items / … so the argument picker + value-label inlay hints stay current.
   * Tooling-only — no re-analyze.
   */
  setHostValues(values: Record<string, ValueItem[]>): void {
    this.bump();
    this.session.set_host_values(JSON.stringify(values));
  }

  /** Clear the host-pushed value cache (e.g. on host disconnect). */
  clearHostValues(): void {
    this.bump();
    this.session.clear_host_values();
  }

  /**
   * Set the severity of manifest-driven external diagnostics: `"error"`
   * (default — a registered manifest is binding) or `"off"`.
   */
  setExternalCheck(level: "error" | "off"): void {
    this.bump();
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
    this.bump();
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

  /**
   * Whole-project story graph (studio-shell spec §4.1): knot/stitch nodes
   * plus END/DONE pseudo-nodes, and divert/choice/tunnel/thread edges.
   * Deterministically ordered; recomputed per call (call after a compile,
   * like the outline). Returns null when no analysis is available.
   */
  getStoryGraph(): StoryGraph | null {
    const json = this.session.story_graph();
    const result = JSON.parse(json);
    return (result as StoryGraph | null) ?? null;
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
    this.bump();
    const json = this.session.reorder_stitch(path, knot, stitch, direction);
    return JSON.parse(json) as MoveResult;
  }

  /** Reorder a knot within the top-level knot list. direction: 1 = down, -1 = up. */
  reorderKnot(path: string, knot: string, direction: number): MoveResult {
    this.bump();
    const json = this.session.reorder_knot(path, knot, direction);
    return JSON.parse(json) as MoveResult;
  }

  /**
   * Reorder all stitches in a knot to match `order` (a permutation of the
   * knot's stitch names). Used by drag-and-drop, which knows the full
   * destination order, and by multi-select moves.
   */
  reorderStitches(path: string, knot: string, order: string[]): MoveResult {
    this.bump();
    const json = this.session.reorder_stitches(path, knot, order);
    return JSON.parse(json) as MoveResult;
  }

  /** Reorder all top-level knots to match `order` (a permutation of the knot names). */
  reorderKnots(path: string, order: string[]): MoveResult {
    this.bump();
    const json = this.session.reorder_knots(path, order);
    return JSON.parse(json) as MoveResult;
  }

  /** Move a stitch from one knot to another. */
  moveStitch(path: string, srcKnot: string, stitch: string, destKnot: string): MoveResult {
    this.bump();
    const json = this.session.move_stitch(path, srcKnot, stitch, destKnot);
    return JSON.parse(json) as MoveResult;
  }

  /** Promote a stitch to a top-level knot. */
  promoteStitch(path: string, knot: string, stitch: string): MoveResult {
    this.bump();
    const json = this.session.promote_stitch(path, knot, stitch);
    return JSON.parse(json) as MoveResult;
  }

  /** Demote a top-level knot to a stitch inside another knot. */
  demoteKnot(path: string, knot: string, destKnot: string): MoveResult {
    this.bump();
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

  /** Move the play head to a knot/stitch path (`"knot"` / `"knot.stitch"`) —
   * ink's `ChoosePathString` equivalent; subsequent `continue*` runs from
   * there. The session keeps its state: variables and visit counts survive,
   * and the jump itself counts as a visit (like a `-> path` divert). Pending
   * choices are abandoned; the transcript so far is kept. Throws on an
   * unknown path, or if the story is parked on an unresolved async external
   * (resolve it — or `reset()` — first).
   *
   * Pass `args` to enter a **parameterized** knot (`=== call(action, present)
   * ===`) with its declared parameters bound from the supplied values. Throws
   * if the argument count doesn't match the knot's declared parameters. */
  goToPath(path: string, ...args: ExternalValue[]): void {
    if (args.length === 0) {
      this.runner.go_to_path(path);
    } else {
      this.runner.go_to_path_with_args(path, args);
    }
  }

  /** Convenience alias for entering a parameterized knot by name with bound
   * arguments — `runKnot("call", "wave", true)` ≡ `goToPath("call", "wave",
   * true)`. */
  runKnot(name: string, ...args: ExternalValue[]): void {
    this.goToPath(name, ...args);
  }

  reset(): void {
    this.runner.reset();
  }

  /** Hot-reload a freshly compiled program **in place**, preserving the
   * session's external bindings, RNG seed, and replay recording, then reset
   * the play head to the start. Follow with `beginReplay()`, a silent re-walk
   * of the saved choice log, and `endReplay()` to restore position with
   * faithful externals (query-gated branches reproduce, effects don't
   * re-fire). Throws on decode/link failure — the old program keeps running. */
  reload(storyBytes: Uint8Array): void {
    this.runner.reload(storyBytes);
  }

  /** Enter replay mode and reset the replay cursor: visible playback
   * (`continueStory`/`continueSingle`/`advanceOne`) serves externals from the
   * recording and re-runs nothing. Bracket the post-`reload` choice re-walk
   * with this and `endReplay()`. */
  beginReplay(): void {
    this.runner.begin_replay();
  }

  /** Leave replay mode: visible playback resumes invoking bindings and
   * recording their results (appending to the existing log). */
  endReplay(): void {
    this.runner.end_replay();
  }

  /** Whether any external has been recorded this session — i.e. whether a
   * post-`reload` re-walk should `beginReplay()` (serve recorded externals)
   * or run live (a fresh load has nothing recorded yet). */
  hasRecording(): boolean {
    return this.runner.has_recording();
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

  // ── Shared flows (#200) ──────────────────────────────────────────
  // Concurrent flows of one story that SHARE this runner's globals / visit
  // counts / rng (true ink flow semantics), each with its own call stack.
  // Drives the studio's "+ new flow". Distinct from a separate
  // `StoryRunnerHandle`, which is an isolated playthrough.

  /** Spawn a shared-context flow at the program root (or `path`). */
  spawnFlow(name: string, path?: string): void {
    this.runner.spawn_flow(name, path);
  }

  /** Advance a shared flow by one line. */
  continueFlow(name: string): Line {
    return JSON.parse(this.runner.continue_flow(name)) as Line;
  }

  /** Select a choice in a shared flow. */
  chooseFlow(name: string, index: number): void {
    this.runner.choose_flow(name, index);
  }

  /** Destroy a shared flow. */
  destroyFlow(name: string): void {
    this.runner.destroy_flow(name);
  }

  /** Active flow names (sorted). */
  flowNames(): string[] {
    return JSON.parse(this.runner.flow_names()) as string[];
  }

  /** Per-flow debug snapshot (State View) for a named flow. */
  flowDebugSnapshot(name: string): DebugState {
    return JSON.parse(this.runner.flow_debug_snapshot(name)) as DebugState;
  }

  free(): void {
    this.runner.free();
  }
}
