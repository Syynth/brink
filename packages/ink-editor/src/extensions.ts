import { Compartment, type Extension } from "@codemirror/state";
import type { CompileResult, SemanticToken, CompletionItem, HoverInfo, Location, InlayHint, CallWidgetSite, SignatureInfo, FoldRange, CodeAction } from "@brink/wasm-types";
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
import { renameExtension } from "./rename.js";
import { codeActionsExtension } from "./code-actions.js";
import { playFromHereExtension } from "./play-from-here.js";

export interface BrinkStudioOptions {
  compile: (source: string) => CompileResult;
  getSemanticTokens: (source: string) => SemanticToken[];
  getTokenTypeNames: () => string[];
  onCompile?: (result: CompileResult) => void;

  /** The view's wasm document-handle slot (per-view DocId, swapped across
   *  mount/unmount). If provided, the editor uses HIR-backed line
   *  classification (`line_contexts_doc`) instead of the regex classifier,
   *  and transition actions can convert elements. */
  handleSlot?: DocumentHandleSlot;

  // IDE features (all optional — features are enabled when provided)
  getCompletions?: (source: string, offset: number) => CompletionItem[];
  getHover?: (source: string, offset: number) => HoverInfo | null;
  gotoDefinition?: (source: string, offset: number) => Location | null;
  /** Called when goto-definition targets a different file. */
  onNavigateToFile?: (location: Location) => void;
  /** Returns the current active file path (for cross-file navigation detection). */
  getActiveFile?: () => string;
  findReferences?: (source: string, offset: number) => Location[];
  prepareRename?: (source: string, offset: number) => Location | null;
  /** F2 — begin a rename of the symbol under the cursor; the host opens the
   *  safe-by-default rename prompt (cross-file + breakage report). */
  startRename?: (offset: number, currentName: string) => void;
  getCodeActions?: (source: string, offset: number) => CodeAction[];
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
    ideExtensions.push(completionsExtension({ getCompletions: options.getCompletions }));
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
  if (options.prepareRename && options.startRename) {
    ideExtensions.push(
      renameExtension({ prepareRename: options.prepareRename, startRename: options.startRename }),
    );
  }
  if (options.getCodeActions) {
    ideExtensions.push(codeActionsExtension({ getCodeActions: options.getCodeActions }));
  }
  if (options.onPlayFrom) {
    ideExtensions.push(
      playFromHereExtension({
        onPlayFrom: options.onPlayFrom,
        onSymbolContextMenu: options.onSymbolContextMenu,
      }),
    );
  }

  return [
    // The per-view document-handle slot, readable by every extension and the
    // elementTypeField via state.facet(documentHandleFacet).
    documentHandleFacet.of(options.handleSlot ?? null),
    brinkTheme,
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
