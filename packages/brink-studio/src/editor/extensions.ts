import { Compartment, type Extension } from "@codemirror/state";
import type { CompileResult, SemanticToken, CompletionItem, HoverInfo, Location, FileEdit, InlayHint, SignatureInfo, FoldRange, CodeAction } from "../wasm.js";
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
import { signatureHelpExtension } from "./signature-help.js";
import { referencesExtension } from "./references.js";
import { renameExtension } from "./rename.js";
import { codeActionsExtension } from "./code-actions.js";
import { statusBarExtension } from "./statusbar.js";

export interface BrinkStudioOptions {
  compile: (source: string) => CompileResult;
  getSemanticTokens: (source: string) => SemanticToken[];
  getTokenTypeNames: () => string[];
  onCompile?: (result: CompileResult) => void;

  // IDE features (all optional — features are enabled when provided)
  getCompletions?: (source: string, offset: number) => CompletionItem[];
  getHover?: (source: string, offset: number) => HoverInfo | null;
  gotoDefinition?: (source: string, offset: number) => Location | null;
  findReferences?: (source: string, offset: number) => Location[];
  prepareRename?: (source: string, offset: number) => Location | null;
  doRename?: (source: string, offset: number, newName: string) => FileEdit[];
  getCodeActions?: (source: string, offset: number) => CodeAction[];
  getInlayHints?: (source: string, start: number, end: number) => InlayHint[];
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
    ideExtensions.push(gotoDefinitionExtension({ gotoDefinition: options.gotoDefinition }));
  }
  if (options.getFoldingRanges) {
    ideExtensions.push(foldingExtension({ getFoldingRanges: options.getFoldingRanges }));
  }
  if (options.getInlayHints) {
    ideExtensions.push(inlayHintsExtension({ getInlayHints: options.getInlayHints }));
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
    brinkTheme,
    screenplayCompartment.of(screenplayDecorations()),
    highlightExtension({
      getSemanticTokens: options.getSemanticTokens,
      getTokenTypeNames: options.getTokenTypeNames,
    }),
    diagnosticsExtension({
      compile: options.compile,
      onCompile: options.onCompile,
    }),
    brinkKeymap(),
    statusBarExtension(),
    ideCompartment.of(ideExtensions),
  ];
}
