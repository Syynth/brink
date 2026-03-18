export { createBrinkEditor } from "./editor/index.js";
export type { BrinkEditorOptions, BrinkEditorHandle, BrinkStudioOptions } from "./editor/index.js";

export { brinkStudio } from "./editor/extensions.js";

export { createBrinkPlayer } from "./player/index.js";
export type { BrinkPlayerHandle } from "./player/index.js";

export {
  initWasm,
  compile,
  getSemanticTokens,
  getTokenTypeNames,
  getTokenModifierNames,
  getCompletions,
  getHover,
  gotoDefinition,
  findReferences,
  prepareRename,
  doRename,
  getCodeActions,
  getInlayHints,
  getSignatureHelp,
  getFoldingRanges,
  getDocumentSymbols,
  formatDocument,
  StoryRunnerHandle,
} from "./wasm.js";
export type {
  CompileResult,
  Diagnostic,
  SemanticToken,
  StepResult,
  Choice,
  CompletionItem,
  HoverInfo,
  Location,
  FileEdit,
  InlayHint,
  SignatureInfo,
  FoldRange,
  DocumentSymbol,
  CodeAction,
} from "./wasm.js";

export { elementTypeField, ElementType } from "./editor/element-type.js";
export type { LineInfo } from "./editor/element-type.js";

export { brinkTheme } from "./editor/theme.js";
