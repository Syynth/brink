import { Compartment, type Extension } from "@codemirror/state";
import type { CompileResult, SemanticToken, CompletionItem, HoverInfo, Location, FileEdit, InlayHint, ColorHint, SignatureInfo, FoldRange, CodeAction } from "@brink/wasm-types";
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
import { colorPickerExtension } from "./color-picker.js";
import { signatureHelpExtension } from "./signature-help.js";
import { referencesExtension } from "./references.js";
import { renameExtension } from "./rename.js";
import { codeActionsExtension } from "./code-actions.js";

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
  doRename?: (source: string, offset: number, newName: string) => FileEdit[];
  getCodeActions?: (source: string, offset: number) => CodeAction[];
  getInlayHints?: (source: string, start: number, end: number) => InlayHint[];
  getColorHints?: (source: string, start: number, end: number) => ColorHint[];
  getSignatureHelp?: (source: string, offset: number) => SignatureInfo | null;
  getFoldingRanges?: (source: string) => FoldRange[];
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
    ideExtensions.push(hoverExtension({ getHover: options.getHover }));
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
  if (options.getColorHints) {
    ideExtensions.push(colorPickerExtension({ getColorHints: options.getColorHints }));
  }
  if (options.getSignatureHelp) {
    ideExtensions.push(signatureHelpExtension({ getSignatureHelp: options.getSignatureHelp }));
  }
  if (options.findReferences) {
    ideExtensions.push(referencesExtension({ findReferences: options.findReferences }));
  }
  if (options.prepareRename && options.doRename) {
    ideExtensions.push(renameExtension({ prepareRename: options.prepareRename, doRename: options.doRename }));
  }
  if (options.getCodeActions) {
    ideExtensions.push(codeActionsExtension({ getCodeActions: options.getCodeActions }));
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
