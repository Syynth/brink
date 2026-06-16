// Self-host the editor font so embedders (e.g. RPG Maker MZ / NW.js) get
// JetBrains Mono instead of falling back to the system monospace (#155). These
// side-effect imports register `@font-face`s; the library build inlines the
// woff2 into `style.css`, so consumers get a self-contained stylesheet with no
// asset-path fragility. Latin subset only (code is Latin) and the three weights
// the editor uses — regular, bold (knot/stitch headers), italic (comments) —
// to keep the inlined payload small.
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-700.css";
import "@fontsource/jetbrains-mono/latin-400-italic.css";
import "./styles/index.css";

export { StoreProvider, useStudioStore, useStudioStoreApi } from "./StoreContext.js";
export {
  StudioApiProvider,
  createStudioApi,
  derivePublicState,
  useStudioApi,
  type PublicElementInfo,
  type StudioApi,
  type StudioApiDeps,
  type StudioPublicState,
} from "./StudioApi.js";
export { App } from "./App.js";
export { Binder, computeReorder, buildBinderTree } from "./Binder.js";
export type { FolderNode } from "./Binder.js";
export { INK_FILE_TYPE_ID, InkFileDocument, inkFileRef } from "./InkFileDocument.js";
export {
  COMPILED_OUTPUT_DOC_ID,
  COMPILED_OUTPUT_TYPE_ID,
  CompiledOutputDocument,
  OPEN_COMPILED_OUTPUT_COMMAND_ID,
  compiledOutputExtensions,
  compiledOutputRef,
  registerCompiledOutputCommand,
  replaceCompiledOutput,
} from "./CompiledOutputDocument.js";
export { inktFolding, inktHighlighting, inktLanguage } from "./inkt-mode.js";
export {
  OPEN_STORY_GRAPH_COMMAND_ID,
  STORY_GRAPH_DOC_ID,
  STORY_GRAPH_TYPE_ID,
  StoryGraphDocument,
  registerStoryGraphCommand,
  storyGraphRef,
  toFlowEdges,
  toFlowNodes,
  useStoryGraphModel,
  type StoryGraphModel,
  type StoryNodeData,
} from "./StoryGraphDocument.js";
export {
  buildGraphView,
  buildOverlay,
  currentNodeId,
  nodeVisitCount,
  type DebugStateLike,
  type GraphView,
  type GraphViewEdge,
  type GraphViewNode,
  type SessionOverlay,
} from "./story-graph-model.js";
export { layoutGraphView, type GraphLayout, type NodeLayout } from "./story-graph-layout.js";
export {
  DIAGNOSTICS_STORAGE_KEY,
  OPEN_SETTINGS_COMMAND_ID,
  SETTINGS_DOC_ID,
  SETTINGS_TYPE_ID,
  SettingsDocument,
  loadDiagnosticsSettings,
  loadEditorSettings,
  registerSettingsCommand,
  saveDiagnosticsSettings,
  saveEditorSettings,
  settingsRef,
  type DiagnosticsSettings,
  type EditorSettings,
} from "./SettingsDocument.js";
export { NewFilePrompt, FILE_NEW_COMMAND_ID } from "./NewFilePrompt.js";
export {
  CompileStatusSegment,
  CursorSegment,
  ElementSegment,
  KeyHintsSegment,
  SessionPicker,
  StorySegment,
} from "./StatusBar.js";
export {
  OPEN_PLAYER_COMMAND_ID,
  PLAYER_DOC_ID,
  PLAYER_TYPE_ID,
  PlayerPane,
  openPlayerSplit,
  playerRef,
  registerOpenPlayerCommand,
} from "./PlayerPane.js";
export { StateView } from "./StateView.js";
export { ProgramView } from "./ProgramView.js";
export {
  ProblemsView,
  ProblemsBadge,
  buildProblemRows,
  diagnosticLocation,
  offsetToLineCol,
  type ProblemRow,
} from "./ProblemsView.js";
export { OutputView, formatOutputTimestamp } from "./OutputView.js";
export {
  SEARCH_DEBOUNCE_MS,
  SEARCH_FOCUS_COMMAND_ID,
  SEARCH_TOOL_WINDOW_ID,
  SearchCommands,
  SearchView,
  registerSearchFocusCommand,
} from "./SearchView.js";
export { ElementDropdown } from "./ElementDropdown.js";
export { QuickOpen, QUICK_OPEN_COMMAND_ID, buildQuickOpenItems } from "./QuickOpen.js";
export { BinderContextMenu } from "./BinderContextMenu.js";
