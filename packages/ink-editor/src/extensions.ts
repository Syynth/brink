import { Compartment, type Extension } from "@codemirror/state";
import type {
  CompileResult,
  SemanticToken,
  HirProjection,
  CompletionItem,
  HoverInfo,
  Location,
  InlayHint,
  CallWidgetSite,
  SignatureInfo,
  FoldRange,
  CodeAction,
  StructuralResult,
  AutoImportResult,
  DialogueDialect,
} from "@brink/wasm-types";
import {
  documentHandleFacet,
  type DocumentHandleSlot,
} from "./document-handle.js";
import { indentationMarkers } from "@replit/codemirror-indentation-markers";
import { hangingIndent } from "./hanging-indent.js";
import { EditorView } from "@codemirror/view";
import { indentUnit } from "@codemirror/language";
import { brinkTheme } from "./theme.js";
import { screenplayDecorations } from "./screenplay.js";
import { AT_CUE_DIALECT, ResolvedDialect } from "./dialect.js";
import {
  dialectFacet,
  reclassifyEffect,
  elementTypeField,
} from "./element-type.js";
import { highlightExtension } from "./highlight.js";
import { diagnosticsExtension } from "./diagnostics.js";
import { brinkKeymap } from "./keybindings.js";
import { completionsExtension } from "./completions.js";
import { hoverExtension } from "./hover.js";
import { augmentHoverWithRuntimeValue } from "./hover-runtime.js";
import { gotoDefinitionExtension } from "./goto-definition.js";
import { foldingExtension } from "./folding.js";
import { inlayHintsExtension } from "./inlay-hints.js";
import {
  argumentWidgetsExtension,
  type FormGlyphMode,
} from "./argument-widgets.js";
import { signatureHelpExtension } from "./signature-help.js";
import { referencesExtension } from "./references.js";
import { renameExtension, type BreakageContext } from "./rename.js";
import { codeActionsExtension } from "./code-actions.js";
import {
  extractActionsExtension,
  extractCodeActions,
  isExtractAction,
  startExtractPrompt,
  EXTRACT_TO_KNOT_ACTION,
  type ExtractKind,
} from "./extract-actions.js";
import { playFromHereExtension, type BreakpointGutterMarker } from "./play-from-here.js";
import {
  executionHighlightExtension,
  type ExecutionHighlight,
} from "./execution-highlight.js";
import { hostGutterExtension, type HostGutterMarker } from "./host-gutter.js";
import { hirOverlayExtension } from "./hir-overlay.js";
import { proseExtension } from "./prose.js";
import type { ProseChecker, ProseLint } from "./prose.js";
import { perfViewportProbe } from "./perf/viewport-probe.js";
import { editorActionKeymap } from "./editor-actions.js";
import { tooltipPortalExtension } from "./tooltip-portal.js";

/**
 * The indent width when the project declares none — mirrors
 * `brink_project_config::DEFAULT_INDENT` (#3149). The two must agree; a
 * disagreement here is the exact failure the ruling exists to prevent,
 * just relocated across the language boundary.
 */
export const DEFAULT_INDENT = 4;

export interface BrinkStudioOptions {
  /** Sync or async (W2a — the studio rides the async session facade);
   *  see `DiagnosticsOptions.compile` for the async landing contract. */
  compile: (source: string) => CompileResult | Promise<CompileResult>;
  getSemanticTokens: (source: string) => SemanticToken[];
  /** Classifier-only token source for the keystroke path in large
   *  documents (#3064 micro) — see `HighlightOptions.getSemanticTokensFast`. */
  getSemanticTokensFast?: (source: string) => SemanticToken[];
  /** W2b deferred-refresh warm-ups (docs/editor-worker-spec.md) — each
   *  runs its pull through the async session facade before the deferred
   *  refresh dispatches. See the per-extension option docs. */
  prepareRefined?: () => Promise<unknown> | undefined;
  prepareProjection?: () => Promise<unknown> | undefined;
  prepareHints?: (start: number, end: number) => Promise<unknown> | undefined;
  prepareWidgets?: (start: number, end: number) => Promise<unknown> | undefined;
  prepareFoldRanges?: () => Promise<unknown> | undefined;
  getTokenTypeNames: () => string[];
  onCompile?: (result: CompileResult) => void;

  /** The HIR structural projection for this document (#454). When provided,
   *  the editor renders the structural overlay: `brink-hir-*` inline marks
   *  with `data-*` identity, per-line rail attributes + the rails gutter, and
   *  identity-keyed occurrence highlighting. Omit for no overlay. */
  getHirProjection?: () => HirProjection;

  /** Prose checking (#3209): the checker the host registers, or `null` for
   *  none. Requires `getHirProjection` too — the projection is what says
   *  which spans are prose, and guessing is the failure this feature exists
   *  to avoid. Omit both for no prose checking at all, which is what a
   *  runtime-only or headless embedder should get: the engine is a separate
   *  6.5 MB wasm module, larger than the whole compiler. */
  getProseChecker?: () => ProseChecker | null;
  /** Project proper nouns for the prose dictionary — knot and cue names.
   *  Without them every invented character name reports as a misspelling. */
  getProseDictionary?: () => string[];
  /** `american` | `british` | `canadian` | `australian`. */
  getProseDialect?: () => string;
  /** Add a word to the project's own dictionary (the "Add to dictionary"
   *  quick-fix). Absent ⇒ the action is not offered at all, rather than
   *  offered and inert. */
  onAddToDictionary?: (word: string) => void;
  /** Prose findings for the host's own list (Problems panel). */
  onProseLints?: (lints: readonly ProseLint[]) => void;

  /** The editor's skin (#363 headless-ready). Defaults to `brinkTheme` (the
   *  `--bs-*`-token CM theme brink-studio uses). Pass `false` for a headless
   *  editor — no theme at all; the host styles the documented class taxonomy
   *  directly (docs/editor-consumer-guide.md). Pass your own `Extension` to
   *  substitute a different CM theme. Structural styles (popup positioning,
   *  data-driven widget colors) are independent of this and always active. */
  theme?: Extension | false;
  /**
   * Indentation guides (the vertical whitespace/tab indent lines, ruled
   * 2026-08-23 alongside "literal whitespace" — the file's real
   * indentation is now the only indentation, so the editor draws guides
   * for it). Default on; pass `false` to omit (e.g. a fully headless
   * composition that draws its own).
   */
  indentGuides?: boolean;

  /**
   * Indent width in spaces — `[project] indent` from `brink.toml`, or the
   * shared default when the project sets none (#3149, ruled 2026-08-27:
   * "everything that indents reads the same setting").
   *
   * Threaded rather than hardcoded because the formatter reads the same
   * value: an editor typing four spaces under a formatter writing two
   * looks like a rendering glitch rather than a config mismatch, and the
   * author has no way to tell which component is wrong. The guides read
   * `indentUnit` too, so they follow automatically.
   *
   * Undefined means "the project did not say", which resolves to
   * {@link DEFAULT_INDENT} — the same constant `brink-project-config`
   * declares, mirrored here because this package cannot import Rust.
   */
  indent?: number;

  /** The view's wasm document-handle slot (per-view DocId, swapped across
   *  mount/unmount). If provided, the editor uses HIR-backed line
   *  classification (`line_contexts_doc`) instead of the regex classifier,
   *  and transition actions can convert elements. */
  handleSlot?: DocumentHandleSlot;

  /**
   * The dialogue dialect (#368) driving screenplay classification,
   * decorations, transitions, and conversions. **No dialect by default**
   * (RULED 2026-08-30): absent or `null` ⇒ plain lines with the entire
   * screenplay layer torn down; a project opts in through
   * `brink.toml [dialogue]` (the studio reads the resolved artifact via
   * `ProjectSession.getConfiguredDialogueDialect()`), or an embedder passes
   * `AT_CUE_DIALECT` explicitly. `null` still tears down — classification,
   * decorations (hidden sigils, atomic ranges, the edit guard), the dialect
   * transition rows, and the dialect-specific keybinding behaviors — for
   * true headless composition (pair with `theme: false`, #363). The
   * STRUCTURAL keymap (Choice/Gather/ChoiceBody/Narrative Tab/Enter
   * transitions, Home/End, arrows, the Alt-Enter picker) stays active
   * regardless: structural rows are interpreter-owned per the dialect spec,
   * and its dialect branches self-guard on kinds that never appear when no
   * dialect is active. When a `handleSlot` is present, the dialect is also
   * pushed to the wasm session (`EditorSession.set_dialect`) so Rust-side
   * `line_contexts` classifies with it; use `setDialect(view, d)` to
   * live-reconfigure an already-mounted editor.
   */
  dialect?: DialogueDialect | null;

  // IDE features (all optional — features are enabled when provided)
  /** Sync or async (W2c) — see `CompletionsOptions.getCompletions`. */
  getCompletions?: (
    source: string,
    offset: number,
  ) => CompletionItem[] | Promise<CompletionItem[]>;
  /** Auto-import (#312 F): on accepting an out-of-scope completion, ensure the
   *  current file `INCLUDE`s the symbol's source file. Only consulted when
   *  `getCompletions` is also provided. */
  autoImport?: (target: string) => AutoImportResult;
  /** Sync or async (W2c) — see `HoverOptions.getHover`. */
  getHover?: (
    source: string,
    offset: number,
  ) => HoverInfo | null | Promise<HoverInfo | null>;
  /** Runtime-value hover (W12/#3305): a markdown note for the identifier
   *  under the cursor — its CURRENT runtime value — appended to the base
   *  hover (or shown alone when no base hover exists). The host owns the
   *  policy (globals always, frame locals while paused, degraded →
   *  null). */
  getRuntimeValueNote?: (name: string) => string | null;
  /** Sync or async (#3110). */
  gotoDefinition?: (
    source: string,
    offset: number,
  ) => Location | null | Promise<Location | null>;
  /** Called when goto-definition targets a different file. */
  onNavigateToFile?: (location: Location) => void;
  /** Returns the current active file path (for cross-file navigation detection). */
  getActiveFile?: () => string;
  /** Sync or async (#3110). */
  findReferences?: (
    source: string,
    offset: number,
  ) => Location[] | Promise<Location[]>;
  /** Sync or async (#3110). */
  prepareRename?: (
    source: string,
    offset: number,
  ) => Location | null | Promise<Location | null>;
  /** Live (debounced) safe-rename query for the inline-rename badge (#323/#324):
   *  computes the new sources + breakage report without applying anything.
   *  `offset` is in view coords; the host folds in any fragment-view origin. */
  /** Sync or async (#3110). */
  renameSymbolAt?: (
    offset: number,
    newName: string,
  ) => StructuralResult | Promise<StructuralResult>;
  /** Commit an inline rename — apply the (already-computed) edits across files.
   *  Called on a safe Enter or an explicit "Rename anyway". `currentName` is the
   *  symbol's original name (for re-keying open symbol tabs). */
  commitRename?: (
    result: StructuralResult,
    newName: string,
    currentName: string,
  ) => void;
  /** Optional host override for the inline breakage surface (#324). Return
   *  `true` to suppress the default inline report and render your own. */
  onRenameBreakage?: (
    result: StructuralResult,
    ctx: BreakageContext,
  ) => boolean;
  /** Sync or async (W2c) — see `CodeActionsOptions.getCodeActions`. */
  getCodeActions?: (
    source: string,
    offset: number,
  ) => CodeAction[] | Promise<CodeAction[]>;
  /**
   * Resolve + apply a (non-extract) code action chosen from the menu (#321
   * studio side): compute its `StructuralResult` via `resolveCodeAction` and
   * apply it through the host's apply seam (toast + Undo). Only wired alongside
   * `getCodeActions`. Absent ⇒ the menu just dismisses (the pre-#315 behavior).
   */
  applyCodeAction?: (action: CodeAction) => void;
  /**
   * Compute an extract (#315 H) `StructuralResult` for the current selection
   * (view coords) + name — side-effect-free. The host folds any fragment-view
   * origin into whole-file offsets and calls `extractToKnot` / `extractToFunction`.
   * When both this and `applyExtract` are provided, a multi-line selection
   * surfaces "Extract to knot/function" in the code-actions menu.
   */
  computeExtract?: (
    kind: ExtractKind,
    start: number,
    end: number,
    name: string,
  ) => StructuralResult | null;
  /** Apply an already-computed extract result — the host's apply seam
   *  (toast + Undo). Called on a safe Enter or an explicit "Extract anyway". */
  applyExtract?: (
    kind: ExtractKind,
    result: StructuralResult,
    name: string,
  ) => void;
  getInlayHints?: (source: string, start: number, end: number) => InlayHint[];
  getArgumentWidgets?: (
    source: string,
    start: number,
    end: number,
  ) => CallWidgetSite[];
  /** How the inline call-level argument-form glyph is shown. Default `off`. */
  argumentFormGlyph?: FormGlyphMode;
  /** Accepting a function completion inserts `()` + opens the Form. Default false. */
  argumentAutoOpen?: boolean;
  /** Sync or async (W2c) — see `SignatureHelpOptions.getSignatureHelp`. */
  getSignatureHelp?: (
    source: string,
    offset: number,
  ) => SignatureInfo | null | Promise<SignatureInfo | null>;
  getFoldingRanges?: (source: string) => FoldRange[];
  /** Breakpoint dots sharing the play gutter's column (W4/#3297, ruled
   * 2026-08-29). 1-based lines; refresh via `refreshBreakpoints(view)`. */
  getBreakpoints?: () => readonly BreakpointGutterMarker[];
  /** Gutter click on a non-header line toggles a breakpoint (1-based). */
  onToggleBreakpoint?: (line: number) => void;
  /** Doc edits moved breakpoint lines (1-based old→new pairs). */
  onBreakpointsMoved?: (moves: readonly { from: number; to: number }[]) => void;
  /** The execution highlights (W6/#3299): bands via
   * `executionHighlightExtension`, arrows via the play gutter's shared
   * column. Plural — a choice point lights several lines at once.
   * Re-read only on `refreshExecutionHighlight(view)`. */
  getExecutionHighlights?: () => readonly ExecutionHighlight[];
  /** "Reveal in Program Explorer" (W9/#3302) — line context-menu jump
   *  from a source line to its compiled instructions; 1-based. */
  onRevealInstructions?: (line: number) => void;
  /** Presence gate for the reveal item — see `PlayFromHereOptions`'s doc:
   *  the resolver is the live session's, so the host omits the item when
   *  no session can answer. Checked per menu open. */
  canRevealInstructions?: () => boolean;
  /** Start a play session entered at a knot/stitch (`onPlayFrom("knot.stitch")`).
   *  When provided, the editor shows a hover ▶ run-icon on knot/stitch
   *  declarations (#186). */
  onPlayFrom?: (inkPath: string, label?: string) => void;
  /** Right-click a knot/stitch declaration → the shared symbol context menu. */
  onSymbolContextMenu?: (
    info: { knot: string; stitch?: string },
    x: number,
    y: number,
  ) => void;
  /** Right-click anywhere that isn't a symbol header: the editor-owned text
   *  menu (docs/editor-context-menu-spec.md). When provided, the native
   *  context menu never appears inside the editor. */
  onTextContextMenu?: (
    request: import("./play-from-here.js").TextMenuRequest,
  ) => void;
  /** Host references surface: Find References (menu + Shift-Alt-F) routes
   *  its results here (the Search panel) instead of the in-view highlight.
   *  `declaration` (when goto-definition resolves one) anchors the host's
   *  references refresh (docs/search-results-cards-spec.md). */
  onShowReferences?: (
    symbol: string,
    locations: Location[],
    declaration?: Location | null,
  ) => void;
  /**
   * Host gutter-marker contribution (#343): the host's markers (breakpoints,
   * per-line annotations, run/flag icons) for the inclusive 1-based line range
   * `[fromLine, toLine]`. When provided, they render in a dedicated gutter
   * slotted after (to the right of) the built-in play-from-here gutter.
   * Recomputed on document changes; dispatch `refreshGutterMarkersEffect` (or
   * call `refreshGutterMarkers(view)`) when the marker set changes externally.
   */
  getGutterMarkers?: (
    source: string,
    fromLine: number,
    toLine: number,
  ) => HostGutterMarker[];
  /** Shared click handler for host gutter markers — fires after the marker's
   *  own `onClick`. Only consulted when `getGutterMarkers` is provided. */
  onGutterMarkerClick?: (marker: HostGutterMarker, line: number) => void;
}

// Compartments for runtime toggling
export const screenplayCompartment = new Compartment();
export const ideCompartment = new Compartment();
// `dialectFacet`'s value lives in its OWN compartment, separate from
// `screenplayCompartment` — it must be reconfigurable independent of
// whether the screenplay layer itself is present (dialect: null still needs
// `dialectFacet` to resolve to `null`, not just an absent screenplay
// bundle), and a value must be provided from EXACTLY ONE place at a time:
// mixing "provided once at mount, outside any compartment" with "provided
// again inside a compartment on reconfigure" would leave two providers
// alive after `setDialect`, and Facet.combine's `values[0]` pick is not
// guaranteed to prefer the reconfigured one.
const dialectCompartment = new Compartment();

/**
 * Resolve a `dialect` option (absent ⇒ preset, `null` ⇒ disabled, explicit ⇒
 * that dialect) to a compiled `ResolvedDialect | null` for `dialectFacet`,
 * pushing the same dialect to the wasm session (when a handle is present) as
 * a side effect. Shared by `brinkStudio(...)` (mount time) and
 * `setDialect(view, d)` (live reconfigure). Returning the resolved value
 * (rather than mutating shared state) is what makes this per-state/per-view:
 * `dialectFacet.of(...)` in the returned extension array scopes it to
 * exactly the `EditorState` it was built for — two views with different
 * dialects (or several `DocumentSessions` instances) never clobber each
 * other, unlike a module-level "current dialect" variable would.
 */
function resolveDialectOption(
  dialect: DialogueDialect | null | undefined,
  handleSlot: DocumentHandleSlot | undefined,
): ResolvedDialect | null {
  // No dialect by default (RULED 2026-08-30): absent and `null` both mean
  // NONE — plain lines, the screenplay layer torn down. The at-cue preset
  // is opt-in (`brink.toml [dialogue] preset = "at-cue"`, or an embedder
  // passing `AT_CUE_DIALECT` explicitly).
  if (dialect === null || dialect === undefined) {
    handleSlot?.handle?.clearDialect();
    return null;
  }
  handleSlot?.handle?.setDialect(dialect);
  return ResolvedDialect.compile(dialect);
}

/**
 * Live-reconfigure an already-mounted editor's dialect (#368): swaps the
 * screenplay compartment (decorations on or off) AND the `dialectFacet`
 * value (its own compartment, reconfigured independently — see the comment
 * on `dialectCompartment`), re-runs the wasm `set_dialect`/`clear_dialect`
 * on the view's current handle, and dispatches `reclassifyEffect` so
 * `elementTypeField` recomputes even though the document text itself didn't
 * change. Pass `null` to tear down the screenplay layer (mirrors
 * `brinkStudio({ dialect: null })`) — `dialectFacet` still resolves to
 * `null` in that case (not just an absent screenplay bundle), since
 * `elementTypeField` reads it unconditionally. `brinkKeymap()` is NOT part
 * of this compartment: the keymap owns the STRUCTURAL weave table
 * (Choice/Gather/ChoiceBody/Narrative Tab/Enter transitions, Home/End,
 * arrows, the Alt-Enter picker), which stays interpreter-owned per the
 * dialect spec — its dialect-specific branches self-guard on element kinds
 * that simply never appear when no dialect is active.
 */
export function setDialect(
  view: EditorView,
  dialect: DialogueDialect | null | undefined,
): void {
  const handleSlot = view.state.facet(documentHandleFacet);
  const resolved = resolveDialectOption(dialect, handleSlot ?? undefined);
  // `undefined` = no dialect but the layer stays mounted (the ruled
  // default; structural line attrs live in it); `null` = headless teardown.
  const screenplayLayer: Extension =
    dialect === null ? [] : screenplayDecorations();
  view.dispatch({
    effects: [
      dialectCompartment.reconfigure(dialectFacet.of(resolved)),
      screenplayCompartment.reconfigure(screenplayLayer),
      reclassifyEffect.of(undefined),
    ],
  });
}

export function brinkStudio(options: BrinkStudioOptions): Extension {
  const ideExtensions: Extension[] = [];

  if (options.getCompletions) {
    ideExtensions.push(
      completionsExtension({
        getCompletions: options.getCompletions,
        autoImport: options.autoImport,
      }),
    );
  }
  if (options.getHover) {
    const baseGetHover = options.getHover;
    const { getRuntimeValueNote } = options;
    // W12/#3305: merge the live session's value into the hover.
    const getHover = getRuntimeValueNote
      ? async (source: string, offset: number) =>
          augmentHoverWithRuntimeValue(
            source,
            offset,
            await baseGetHover(source, offset),
            getRuntimeValueNote,
          )
      : baseGetHover;
    ideExtensions.push(
      hoverExtension({
        getHover,
        getArgumentWidgets: options.getArgumentWidgets,
        // The same hook goto-definition uses: a hover-card reference and a
        // goto both mean "take me to the declaration", and routing them
        // apart would let one work while the other silently did not.
        onNavigate: options.onNavigateToFile,
      }),
    );
  }
  if (options.gotoDefinition) {
    ideExtensions.push(
      gotoDefinitionExtension({
        gotoDefinition: options.gotoDefinition,
        onNavigateToFile: options.onNavigateToFile,
        getActiveFile: options.getActiveFile,
        // Cmd-click on the definition itself runs Find References instead
        // of a no-op self-navigation (ruled 2026-08-24).
        findReferences: options.findReferences,
        onShowReferences: options.onShowReferences,
      }),
    );
  }
  if (options.getFoldingRanges) {
    ideExtensions.push(
      foldingExtension({
        getFoldingRanges: options.getFoldingRanges,
        prepareFoldRanges: options.prepareFoldRanges,
      }),
    );
  }
  if (options.getInlayHints) {
    ideExtensions.push(
      inlayHintsExtension({
        getInlayHints: options.getInlayHints,
        prepareHints: options.prepareHints,
      }),
    );
  }
  if (options.getArgumentWidgets) {
    ideExtensions.push(
      argumentWidgetsExtension({
        getArgumentWidgets: options.getArgumentWidgets,
        prepareWidgets: options.prepareWidgets,
        formGlyph: options.argumentFormGlyph,
        autoOpen: options.argumentAutoOpen,
      }),
    );
  }
  if (options.getSignatureHelp) {
    ideExtensions.push(
      signatureHelpExtension({ getSignatureHelp: options.getSignatureHelp }),
    );
  }
  if (options.findReferences) {
    ideExtensions.push(
      referencesExtension({
        findReferences: options.findReferences,
        gotoDefinition: options.gotoDefinition,
        onShowReferences: options.onShowReferences,
      }),
    );
  }
  if (options.prepareRename && options.renameSymbolAt && options.commitRename) {
    ideExtensions.push(
      renameExtension({
        prepareRename: options.prepareRename,
        renameSymbolAt: options.renameSymbolAt,
        commitRename: options.commitRename,
        onBreakage: options.onRenameBreakage,
      }),
    );
  }
  if (options.getCodeActions) {
    const { computeExtract, applyExtract, applyCodeAction } = options;
    const extractEnabled =
      computeExtract !== undefined && applyExtract !== undefined;
    ideExtensions.push(
      codeActionsExtension({
        getCodeActions: options.getCodeActions,
        // Extract entries appear only when the extract seam is wired.
        getSelectionActions: extractEnabled
          ? (view) => extractCodeActions(view.state)
          : undefined,
        // Dispatch: extract actions open the name prompt; everything else
        // resolves + applies through the #321 apply seam.
        onSelect: (action, view) => {
          if (isExtractAction(action)) {
            if (!extractEnabled) return;
            const kind: ExtractKind =
              action.data.action === EXTRACT_TO_KNOT_ACTION
                ? "knot"
                : "function";
            const sel = view.state.selection.main;
            // Snap to whole lines so the prompt anchor + wasm op agree.
            const start = view.state.doc.lineAt(sel.from).from;
            const end = view.state.doc.lineAt(sel.to).to;
            startExtractPrompt(view, kind, { start, end });
            return;
          }
          applyCodeAction?.(action);
        },
      }),
    );
    if (computeExtract !== undefined && applyExtract !== undefined) {
      ideExtensions.push(
        extractActionsExtension({ computeExtract, applyExtract }),
      );
    }
  }
  if (options.onPlayFrom) {
    ideExtensions.push(
      playFromHereExtension({
        onPlayFrom: options.onPlayFrom,
        onSymbolContextMenu: options.onSymbolContextMenu,
        onTextContextMenu: options.onTextContextMenu,
        // The menu's Navigate/Rename identity group reuses the same
        // callbacks the cmd-click / Shift-Alt-F / F2 surfaces use.
        gotoDefinition: options.gotoDefinition,
        findReferences: options.findReferences,
        onShowReferences: options.onShowReferences,
        getActiveFile: options.getActiveFile,
        onNavigateToFile: options.onNavigateToFile,
        renameEnabled: Boolean(
          options.prepareRename &&
          options.renameSymbolAt &&
          options.commitRename,
        ),
        prepareRename: options.prepareRename,
        getBreakpoints: options.getBreakpoints,
        onToggleBreakpoint: options.onToggleBreakpoint,
        onBreakpointsMoved: options.onBreakpointsMoved,
        getExecutionHighlights: options.getExecutionHighlights,
        onRevealInstructions: options.onRevealInstructions,
        canRevealInstructions: options.canRevealInstructions,
      }),
    );
  }
  if (options.getExecutionHighlights) {
    ideExtensions.push(
      executionHighlightExtension({
        getExecutionHighlights: options.getExecutionHighlights,
      }),
    );
  }
  // Host gutter (#343) — registered after the play-from-here gutter so its
  // slot is defined: built-in play gutter first, host gutter beside the text.
  if (options.getGutterMarkers) {
    ideExtensions.push(
      hostGutterExtension({
        getGutterMarkers: options.getGutterMarkers,
        onGutterMarkerClick: options.onGutterMarkerClick,
      }),
    );
  }

  // Theme opt-out (#363): `false` ⇒ headless (host CSS owns the skin);
  // an Extension ⇒ the host's own theme; absent ⇒ the studio brinkTheme.
  const theme: Extension =
    options.theme === false ? [] : (options.theme ?? brinkTheme);

  // Dialect (#368): `dialect: null` OR absent ⇒ NO dialect (RULED
  // 2026-08-30 "no dialect by default" — the screenplay layer is torn
  // down; the at-cue preset is opt-in); an explicit dialect ⇒ that
  // dialect. Resolved once here and
  // provided via `dialectFacet` (its own compartment, so `setDialect` can
  // reconfigure it independent of the screenplay bundle) — scoped to THIS
  // state/view, not a module global, so two views with different dialects
  // never clobber each other. Also pushed to the wasm session when a handle
  // is present, so Rust-side `line_contexts` classifies with it.
  // `elementTypeField` (structural classification: Choice/Gather/Divert/…
  // depth, StatusBar, folding, transitions) and `brinkKeymap()` (the
  // STRUCTURAL weave table: Choice/Gather/ChoiceBody/Narrative Tab/Enter
  // transitions, Home/End, arrows, the Alt-Enter picker — interpreter-owned
  // per the dialect spec) are NOT part of the screenplay gate — both always
  // run, dialect or not; the keymap's dialect-specific branches self-guard
  // on element kinds that never appear when no dialect is active. Only the
  // screenplay decorations/atomic-ranges/edit-guard are gated by
  // `screenplayCompartment`.
  const resolvedDialect = resolveDialectOption(
    options.dialect,
    options.handleSlot,
  );
  // Only an EXPLICIT `null` tears the screenplay layer down (headless
  // embedding). The layer also carries the interpreter-owned structural
  // line attrs (choice depth, option paths), so "no dialect" — the ruled
  // default — keeps it mounted; with no dialect facet the classifier
  // simply never yields a dialect kind, so no cue decoration ever renders.
  const screenplayLayer: Extension =
    options.dialect === null ? [] : screenplayDecorations();

  return [
    // Tooltips (#3349): reparent hover/lint/autocomplete out of `.cm-editor`
    // so a sibling pane's stacking context or `overflow` (the Player split)
    // never clips them. See `tooltip-portal.ts` for why this can't just be
    // `tooltips({ parent: document.body })` inline.
    tooltipPortalExtension(),
    // Viewport/scroll instrumentation (measure-first ruling, 2026-08-24).
    // Inert branches while the probe is disabled — the production state.
    perfViewportProbe(),
    // The per-view document-handle slot, readable by every extension and the
    // elementTypeField via state.facet(documentHandleFacet).
    documentHandleFacet.of(options.handleSlot ?? null),
    dialectCompartment.of(dialectFacet.of(resolvedDialect)),
    // The rebindable editor-action chords (rename, references, code
    // actions, argument form, element picker) — one keymap in a
    // compartment, dispatching into whichever feature runners the options
    // above wired. Hosts rebind live via `setEditorActionKeys`.
    editorActionKeymap(),
    elementTypeField,
    theme,
    // The project's indent width (#3149) — drives the guide spacing below
    // (the markers package reads this facet) and any indent-aware command.
    // Four by default, which was this line's hardcoded value before the
    // config key existed; the difference is that the formatter now reads
    // the same number instead of its own.
    indentUnit.of(" ".repeat(options.indent ?? DEFAULT_INDENT)),
    // Hanging indent for wrapped lines (the literal-whitespace ruling's
    // companion): continuation rows align even with the first row's text
    // start, so the indent guides never cross wrapped text.
    hangingIndent(),
    // Indent guides (ruled 2026-08-23): tokens, not hardcoded colors — the
    // extension interpolates these strings into its generated stylesheet,
    // where `var()` resolves against the host theme like any other rule.
    options.indentGuides === false
      ? []
      : [
          indentationMarkers({
            hideFirstIndent: true,
            // OFF and load-bearing (maintainer perf report, 2026-08-23):
            // the active-block highlight regenerates every visible guide on
            // EVERY cursor move, and its block scan walks lazily-computed
            // indentation from the cursor toward BOTH ends of the whole
            // document — O(doc) per keystroke, catastrophic on a
            // real-project file. The static guides never pay that cost.
            highlightActiveBlock: false,
            thickness: 1,
            colors: {
              light: "var(--bs-border)",
              dark: "var(--bs-border)",
              activeLight: "var(--bs-border-strong, var(--bs-fg-muted))",
              activeDark: "var(--bs-border-strong, var(--bs-fg-muted))",
            },
          }),
          EditorView.baseTheme({
            // ── Column alignment (#3141) ──────────────────────────────
            //
            // The package paints its guides HALF A CHARACTER right of the
            // column they mark. Not a rounding artefact — it is literal in
            // the upstream gradient, which builds its background-position
            // as `${startOffset * indentWidth}.5ch`, appending a `.5` to
            // every stop. So a caret sitting at that indent level lands
            // half a character LEFT of its own guide, which reads as if
            // one more space were needed to line up.
            //
            // Constant, not compounding: the `.5` is added once per stop
            // rather than accumulated, so depth 1 and depth 6 are equally
            // wrong. That is why this is a fixed shift rather than a
            // character-width correction.
            //
            // Shifting the pseudo LEFT beats overriding
            // `background-position`, which is where the offset actually
            // lives: that property also carries `startOffset * indentWidth`
            // — the first visible column, which matters for a horizontally
            // scrolled line — and rewriting it here would have to
            // reconstruct that from state this rule cannot see. `2px` is
            // the package's own value, mirroring `.cm-line`'s padding.
            //
            // `ch` rather than a pixel nudge because the editor font size
            // is user-settable (Mod-= / Mod--): a fixed 3.6px is only
            // correct at one size, and wrong at both extremes of the range.
            // `.cm-line` is redundant for MATCHING — the marker class only
            // ever lands on a line — but not for WINNING. The package sets
            // `left: 2px` at the same specificity as a bare
            // `.cm-indent-markers::before`, and CodeMirror injects its base
            // theme after ours, so an equal-specificity rule silently loses
            // on order. Measured in the browser: the height override (which
            // the package does not set) applied while the left override did
            // not. One extra class makes it (0,3,1) against (0,2,1) and the
            // order stops mattering.
            ".cm-line.cm-indent-markers::before": {
              left: "calc(2px - 0.5ch)",

              // ── Row gap (#3143, maintainer 2026-08-27) ──────────────
              //
              // Inky draws each row's guide slightly shorter than the row,
              // so consecutive rows show a small break rather than one
              // unbroken rule. The package paints a full-height pseudo, so
              // the gap has to come from the height.
              //
              // `1lh` is one text row exactly, at any line-height — the
              // same unit the wrapped-line rule below already relies on,
              // and the reason a percentage is wrong here (the pseudo is
              // not guaranteed to be exactly one row on a wrapped line).
              // The 2px comes off the BOTTOM, leaving the break under each
              // row where Inky puts it.
              //
              // `bottom: auto` is required, not decorative: the package
              // sets `top: 0` AND `bottom: 0`, which pins both edges and
              // makes `height` inert. Releasing the bottom is what lets the
              // height apply.
              //
              // This ALSO subsumes the wrapped-line fix (maintainer,
              // 2026-08-23): the package paints one full-height pseudo per
              // LINE, so on a wrapped line its guide ran alongside every
              // continuation row. Capping to one text row fixes that too.
              //
              // That earlier fix used to live in its own
              // `.cm-lineWrapping …` rule, which was DELETED here rather
              // than kept — keeping it broke the gap. Line wrapping is on
              // for every line in this editor, so that selector was
              // strictly more specific (0,4,1 vs 0,3,1) and quietly won,
              // putting `height: 1lh` back and cancelling the 2px. Two
              // rules stating the same invariant at different specificities
              // is how you get a fix that measures as applied and renders
              // as absent.
              bottom: "auto",
              height: "calc(1lh - 2px)",
            },
          }),
        ],
    screenplayCompartment.of(screenplayLayer),
    highlightExtension({
      getSemanticTokens: options.getSemanticTokens,
      getSemanticTokensFast: options.getSemanticTokensFast,
      prepareRefined: options.prepareRefined,
      getTokenTypeNames: options.getTokenTypeNames,
    }),
    // The HIR structural overlay (#454) — an independent layer on top of (not
    // replacing) the tok-* token highlight above.
    options.getHirProjection
      ? hirOverlayExtension({
          getHirProjection: options.getHirProjection,
          prepareProjection: options.prepareProjection,
        })
      : [],
    diagnosticsExtension({
      compile: options.compile,
      onCompile: options.onCompile,
      getActiveFile: options.getActiveFile,
    }),
    // Prose checking (#3209). Absent unless the host registers a checker AND
    // supplies the projection: no checker means no checking, which is the
    // correct behaviour for a runtime-only or headless embedder.
    options.getProseChecker && options.getHirProjection
      ? proseExtension({
          getChecker: options.getProseChecker,
          getHirProjection: options.getHirProjection,
          getDictionary: options.getProseDictionary,
          getDialect: options.getProseDialect,
          onAddToDictionary: options.onAddToDictionary,
          onLints: options.onProseLints,
        })
      : [],
    brinkKeymap(),
    ideCompartment.of(ideExtensions),
  ];
}
