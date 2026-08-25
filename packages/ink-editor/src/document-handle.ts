/**
 * Per-view wasm document handle (issue #122 / #90).
 *
 * Every mounted editor view owns one `DocHandle` — a (session, DocumentId)
 * pair opened via `open_document` / `open_fragment` — and all wasm-backed CM6
 * extensions query through it instead of the old active-file singleton. The
 * handle is injected into the editor state via `documentHandleFacet`, so IDE
 * intelligence works in every group simultaneously with no `set_active_file`
 * choreography and no module-global session ref.
 */

import { Annotation, Facet } from "@codemirror/state";
import type { EditorSessionHandle, SegmentManifest } from "@brink-lang/web";
import type {
  AutoImportResult,
  CodeAction,
  CodeActionData,
  CompletionItem,
  ConvertTarget,
  DialogueDialect,
  DocumentChangeSpec,
  DocumentId,
  FoldRange,
  HirProjection,
  HoverInfo,
  InlayHint,
  CallWidgetSite,
  LineContext,
  Location,
  SemanticToken,
  SignatureInfo,
  StructuralResult,
  TextEdit,
} from "@brink/wasm-types";

/**
 * Marks transactions applied by the cross-view mirror (DocumentSessions).
 * The official CM6 sync-dispatch pattern: mirrored changes carry this
 * annotation so the receiving view's own mirror listener does not echo them
 * back (and auto-pin ignores them — a mirrored edit is not a user edit in
 * that view).
 */
export const syncAnnotation = Annotation.define<boolean>();

export class DocHandle {
  private pendingSpec: DocumentChangeSpec | null = null;
  private closed = false;
  /** TS-side tracking of a fragment handle's file range (UTF-16). */
  private range: { start: number; end: number } | null = null;
  /** The source string this handle last pushed — the cheap no-op guard (#14),
   *  so a redundant push costs no wasm round-trip. A handle is opened fresh
   *  (and re-created on fragment reopen / file invalidation), so this never
   *  goes stale relative to the wasm doc it addresses. */
  private lastPushed: string | null = null;

  constructor(
    private readonly session: EditorSessionHandle,
    /** The wasm document id. */
    readonly id: DocumentId,
    /** File path this handle addresses. */
    readonly path: string,
    /** True for fragment (symbol) handles. */
    readonly isFragment: boolean,
  ) {}

  /** Current wasm-side text for this handle's view (fragment or full file). */
  viewSource(): string | null {
    return this.session.getViewSourceDoc(this.id);
  }

  /**
   * Push the view's text to the wasm session if it changed. The returned
   * change spec (what actually changed, in UTF-16 *file* coordinates) is
   * kept until `takePendingChangeSpec` so the mirror — which runs after the
   * transaction that triggered the push — can forward it to sibling views
   * of the same file. Redundant pushes (mirrored content, repeat queries)
   * compare equal and are free of reanalysis.
   */
  pushSource(source: string): void {
    if (this.closed) return;
    // Cheap no-op guard (#14): compare against the last source WE pushed, not a
    // full-source round-trip out of wasm (`getViewSourceDoc`). Redundant pushes
    // — the same source re-queried by several extensions in one keystroke, or
    // mirrored content — short-circuit here for free.
    if (this.lastPushed === source) return;
    const spec = this.session.updateDocument(this.id, source);
    // `spec === null` means either an unknown handle OR a refused write
    // (issue #2306: this handle's file currently resolves to a mounted
    // stdlib copy). Only on a genuinely applied push do we cache
    // `lastPushed` and rebase the TS-side range — otherwise the wasm-side
    // ViewContext never moved, so caching here would desync a later
    // hover/completion/semanticTokens query against a stale offset, and
    // would suppress a legitimate later push of byte-identical text once
    // the file is shadowed (un-mounted) by a real project file.
    if (spec === null) return;
    this.lastPushed = source;
    this.manifestStale = true;
    this.pendingSpec = spec;
    if (this.range !== null) {
      // The wasm side rebased this handle's view range during the splice;
      // mirror that here (the view always spans exactly the pushed text).
      this.range = { start: this.range.start, end: this.range.start + source.length };
    }
  }

  /**
   * Apply a bounded edit list instead of pushing the whole document
   * (#3064 C1). Single-edit batches only — a multi-cursor transaction
   * falls back to {@link pushSource} (returns false) so the cross-view
   * mirror's change spec stays a single range. On success the spec is
   * synthesized locally (the wasm side no longer computes one) and the
   * full-text no-op guard is invalidated.
   */
  applyChanges(edits: readonly { from: number; to: number; insert: string }[]): boolean {
    if (this.closed || this.isFragment) return false;
    if (edits.length !== 1) return false;
    const applier = this.session.applyEditsDocument?.bind(this.session);
    if (!applier) return false;
    if (!applier(this.id, edits)) return false;
    this.lastPushed = null;
    this.manifestStale = true;
    const e = edits[0];
    this.pendingSpec = { path: this.path, start: e.from, end: e.to, text: e.insert };
    return true;
  }

  /** Record the fragment range this handle was opened with. */
  setFragmentRange(start: number, end: number): void {
    this.range = { start, end };
  }

  /** The fragment handle's current file range (null for file handles). */
  fragmentRange(): { start: number; end: number } | null {
    return this.range;
  }

  /** Consume the change spec recorded by the last effective push. */
  takePendingChangeSpec(): DocumentChangeSpec | null {
    const spec = this.pendingSpec;
    this.pendingSpec = null;
    return spec;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.session.closeDocument(this.id);
  }

  // ── Queries (offsets are UTF-16, view-relative — like the view's doc) ──

  /** Per-segment slice cache, keyed by manifest version key (#3064
   *  option A). Keys are self-invalidating — a segment's key changes
   *  exactly when its content does and survives shift edits — so the
   *  only maintenance is pruning keys that leave the manifest. */
  private segSlices = new Map<string, { contexts?: LineContext[]; tokens?: SemanticToken[] }>();
  /** Config epoch the slice cache was filled under — a dialect or host-
   *  manifest swap changes slice CONTENT without changing identity keys,
   *  so the cache must clear when the epoch moves. */
  private sliceEpoch = -1;
  /** One manifest fetch per document version (#3064 micro B): the
   *  manifest changes only when the document (or config) does, and both
   *  invalidation points run through this handle. */
  private manifestCache: SegmentManifest | null = null;
  private manifestStale = true;

  private segmentManifest(): SegmentManifest | null {
    if (this.isFragment) return null;
    const epoch = this.session.configEpoch?.() ?? 0;
    if (epoch !== this.sliceEpoch) {
      this.sliceEpoch = epoch;
      this.segSlices.clear();
      this.manifestStale = true;
    }
    if (this.manifestStale) {
      this.manifestCache = this.session.getSegmentManifestDoc?.(this.id) ?? null;
      this.manifestStale = false;
    }
    return this.manifestCache;
  }

  /**
   * The per-segment context slices with their version keys (#3064 micro
   * C): lets a consumer cache DERIVED per-line values (the element-type
   * `LineInfo`s) under the same self-invalidating keys instead of
   * rebuilding them for every line of the document on each keystroke.
   * `null` → no delta protocol (fragment view, native file, mock) — use
   * {@link lineContexts} and derive whole-doc.
   */
  lineContextSlices(): { key: string; ownedFrom: number; contexts: LineContext[] }[] | null {
    const manifest = this.segmentManifest();
    if (manifest === null) return null;
    const out: { key: string; ownedFrom: number; contexts: LineContext[] }[] = [];
    const live = new Set<string>();
    for (const seg of manifest.segments) {
      live.add(seg.key);
      let entry = this.segSlices.get(seg.key);
      if (!entry?.contexts) {
        const slice = this.session.getSegmentLineContextsDoc?.(this.id, seg.key) ?? null;
        if (slice === null) return null;
        entry = { ...entry, contexts: slice };
        this.segSlices.set(seg.key, entry);
      }
      out.push({ key: seg.key, ownedFrom: seg.ownedFrom, contexts: entry.contexts });
    }
    this.pruneSlices(live);
    return out;
  }

  lineContexts(): LineContext[] {
    const manifest = this.segmentManifest();
    if (manifest === null) return this.session.getLineContextsDoc(this.id);
    const out: LineContext[] = [];
    const live = new Set<string>();
    for (const seg of manifest.segments) {
      live.add(seg.key);
      let entry = this.segSlices.get(seg.key);
      if (!entry?.contexts) {
        const slice = this.session.getSegmentLineContextsDoc?.(this.id, seg.key) ?? null;
        // A stale key (manifest raced an edit) — fall back wholesale.
        if (slice === null) return this.session.getLineContextsDoc(this.id);
        entry = { ...entry, contexts: slice };
        this.segSlices.set(seg.key, entry);
      }
      for (const c of entry.contexts) out.push(c);
    }
    this.pruneSlices(live);
    return out;
  }

  /**
   * Whole-document semantic tokens. `fast: true` (the keystroke path)
   * serves any UNCACHED segment from the classifier-only slice — no
   * analysis pull, tokens marked unrefined so a later refined fetch
   * replaces them; `fast: false` (initial build, deferred refresh)
   * fetches refined slices and caches them as final.
   */
  semanticTokens(fast = false): SemanticToken[] {
    const manifest = this.segmentManifest();
    if (manifest === null) return this.session.getSemanticTokensDoc(this.id);
    const out: SemanticToken[] = [];
    const live = new Set<string>();
    for (const seg of manifest.segments) {
      live.add(seg.key);
      let entry = this.segSlices.get(seg.key);
      if (!entry?.tokens) {
        const refined = fast
          ? null
          : (this.session.getSegmentSemanticTokensDoc?.(this.id, seg.key) ?? null);
        if (refined !== null) {
          entry = { ...entry, tokens: refined };
          this.segSlices.set(seg.key, entry);
        } else if (fast) {
          const quick = this.session.getSegmentSemanticTokensFastDoc?.(this.id, seg.key) ?? null;
          if (quick === null) return this.session.getSemanticTokensDoc(this.id);
          // Deliberately NOT cached as final: the next non-fast call
          // (the deferred refresh) fetches and caches the refined slice.
          for (const t of quick) out.push({ ...t, line: t.line + seg.ownedFrom });
          continue;
        } else {
          return this.session.getSemanticTokensDoc(this.id);
        }
      }
      // Cached token lines are segment-relative; rebase by the CURRENT
      // manifest position (this is what makes shift edits free).
      for (const t of entry.tokens) out.push({ ...t, line: t.line + seg.ownedFrom });
    }
    this.pruneSlices(live);
    return out;
  }

  private pruneSlices(live: Set<string>): void {
    for (const key of this.segSlices.keys()) {
      if (!live.has(key)) this.segSlices.delete(key);
    }
  }

  /** The HIR structural projection (#454): spans + per-line container stack. */
  hirProjection(): HirProjection {
    return this.session.getHirSpansDoc(this.id);
  }

  completions(offset: number): CompletionItem[] {
    return this.session.getCompletionsDoc(this.id, offset);
  }

  /**
   * Auto-import (#312 F): ensure the file backing this handle `INCLUDE`s
   * `target`. Returns whether it was already reachable and, when not, the
   * whole-file UTF-16 `INCLUDE`-insertion edit. Idempotent.
   */
  autoImport(target: string): AutoImportResult {
    return this.session.autoImportIncludeDoc(this.id, target);
  }

  /**
   * Auto-import (#312 F, fragment-view path): ensure the file backing this
   * fragment handle `INCLUDE`s `target`, **applying the INCLUDE out-of-band**
   * (it lives above the fragment and cannot be dispatched into this view) and
   * rebasing the wasm-side view range. This handle's TS-side fragment range is
   * rebased here to match, so a subsequent {@link pushSource} splices at the
   * correct (post-shift) window. Returns `{ edit: null }` on success — the
   * INCLUDE is already applied, so the caller only inserts the symbol text.
   * Idempotent — already-reachable ⇒ no INCLUDE, no rebase.
   */
  autoImportApply(target: string): AutoImportResult {
    const result = this.session.autoImportApplyIncludeDoc(this.id, target);
    // Rebase this fragment's TS-side range by the applied INCLUDE's UTF-16
    // delta so it stays consistent with the wasm view (which was rebased in
    // the same op). The returned `edit` describes the applied shift; it must
    // NOT be re-applied to the CM view.
    if (result.ok && !result.already_reachable && result.edit && this.range !== null) {
      const edit = result.edit;
      const delta = edit.insert.length - (edit.to - edit.from);
      // Only shift when the edit landed at/before the fragment start (the
      // INCLUDE block sits above the fragment — the normal case).
      if (edit.from <= this.range.start) {
        this.range = {
          start: this.range.start + delta,
          end: this.range.end + delta,
        };
      }
    }
    // The INCLUDE is applied; the caller must not dispatch an edit into the
    // fragment CM view. Strip it so `outOfScopeApply` only inserts the symbol.
    return { ...result, edit: null };
  }

  hover(offset: number): HoverInfo | null {
    return this.session.getHoverDoc(this.id, offset);
  }

  gotoDefinition(offset: number): Location | null {
    return this.session.gotoDefinitionDoc(this.id, offset);
  }

  findReferences(offset: number): Location[] {
    return this.session.findReferencesDoc(this.id, offset);
  }

  prepareRename(offset: number): Location | null {
    return this.session.prepareRenameDoc(this.id, offset);
  }

  /**
   * Compute the safe-rename result for renaming the symbol at `offset` (a
   * whole-file UTF-16 offset) to `newName`. Side-effect-free — the wasm side
   * computes the new sources + breakage report without applying anything, so
   * the inline-rename badge can query it live on each keystroke (#324).
   */
  renameSymbolAt(offset: number, newName: string): StructuralResult {
    return this.session.renameSymbolAt(this.path, offset, newName);
  }

  codeActions(offset: number): CodeAction[] {
    return this.session.getCodeActionsDoc(this.id, offset);
  }

  /**
   * Resolve a (non-extract) code action chosen from the menu (#321): compute
   * the action's `StructuralResult` from its opaque `data` payload. `offset` is
   * a whole-file UTF-16 offset (the doc-handle variant folds fragment origin).
   * Side-effect-free — the caller applies the returned edits through the host
   * apply seam.
   */
  resolveCodeAction(data: CodeActionData, offset: number): StructuralResult {
    return this.session.resolveCodeActionDoc(this.id, data, offset);
  }

  /**
   * Extract the selected lines into a new `=== name ===` knot, replacing the
   * selection with the tunnel call `-> name ->` (#315 H). `start`/`end` are
   * whole-file UTF-16 offsets (fold any fragment-view origin first). Returns a
   * safe-by-default `StructuralResult`; the caller applies via the apply seam.
   */
  extractToKnot(start: number, end: number, name: string): StructuralResult {
    return this.session.extractToKnot(this.path, start, end, name);
  }

  /**
   * Extract the selected lines into a new `=== function name() ===`, replacing
   * the selection with `{name()}` / `~ name()` (#315 H). Same offset/gate
   * semantics as {@link extractToKnot}.
   */
  extractToFunction(start: number, end: number, name: string): StructuralResult {
    return this.session.extractToFunction(this.path, start, end, name);
  }

  inlayHints(start: number, end: number): InlayHint[] {
    return this.session.getInlayHintsDoc(this.id, start, end);
  }

  argumentWidgets(start: number, end: number): CallWidgetSite[] {
    return this.session.getArgumentWidgetsDoc(this.id, start, end);
  }

  signatureHelp(offset: number): SignatureInfo | null {
    return this.session.getSignatureHelpDoc(this.id, offset);
  }

  foldingRanges(): FoldRange[] {
    return this.session.getFoldingRangesDoc(this.id);
  }

  convertElement(offset: number, target: ConvertTarget): TextEdit | null {
    return this.session.convertElementDoc(this.id, offset, target);
  }

  /**
   * Register (or replace) the dialogue dialect (#368) on this handle's
   * shared wasm session. The session — not the handle — owns the
   * registration (mirrors `setHostManifest`): every document handle sharing
   * this session sees the same dialect facet on its next `lineContexts()`
   * call. Throws on an invalid dialect.
   */
  setDialect(dialect: DialogueDialect): void {
    this.session.setDialect(dialect);
  }

  /** Clear the registered dialect — `lineContexts()` reverts to plain
   *  structural classification. */
  clearDialect(): void {
    this.session.clearDialect();
  }

  /**
   * Enable or disable machinery/narrative fold runs (#479 — off by default)
   * on this handle's shared wasm session. Hosts implementing prose/logic
   * view modes turn this on alongside `setActiveFoldKinds`; without it,
   * `foldingRanges()` returns structural folds only and the run computation
   * is skipped entirely.
   */
  setFoldRunsEnabled(enabled: boolean): void {
    this.session.setFoldRunsEnabled(enabled);
  }
}

/**
 * Indirection between editor states and the live wasm handle: a slot's
 * `handle` is swapped on mount/unmount (handles are opened per mount, and a
 * backgrounded tab's cached EditorState must not pin a closed handle).
 * Closures and facet readers always go through the slot.
 */
export interface DocumentHandleSlot {
  readonly handle: DocHandle | null;
}

/**
 * The view's document-handle slot, provided once per editor state by
 * `brinkStudio({ handleSlot })`. Extensions and keybindings read it with
 * `state.facet(documentHandleFacet)?.handle`; null in handle-less states
 * (tests, plain editors).
 */
export const documentHandleFacet = Facet.define<
  DocumentHandleSlot | null,
  DocumentHandleSlot | null
>({
  combine: (values) => values.find((v) => v !== null) ?? null,
});
