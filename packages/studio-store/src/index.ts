/**
 * @brink/studio-store — Zustand store for brink-studio React migration.
 *
 * Combines domain slices (editor, compile, tabs, session, player, binder) into a
 * single store. Non-reactive refs (prefixed with _) hold imperative handles
 * that should not trigger re-renders.
 */

import { create } from "zustand";

import type { EditorSlice } from "./slices/editor.js";
import type { CompileSlice } from "./slices/compile.js";
import type { TabsSlice } from "./slices/tabs.js";
import type { SessionSlice } from "./slices/session.js";
import type { BinderSlice } from "./slices/binder.js";
import type { OutputSlice } from "./slices/output.js";
import type { InkEditorHandle, EditorStateManager, ProjectSession } from "./types.js";

import { createEditorSlice } from "./slices/editor.js";
import { createCompileSlice } from "./slices/compile.js";
import { createTabsSlice } from "./slices/tabs.js";
import { createSessionSlice } from "./slices/session.js";
import { createBinderSlice } from "./slices/binder.js";
import { createOutputSlice } from "./slices/output.js";

// ── Combined state ──────────────────────────────────────────────────

export interface StudioState
  extends EditorSlice,
    CompileSlice,
    TabsSlice,
    SessionSlice,
    BinderSlice,
    OutputSlice {
  // Non-reactive refs — imperative handles that don't trigger re-renders
  _editorRef: InkEditorHandle | null;
  _stateManager: EditorStateManager | null;
  _project: ProjectSession | null;

  initialize(
    project: ProjectSession,
    stateManager: EditorStateManager,
    editorRef: InkEditorHandle,
  ): void;
}

// ── Store factory ───────────────────────────────────────────────────

export const createStudioStore = () =>
  create<StudioState>()((...args) => {
    const [set, get] = args;

    return {
      // Slices
      ...createEditorSlice(...args),
      ...createCompileSlice(...args),
      ...createTabsSlice(...args),
      ...createSessionSlice(...args),
      ...createBinderSlice(...args),
      ...createOutputSlice(...args),

      // Non-reactive refs
      _editorRef: null,
      _stateManager: null,
      _project: null,

      // Initialization — binds imperative handles and syncs initial state
      initialize(project, stateManager, editorRef) {
        const tabs = [...stateManager.getTabs()];
        const activeTab = stateManager.getActiveTab();
        const files: string[] = stateManager.files();

        set({
          _project: project,
          _stateManager: stateManager,
          _editorRef: editorRef,
          tabs,
          activeTabId: activeTab.id,
          files,
        });

        // Trigger an initial compile to populate outline/diagnostics
        get()._editorRef?.triggerCompile();
      },
    };
  });

// ── Typed store instance type ───────────────────────────────────────

export type StudioStore = ReturnType<typeof createStudioStore>;

// ── Re-exports ──────────────────────────────────────────────────────

// Exposed for unit testing the replay loop's termination + divergence
// behavior (spec §7.6).
export {
  replayChoices,
  sessionCanContinue,
  REPLAY_DIVERGED_MESSAGE,
  type SessionStatus,
} from "./slices/session.js";

// Problems ordering (canonical sort, unit-testable pure helper).
export { sortDiagnostics } from "./slices/compile.js";

// Output log (Output tool window, spec §4) — entries + growth cap.
export {
  OUTPUT_LOG_LIMIT,
  type OutputEntry,
  type OutputSource,
} from "./slices/output.js";

export type {
  ElementType,
  LineInfo,
  KeyHint,
  TabTarget,
  TabInfo,
  InkEditorHandle,
  EditorStateManager,
  ProjectSession,
} from "./types.js";

export { ElementType as ElementTypeEnum } from "./types.js";
