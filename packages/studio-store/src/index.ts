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
import type { DocumentsSlice } from "./slices/documents.js";
import type { SessionSlice } from "./slices/session.js";
import type { BinderSlice } from "./slices/binder.js";
import type { OutputSlice } from "./slices/output.js";
import type { SearchSlice } from "./slices/search.js";
import type { DocumentSessions, ProjectSession } from "./types.js";

import { createEditorSlice } from "./slices/editor.js";
import { createCompileSlice } from "./slices/compile.js";
import { createDocumentsSlice } from "./slices/documents.js";
import { createSessionSlice } from "./slices/session.js";
import { createBinderSlice } from "./slices/binder.js";
import { createOutputSlice } from "./slices/output.js";
import { createSearchSlice } from "./slices/search.js";

// ── Notifications (store → shell bridge) ────────────────────────────

/**
 * A notification request raised by a slice (binder undo, replay divergence).
 *
 * The store sits below the shell (spec §7.2), so it cannot import the
 * notification service — instead the app boundary (main.tsx) injects a
 * notifier callback via `setNotifier`, and slices emit this plain-data shape.
 * It is structurally assignable to the shell's `NotificationInput` (§7.5):
 * command-only actions, no callbacks.
 */
export interface StoreNotification {
  severity: "info" | "warning" | "error";
  message: string;
  /** Origin tag, e.g. "binder", "story". */
  source?: string;
  actions?: { label: string; commandId: string; args?: unknown }[];
}

// ── Combined state ──────────────────────────────────────────────────

export interface StudioState
  extends EditorSlice,
    CompileSlice,
    DocumentsSlice,
    SessionSlice,
    BinderSlice,
    OutputSlice,
    SearchSlice {
  // Non-reactive refs — imperative handles that don't trigger re-renders
  _documents: DocumentSessions | null;
  _project: ProjectSession | null;
  /** Injected notifier (see StoreNotification); null until the app binds it. */
  _notify: ((notification: StoreNotification) => void) | null;

  initialize(project: ProjectSession, documents: DocumentSessions): void;
  /** Bind the shell notifier bridge (main.tsx, at bootstrap). */
  setNotifier(notify: (notification: StoreNotification) => void): void;
}

// ── Store factory ───────────────────────────────────────────────────

export const createStudioStore = () =>
  create<StudioState>()((...args) => {
    const [set, get] = args;

    return {
      // Slices
      ...createEditorSlice(...args),
      ...createCompileSlice(...args),
      ...createDocumentsSlice(...args),
      ...createSessionSlice(...args),
      ...createBinderSlice(...args),
      ...createOutputSlice(...args),
      ...createSearchSlice(...args),

      // Non-reactive refs
      _documents: null,
      _project: null,
      _notify: null,

      setNotifier(notify) {
        set({ _notify: notify });
      },

      // Initialization — binds imperative handles and kicks the first compile
      initialize(project, documents) {
        set({ _project: project, _documents: documents });

        // Apply a restored diagnostics setting (Settings document, #93)
        // before the first compile — setExternalCheck called pre-initialize
        // only seeds the state, since no session is bound yet.
        const externalCheck = get().externalCheck;
        if (externalCheck !== "error") {
          project.getSession().setExternalCheck(externalCheck);
        }

        // Trigger an initial compile to populate outline/diagnostics
        documents.triggerCompile();
      },
    };
  });

// ── Typed store instance type ───────────────────────────────────────

export type StudioStore = ReturnType<typeof createStudioStore>;

// ── Re-exports ──────────────────────────────────────────────────────

// Session channel (docs/live-inspector-spec.md §3): the provider seam plus the
// status helpers and divergence message. `LocalSessionProvider` owns the wasm
// runner, the choice-replay loop, and divergence truncation (spec §6.1) — its
// behavior is unit-tested directly.
export {
  sessionCanContinue,
  sessionDegraded,
  statusOfLine,
  REPLAY_DIVERGED_MESSAGE,
  LocalSessionProvider,
  FlowSessionProvider,
  EMPTY_SNAPSHOT,
  ALL_CAPABILITIES,
  DEFAULT_SESSION_ID,
  type SessionStatus,
  type SessionSnapshot,
  type SessionProvider,
  type SessionCapability,
  type SessionEntry,
  type SessionId,
} from "./slices/session.js";

// Problems ordering (canonical sort, unit-testable pure helper) + the
// external-check severity level (Settings document, #93).
export { sortDiagnostics, type ExternalCheckLevel } from "./slices/compile.js";

// Output log (Output tool window, spec §4) — entries + growth cap.
export {
  OUTPUT_LOG_LIMIT,
  type OutputEntry,
  type OutputSource,
} from "./slices/output.js";

// Project-wide search engine (Search tool window, issue #94) — pure,
// unit-testable helpers, plus the result cap (unbounded-growth guard).
export {
  DEFAULT_SEARCH_OPTIONS,
  SEARCH_CONTEXT_BEFORE,
  SEARCH_RESULT_CAP,
  applyReplacements,
  buildSearchPattern,
  escapeRegExp,
  matchLineSegments,
  replacementTextFor,
  searchSources,
  type FileSearchResult,
  type MatchLineSegments,
  type ProjectSearchResult,
  type ReplacementEdit,
  type SearchMatch,
  type SearchPatternResult,
  type SearchQueryOptions,
} from "./search-engine.js";

// Document key/title helpers (shared with the shell's DocumentRefs).
export { docKeyFor, docTitleFor } from "@brink/ink-editor";

export type {
  ElementType,
  LineInfo,
  KeyHint,
  TabTarget,
  DocumentSessions,
  ProjectSession,
} from "./types.js";

export { ElementType as ElementTypeEnum } from "./types.js";
