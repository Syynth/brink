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
export {
  foldPlayerRuns,
  speakerPaletteIndex,
  type PlayerGroup,
  type PlayerRow,
} from "./player-runs.js";
export { Binder, BinderRow, computeReorder, buildBinderTree, LIBRARY_ROW_KEY } from "./Binder.js";
export { fileMarks, symbolMarks, filterOutline, type RowMarks } from "./Binder.js";
export type { FolderNode } from "./Binder.js";
export { SymbolContextMenuHost } from "./SymbolContextMenuHost.js";
export { SymbolRenamePrompt } from "./SymbolRenamePrompt.js";
export {
  dispatchSymbolAction,
  performSymbolRename,
  applyComputedRename,
  type SymbolRenameOutcome,
} from "./symbolMenuActions.js";
export { useSymbolMenuActions } from "./useSymbolMenuActions.js";
export { INK_FILE_TYPE_ID, InkFileDocument, inkFileRef, inkDocPath, isOutOfScope } from "./InkFileDocument.js";
export { DocumentIcon, type DocumentIconProps } from "./DocumentIcon.js";
export { LintSettings } from "./LintSettings.js";
export { FormattingSettings } from "./FormattingSettings.js";
export { ProseSettings } from "./ProseSettings.js";
export { DraftSettings } from "./DraftSettings.js";
export { PlayerReadingSection, PlayerReadingAidsSection, CURATED_FONTS } from "./PlayerStyling.js";
export { ConventionsSettings } from "./ConventionsSettings.js";
export { renderRowBody } from "./PlayerPane.js";
export { KeymapSettings } from "./KeymapSettings.js";
export { ThemePicker } from "./ThemePicker.js";
export {
  SettingsGroup,
  SettingsRow,
  SettingsStepper,
  SettingsToggle,
} from "./SettingsRow.js";
export {
  SettingsModal,
  SETTINGS_ICONS,
  type SettingsSection,
} from "./SettingsModal.js";
export { settingsSections } from "./settingsSections.js";
export {
  DEFAULT_SETTINGS_SECTION,
  SETTINGS_SECTION_IDS,
} from "./settingsSectionIds.js";
export {
  isSuppressible,
  suppressAllInFile,
  suppressInFile,
  suppressOnLine,
} from "./suppressDiagnostic.js";
export {
  ProblemsContextMenu,
  type ProblemsMenuTarget,
} from "./ProblemsContextMenu.js";
export { ConfigFormPanel, isConfigPath } from "./ConfigFormPanel.js";
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
  EDITOR_STORAGE_KEY,
  DEBUG_STORAGE_KEY,
  loadDebugSettings,
  loadPlayerSettings,
  savePlayerSettings,
  PLAYER_STORAGE_KEY,
  loadEditorSettings,
  registerSettingsCommand,
  saveDebugSettings,
  saveDiagnosticsSettings,
  saveEditorSettings,
  settingsRef,
  type DebugSettings,
  type DiagnosticsSettings,
  type EditorSettings,
} from "./SettingsDocument.js";
export { NewFilePrompt, FILE_NEW_COMMAND_ID } from "./NewFilePrompt.js";
export { ConflictMergeView } from "./ConflictMergeView.js";
export {
  CompileStatusSegment,
  ScopeNoteSegment,
  CursorSegment,
  ElementSegment,
  KeyHintsSegment,
  StorySegment,
  StructuralOpSegment,
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
export { DebuggerPanel, DebuggerActions } from "./DebuggerPanel.js";
export { PlayerLauncher } from "./PlayerLauncher.js";
export { StateView } from "./StateView.js";
export { ProgramExplorerActions, ProgramView } from "./ProgramView.js";
export {
  ProblemsActions,
  ProblemsView,
  countBySeverity,
  filterProblemRows,
  groupProblemRows,
  matchesProblemFilter,
  severityBucket,
  summarizeCounts,
  type ProblemFileGroup,
  ProblemsBadge,
  buildProblemRows,
  diagnosticLocation,
  offsetToLineCol,
  type ProblemRow,
} from "./ProblemsView.js";
export {
  TodosActions,
  TodosView,
  TodosBadge,
  TODO_DIAGNOSTIC_CODE,
  collectTodoItems,
  containerAt,
  matchesTodoFilter,
  groupTodoItems,
  todoKey,
  keyTodoItems,
  type TodoItem,
  type TodoContainerGroup,
  type TodoFileGroup,
  splitTodoTag,
  todoTags,
  filterTodosByTag,
} from "./TodosView.js";
export { OutputView, formatOutputTimestamp } from "./OutputView.js";
export { PerfView } from "./PerfView.js";
export type { PerfViewBridge, WasmCounterMap } from "./PerfView.js";
export {
  SEARCH_DEBOUNCE_MS,
  SEARCH_FOCUS_COMMAND_ID,
  SEARCH_TOOL_WINDOW_ID,
  SearchCommands,
  SearchView,
  registerSearchFocusCommand,
} from "./SearchView.js";
export { SearchCardList } from "./SearchCardList.js";
export { ElementDropdown } from "./ElementDropdown.js";
export { QuickOpen, QUICK_OPEN_COMMAND_ID, buildQuickOpenItems } from "./QuickOpen.js";
export { BinderContextMenu } from "./BinderContextMenu.js";
export { EditorTextMenuHost } from "./EditorTextMenuHost.js";
export { StudioContinuousView } from "./StudioContinuousView.js";
