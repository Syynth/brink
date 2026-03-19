export { createBrinkEditor } from "./editor/index.js";
export type { BrinkEditorOptions, BrinkEditorHandle, BrinkStudioOptions } from "./editor/index.js";

export { brinkStudio } from "./editor/extensions.js";

export { createBrinkPlayer } from "./player/index.js";
export type { BrinkPlayerHandle } from "./player/index.js";

export {
  initWasm,
  compile,
  getTokenTypeNames,
  getTokenModifierNames,
  EditorSessionHandle,
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
  LineContext,
  WeavePosition,
  WeaveElement,
  LineElement,
  ProjectFile,
  FileOutline,
} from "./wasm.js";

export { createBinder } from "./binder/index.js";
export type { BinderOptions, BinderHandle } from "./binder/index.js";

export { elementTypeField, ElementType, setEditorSession } from "./editor/element-type.js";
export type { LineInfo } from "./editor/element-type.js";

export { brinkTheme } from "./editor/theme.js";
