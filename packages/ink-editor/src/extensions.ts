import { Compartment, type Extension } from "@codemirror/state";
import type { CompileResult, SemanticToken, CompletionItem, HoverInfo, Location, InlayHint, CallWidgetSite, SignatureInfo, FoldRange, CodeAction, StructuralResult, AutoImportResult } from "@brink/wasm-types";
import { documentHandleFacet, type DocumentHandleSlot } from "./document-handle.js";
import { brinkTheme } from "./theme.js";
import { screenplayDecorations } from "./screenplay.js";
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

export interface BrinkStudioOptions {
  compile: (source: string) => CompileResult;
  getSemanticTokens: (source: string) => SemanticToken[];
  getTokenTypeNames: () => string[];
  onCompile?: (result: CompileResult) => void;

  /** The editor's skin (#363 headless-ready). Defaults to `brinkTheme` (the
   *  `--bs-*`-token CM theme brink-studio uses). Pass `false` for a headless
   *  editor — no theme at all; the host styles the documented class taxonomy
   *  directly (docs/editor-consumer-guide.md). Pass your own `Extension` to
   *  substitute a different CM theme. Structural styles (popup positioning,
   *  data-driven widget colors) are independent of this and always active. */
  theme?: Extension | false;

  /** The view's wasm document-handle slot (per-view DocId, swapped across
   *  mount/unmount). If provided, the editor uses HIR-backed line
   *  classification (`line_contexts_doc`) instead of the regex classifier,
   *  and transition actions can convert elements. */
  handleSlot?: DocumentHandleSlot;

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
}

// Compartments for runtime toggling
export const screenplayCompartment = new Compartment();
export const ideCompartment = new Compartment();

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

  // Theme opt-out (#363): `false` ⇒ headless (host CSS owns the skin);
  // an Extension ⇒ the host's own theme; absent ⇒ the studio brinkTheme.
  const theme: Extension = options.theme === false ? [] : (options.theme ?? brinkTheme);

  return [
    // The per-view document-handle slot, readable by every extension and the
    // elementTypeField via state.facet(documentHandleFacet).
    documentHandleFacet.of(options.handleSlot ?? null),
    theme,
    screenplayCompartment.of(screenplayDecorations()),
    highlightExtension({
      getSemanticTokens: options.getSemanticTokens,
      getTokenTypeNames: options.getTokenTypeNames,
    }),
    diagnosticsExtension({
      compile: options.compile,
      onCompile: options.onCompile,
      getActiveFile: options.getActiveFile,
    }),
    brinkKeymap(),
    ideCompartment.of(ideExtensions),
  ];
}
