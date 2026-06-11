import "./studio.css";

export { StoreProvider, useStudioStore, useStudioStoreApi } from "./StoreContext.js";
export { App } from "./App.js";
export { Binder, computeReorder } from "./Binder.js";
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
export { NewFilePrompt, FILE_NEW_COMMAND_ID } from "./NewFilePrompt.js";
export {
  CompileStatusSegment,
  CursorSegment,
  ElementSegment,
  KeyHintsSegment,
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
export { ElementDropdown } from "./ElementDropdown.js";
export { QuickOpen, QUICK_OPEN_COMMAND_ID, buildQuickOpenItems } from "./QuickOpen.js";
export { BinderContextMenu } from "./BinderContextMenu.js";
