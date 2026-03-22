/**
 * Re-exports from @brink/wasm-types and @brink/wasm.
 *
 * This file exists for backwards compatibility — existing code that imports
 * from "./wasm.js" continues to work during the migration.
 */

// Re-export all types
export type {
  Diagnostic,
  CompileResult,
  SemanticToken,
  Line,
  LineType,
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
  ProjectFile,
  FileOutline,
  ConvertTarget,
  TextEdit,
  IncludeInfo,
  LineElement,
  WeavePosition,
  WeaveElement,
  LineContext,
} from "@brink/wasm-types";

// Re-export all runtime values
export {
  initWasm,
  compile,
  getTokenTypeNames,
  getTokenModifierNames,
  EditorSessionHandle,
  StoryRunnerHandle,
} from "@brink/wasm";
