import { Compartment, type Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { CompileResult, SemanticToken, HirProjection, CompletionItem, HoverInfo, Location, InlayHint, CallWidgetSite, SignatureInfo, FoldRange, CodeAction, StructuralResult, AutoImportResult, DialogueDialect } from "@brink/wasm-types";
import { documentHandleFacet, type DocumentHandleSlot } from "./document-handle.js";
import { indentationMarkers } from "@replit/codemirror-indentation-markers";
import { hangingIndent } from "./hanging-indent.js";
import { indentUnit } from "@codemirror/language";
import { brinkTheme } from "./theme.js";
import { screenplayDecorations } from "./screenplay.js";
import { AT_CUE_DIALECT, ResolvedDialect } from "./dialect.js";
import { dialectFacet, reclassifyEffect, elementTypeField } from "./element-type.js";
import { highlightExtension } from "./highlight.js";
import { diagnosticsExtension } from "./diagnostics.js";
import { brinkKeymap } from "./keybindings.js";
import { completionsExtension } from "./completions.js";
import { hoverExtension } from "./hover.js";
import { gotoDefinitionExtension } from "./goto-definition.js";
import { foldingExtension } from "./folding.js";
import { inlayHintsExtension } from "./inlay-hints.js";
import { argumentWidgetsExtension, type FormGlyphMode } from "./argument-widgets.js";
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
import { playFromHereExtension } from "./play-from-here.js";
import { hostGutterExtension, type HostGutterMarker } from "./host-gutter.js";
import { hirOverlayExtension } from "./hir-overlay.js";

export interface BrinkStudioOptions {
  compile: (source: string) => CompileResult;
  getSemanticTokens: (source: string) => SemanticToken[];
  getTokenTypeNames: () => string[];
  onCompile?: (result: CompileResult) => void;

  /** The HIR structural projection for this document (#454). When provided,
   *  the editor renders the structural overlay: `brink-hir-*` inline marks
   *  with `data-*` identity, per-line rail attributes + the rails gutter, and
   *  identity-keyed occurrence highlighting. Omit for no overlay. */
  getHirProjection?: () => HirProjection;

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

  /** The view's wasm document-handle slot (per-view DocId, swapped across
   *  mount/unmount). If provided, the editor uses HIR-backed line
   *  classification (`line_contexts_doc`) instead of the regex classifier,
   *  and transition actions can convert elements. */
  handleSlot?: DocumentHandleSlot;

  /**
   * The dialogue dialect (#368) driving screenplay classification,
   * decorations, transitions, and conversions. Defaults to `AT_CUE_DIALECT`
   * (byte-identical to the pre-#368 hardcoded `@Name:<>` behavior). Pass
   * `null` to tear down the ENTIRE screenplay layer — classification,
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
  getCompletions?: (source: string, offset: number) => CompletionItem[];
  /** Auto-import (#312 F): on accepting an out-of-scope completion, ensure the
   *  current file `INCLUDE`s the symbol's source file. Only consulted when
   *  `getCompletions` is also provided. */
  autoImport?: (target: string) => AutoImportResult;
  getHover?: (source: string, offset: number) => HoverInfo | null;
  gotoDefinition?: (source: string, offset: number) => Location | null;
  /** Called when goto-definition targets a different file. */
  onNavigateToFile?: (location: Location) => void;
  /** Returns the current active file path (for cross-file navigation detection). */
  getActiveFile?: () => string;
  findReferences?: (source: string, offset: number) => Location[];
  prepareRename?: (source: string, offset: number) => Location | null;
  /** Live (debounced) safe-rename query for the inline-rename badge (#323/#324):
   *  computes the new sources + breakage report without applying anything.
   *  `offset` is in view coords; the host folds in any fragment-view origin. */
  renameSymbolAt?: (offset: number, newName: string) => StructuralResult;
  /** Commit an inline rename — apply the (already-computed) edits across files.
   *  Called on a safe Enter or an explicit "Rename anyway". `currentName` is the
   *  symbol's original name (for re-keying open symbol tabs). */
  commitRename?: (result: StructuralResult, newName: string, currentName: string) => void;
  /** Optional host override for the inline breakage surface (#324). Return
   *  `true` to suppress the default inline report and render your own. */
  onRenameBreakage?: (result: StructuralResult, ctx: BreakageContext) => boolean;
  getCodeActions?: (source: string, offset: number) => CodeAction[];
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
  applyExtract?: (kind: ExtractKind, result: StructuralResult, name: string) => void;
  getInlayHints?: (source: string, start: number, end: number) => InlayHint[];
  getArgumentWidgets?: (source: string, start: number, end: number) => CallWidgetSite[];
  /** How the inline call-level argument-form glyph is shown. Default `off`. */
  argumentFormGlyph?: FormGlyphMode;
  /** Accepting a function completion inserts `()` + opens the Form. Default false. */
  argumentAutoOpen?: boolean;
  getSignatureHelp?: (source: string, offset: number) => SignatureInfo | null;
  getFoldingRanges?: (source: string) => FoldRange[];
  /** Start a play session entered at a knot/stitch (`onPlayFrom("knot.stitch")`).
   *  When provided, the editor shows a hover ▶ run-icon on knot/stitch
   *  declarations (#186). */
  onPlayFrom?: (inkPath: string, label?: string) => void;
  /** Right-click a knot/stitch declaration → the shared symbol context menu. */
  onSymbolContextMenu?: (info: { knot: string; stitch?: string }, x: number, y: number) => void;
  /**
   * Host gutter-marker contribution (#343): the host's markers (breakpoints,
   * per-line annotations, run/flag icons) for the inclusive 1-based line range
   * `[fromLine, toLine]`. When provided, they render in a dedicated gutter
   * slotted after (to the right of) the built-in play-from-here gutter.
   * Recomputed on document changes; dispatch `refreshGutterMarkersEffect` (or
   * call `refreshGutterMarkers(view)`) when the marker set changes externally.
   */
  getGutterMarkers?: (source: string, fromLine: number, toLine: number) => HostGutterMarker[];
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
  if (dialect === null) {
    handleSlot?.handle?.clearDialect();
    return null;
  }
  const resolved = dialect ?? AT_CUE_DIALECT;
  handleSlot?.handle?.setDialect(resolved);
  return ResolvedDialect.compile(resolved);
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
export function setDialect(view: EditorView, dialect: DialogueDialect | null): void {
  const handleSlot = view.state.facet(documentHandleFacet);
  const resolved = resolveDialectOption(dialect, handleSlot ?? undefined);
  const screenplayLayer: Extension = dialect === null ? [] : screenplayDecorations();
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
    ideExtensions.push(
      hoverExtension({
        getHover: options.getHover,
        getArgumentWidgets: options.getArgumentWidgets,
      }),
    );
  }
  if (options.gotoDefinition) {
    ideExtensions.push(gotoDefinitionExtension({
      gotoDefinition: options.gotoDefinition,
      onNavigateToFile: options.onNavigateToFile,
      getActiveFile: options.getActiveFile,
    }));
  }
  if (options.getFoldingRanges) {
    ideExtensions.push(foldingExtension({ getFoldingRanges: options.getFoldingRanges }));
  }
  if (options.getInlayHints) {
    ideExtensions.push(inlayHintsExtension({ getInlayHints: options.getInlayHints }));
  }
  if (options.getArgumentWidgets) {
    ideExtensions.push(
      argumentWidgetsExtension({
        getArgumentWidgets: options.getArgumentWidgets,
        formGlyph: options.argumentFormGlyph,
        autoOpen: options.argumentAutoOpen,
      }),
    );
  }
  if (options.getSignatureHelp) {
    ideExtensions.push(signatureHelpExtension({ getSignatureHelp: options.getSignatureHelp }));
  }
  if (options.findReferences) {
    ideExtensions.push(referencesExtension({ findReferences: options.findReferences }));
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
    const extractEnabled = computeExtract !== undefined && applyExtract !== undefined;
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
              action.data.action === EXTRACT_TO_KNOT_ACTION ? "knot" : "function";
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
      ideExtensions.push(extractActionsExtension({ computeExtract, applyExtract }));
    }
  }
  if (options.onPlayFrom) {
    ideExtensions.push(
      playFromHereExtension({
        onPlayFrom: options.onPlayFrom,
        onSymbolContextMenu: options.onSymbolContextMenu,
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
  const theme: Extension = options.theme === false ? [] : (options.theme ?? brinkTheme);

  // Dialect (#368): `dialect: null` tears down the screenplay-specific layer
  // (decorations/keybindings); absent ⇒ the at-cue preset (byte-identical
  // default); an explicit dialect ⇒ that dialect. Resolved once here and
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
  const resolvedDialect = resolveDialectOption(options.dialect, options.handleSlot);
  const screenplayLayer: Extension =
    options.dialect === null ? [] : screenplayDecorations();

  return [
    // The per-view document-handle slot, readable by every extension and the
    // elementTypeField via state.facet(documentHandleFacet).
    documentHandleFacet.of(options.handleSlot ?? null),
    dialectCompartment.of(dialectFacet.of(resolvedDialect)),
    elementTypeField,
    theme,
    // Four-column indent unit (maintainer, 2026-08-23): ink convention —
    // drives the guide spacing below (the markers package reads this
    // facet) and any indent-aware command.
    indentUnit.of("    "),
    // Hanging indent for wrapped lines (the literal-whitespace ruling's
    // companion): continuation rows align even with the first row's text
    // start, so the indent guides never cross wrapped text.
    hangingIndent(),
    // Indent guides (ruled 2026-08-23): tokens, not hardcoded colors — the
    // extension interpolates these strings into its generated stylesheet,
    // where `var()` resolves against the host theme like any other rule.
    options.indentGuides === false
      ? []
      : indentationMarkers({
          hideFirstIndent: true,
          thickness: 1,
          colors: {
            light: "var(--bs-border)",
            dark: "var(--bs-border)",
            activeLight: "var(--bs-border-strong, var(--bs-fg-muted))",
            activeDark: "var(--bs-border-strong, var(--bs-fg-muted))",
          },
        }),
    screenplayCompartment.of(screenplayLayer),
    highlightExtension({
      getSemanticTokens: options.getSemanticTokens,
      getTokenTypeNames: options.getTokenTypeNames,
    }),
    // The HIR structural overlay (#454) — an independent layer on top of (not
    // replacing) the tok-* token highlight above.
    options.getHirProjection
      ? hirOverlayExtension({ getHirProjection: options.getHirProjection })
      : [],
    diagnosticsExtension({
      compile: options.compile,
      onCompile: options.onCompile,
      getActiveFile: options.getActiveFile,
    }),
    brinkKeymap(),
    ideCompartment.of(ideExtensions),
  ];
}
