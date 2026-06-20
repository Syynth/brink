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
import type { EditorSessionHandle } from "@brink-lang/web";
import type {
  CodeAction,
  CompletionItem,
  ConvertTarget,
  DocumentChangeSpec,
  DocumentId,
  FoldRange,
  HoverInfo,
  InlayHint,
  CallWidgetSite,
  LineContext,
  Location,
  SemanticToken,
  SignatureInfo,
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
    this.lastPushed = source;
    const spec = this.session.updateDocument(this.id, source);
    if (spec !== null) this.pendingSpec = spec;
    if (this.range !== null) {
      // The wasm side rebased this handle's view range during the splice;
      // mirror that here (the view always spans exactly the pushed text).
      this.range = { start: this.range.start, end: this.range.start + source.length };
    }
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

  lineContexts(): LineContext[] {
    return this.session.getLineContextsDoc(this.id);
  }

  semanticTokens(): SemanticToken[] {
    return this.session.getSemanticTokensDoc(this.id);
  }

  completions(offset: number): CompletionItem[] {
    return this.session.getCompletionsDoc(this.id, offset);
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

  codeActions(offset: number): CodeAction[] {
    return this.session.getCodeActionsDoc(this.id, offset);
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
