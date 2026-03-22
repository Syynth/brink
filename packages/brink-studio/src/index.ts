// ── Types (from @brink/wasm-types) ─────────────────────────────
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
  InkEditor,
  brinkStudio,
  elementTypeField,
  ElementType,
  setEditorSession,
  EditorStateManager,
  ProjectSession,
  InMemoryFileProvider,
  brinkTheme,
  convertLineToType,
} from "@brink/ink-editor";
export type {
  InkEditorProps,
  InkEditorHandle,
  KeyHint,
  BrinkStudioOptions,
  LineInfo,
  TabTarget,
  TabInfo,
  ProjectSessionOptions,
  FileProvider,
} from "@brink/ink-editor";

// ── Store (from @brink/studio-store) ────────────────────────────
export { createStudioStore } from "@brink/studio-store";
export type { StudioState, StudioStore } from "@brink/studio-store";

// ── UI (from @brink/studio-ui) ─────────────────────────────────
export {
  StoreProvider,
  useStudioStore,
  App,
  Binder,
  FileTabBar,
  StatusBar,
  PlayerPane,
  EditorPane,
  ElementDropdown,
} from "@brink/studio-ui";
