// ── Types (from @brink/wasm-types) ─────────────────────────────
export type {
  CompileResult,
  Diagnostic,
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
  LineContext,
  WeavePosition,
  WeaveElement,
  LineElement,
  ProjectFile,
  FileOutline,
  IncludeInfo,
} from "@brink/wasm-types";

// ── Wasm bindings (from @brink/wasm) ───────────────────────────
export {
  initWasm,
  compile,
  getTokenTypeNames,
  getTokenModifierNames,
  EditorSessionHandle,
  StoryRunnerHandle,
} from "@brink/wasm";

// ── Pure operations (from @brink/ink-operations) ────────────────
export {
  CONVERTIBLE_TYPES,
  extractLineContent,
  getLineSigilRange,
} from "@brink/ink-operations";

// ── Editor (from @brink/ink-editor) ─────────────────────────────
export {
  brinkStudio,
  elementTypeField,
  ElementType,
  DocHandle,
  DocumentSessions,
  docKeyFor,
  docTitleFor,
  documentHandleFacet,
  parseDocKey,
  syncAnnotation,
  ProjectSession,
  InMemoryFileProvider,
  brinkTheme,
  convertLineToType,
} from "@brink/ink-editor";
export type {
  KeyHint,
  BrinkStudioOptions,
  DocTarget,
  DocumentCallbacks,
  DocumentHandleSlot,
  LineInfo,
  ProjectSessionOptions,
  FileProvider,
} from "@brink/ink-editor";

// ── Store (from @brink/studio-store) ────────────────────────────
export { createStudioStore } from "@brink/studio-store";
export type { StudioState, StudioStore, TabTarget } from "@brink/studio-store";

// ── UI (from @brink/studio-ui) ─────────────────────────────────
export {
  StoreProvider,
  useStudioStore,
  App,
  Binder,
  INK_FILE_TYPE_ID,
  InkFileDocument,
  inkFileRef,
  NewFilePrompt,
  CompileStatusSegment,
  CursorSegment,
  ElementSegment,
  KeyHintsSegment,
  StorySegment,
  PlayerPane,
  ElementDropdown,
} from "@brink/studio-ui";
