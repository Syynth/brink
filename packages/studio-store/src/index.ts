/**
 * @brink/studio-store — Zustand store for brink-studio React migration.
 *
 * Combines domain slices (editor, compile, tabs, session, player, binder) into a
 * single store. Non-reactive refs (prefixed with _) hold imperative handles
 * that should not trigger re-renders.
 */

import { create } from "zustand";
import { isPerfEnabled, perfRecord } from "@brink-lang/editor";

import type { EditorSlice } from "./slices/editor.js";
import type { CompileSlice } from "./slices/compile.js";
import type { DocumentsSlice } from "./slices/documents.js";
import type { SessionSlice } from "./slices/session.js";
import type { BinderSlice } from "./slices/binder.js";
import type { OutputSlice } from "./slices/output.js";
import type { SearchSlice } from "./slices/search.js";
import type { ProblemsSlice } from "./slices/problems.js";
import type { SymbolMenuSlice } from "./slices/symbol-menu.js";
import type { ConflictSlice } from "./slices/conflict.js";
import type { DocumentSessions, ProjectSession } from "./types.js";

import { createEditorSlice } from "./slices/editor.js";
import { createCompileSlice } from "./slices/compile.js";
import { createDocumentsSlice } from "./slices/documents.js";
import { createSessionSlice } from "./slices/session.js";
import { createBinderSlice } from "./slices/binder.js";
import { createOutputSlice } from "./slices/output.js";
import { createSearchSlice } from "./slices/search.js";
import { createProblemsSlice } from "./slices/problems.js";
import { createSymbolMenuSlice } from "./slices/symbol-menu.js";
import { createConflictSlice } from "./slices/conflict.js";

// ── Notifications (store → shell bridge) ────────────────────────────

/**
 * A notification request raised by a slice (binder undo, replay divergence),
 * or — since #2528 — by a `studio-ui` action module holding the injected
 * notifier (`performSymbolRename`'s refusal path). Producers are not limited
 * to slices; what is fixed is the SHAPE and the store→shell bridge below.
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
    SearchSlice,
    ProblemsSlice,
    SymbolMenuSlice,
    ConflictSlice {
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
  create<StudioState>()((rawSet, get, api) => {
    // Store-write timing (measure-first ruling, 2026-08-24): every `set()`
    // synchronously re-runs every mounted selector, so its duration IS the
    // subscription sweep cost. The span is tagged with the partial's first
    // key (`store.set.cursor`, `store.set.diagnosticsList`, …) so a report
    // attributes sweeps to the field that triggered them. Inert single
    // branch while the probe is disabled — the production state.
    const timedSet: typeof rawSet = (partial, replace) => {
      if (!isPerfEnabled()) {
        (rawSet as (p: unknown, r?: unknown) => void)(partial, replace);
        return;
      }
      const tag =
        typeof partial === "function"
          ? "store.set.fn"
          : `store.set.${Object.keys(partial as object)[0] ?? "empty"}`;
      const t0 = performance.now();
      try {
        (rawSet as (p: unknown, r?: unknown) => void)(partial, replace);
      } finally {
        perfRecord(tag, t0, performance.now() - t0);
      }
    };
    // Slices created below close over the timed variant; external callers
    // going through `api.setState`/`useStore.setState` get it too.
    const origSetState = api.setState;
    api.setState = ((partial, replace) => {
      if (!isPerfEnabled()) {
        (origSetState as (p: unknown, r?: unknown) => void)(partial, replace);
        return;
      }
      const tag =
        typeof partial === "function"
          ? "store.set.fn"
          : `store.set.${Object.keys(partial as object)[0] ?? "empty"}`;
      const t0 = performance.now();
      try {
        (origSetState as (p: unknown, r?: unknown) => void)(partial, replace);
      } finally {
        perfRecord(tag, t0, performance.now() - t0);
      }
    }) as typeof api.setState;
    const args = [timedSet, get, api] as const;
    const set = timedSet;

    return {
      // Slices
      ...createEditorSlice(...args),
      ...createCompileSlice(...args),
      ...createDocumentsSlice(...args),
      ...createSessionSlice(...args),
      ...createBinderSlice(...args),
      ...createOutputSlice(...args),
      ...createSearchSlice(...args),
      ...createProblemsSlice(...args),
      ...createSymbolMenuSlice(...args),
      ...createConflictSlice(...args),

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
// Out-of-scope banner helpers (#3017) — exported for the banner's tests;
// the production caller is the compile slice's includeInEntry action.
export { insertIncludeLine, relativeIncludePath } from "./include-insert.js";
// Comment-preserving structured edits for brink.toml (#3015) — used by
// the studio's config form panel.
export {
  getTomlBool,
  getTomlString,
  setTomlBool,
  setTomlString,
  tomlTableKeys,
} from "./toml-edit.js";
// The .binder.json order sidecar's pure model (#3038).
export {
  BINDER_SIDECAR_PATH,
  EMPTY_BINDER_ORDER,
  addFolder,
  applyReorder,
  isFolderId,
  orderChildIds,
  parseBinderOrder,
  rekeyBinderOrder,
  removeFromBinderOrder,
  serializeBinderOrder,
  type BinderOrder,
} from "./binder-order.js";

// Shared knot/stitch context-menu transport (#186 follow-up).
export type {
  EditorTextMenuRequest,
  SymbolMenuRequest,
  SymbolRenameRequest,
} from "./slices/symbol-menu.js";

// Output log (Output tool window, spec §4) — entries + growth cap.
export {
  OUTPUT_LOG_LIMIT,
  type OutputEntry,
  type OutputSource,
} from "./slices/output.js";

// Project-wide search engine (Search tool window, issue #94) — pure,
// unit-testable helpers, plus the result cap (unbounded-growth guard).
// The model now lives in @brink-lang/editor (issue #322, framework-agnostic);
// re-exported here so studio-store stays the single import surface.
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
  buildResultsRows,
  mapRowEditToSource,
  SearchResultsBuffer,
  DEFAULT_COMMIT_DELAY_MS,
  SearchCardBuffer,
  cardLineSegments,
  type FileSearchResult,
  type MatchLineSegments,
  type ProjectSearchResult,
  type ReplacementEdit,
  type ResultRow,
  type ResultsBufferModel,
  type SearchMatch,
  type SearchPatternResult,
  type SearchQueryOptions,
  type SearchResultsBufferOptions,
  type CardLineSegment,
  type SearchCardBufferOptions,
  type SearchCardHighlight,
  type SearchCardModel,
} from "@brink-lang/editor";

// Frozen search snapshot (docs/search-results-cards-spec.md, PR B): the
// pure model — capture, edit-mapping, staleness — plus the context-lines
// knob's defaults. The slice owns when to capture/remap.
export {
  DEFAULT_SEARCH_CONTEXT_LINES,
  MAX_SEARCH_CONTEXT_LINES,
  attachReferenceKinds,
  captureSnapshot,
  cardSlice,
  clampContextLines,
  diffSources,
  lineInfoAt,
  mapSpan,
  remapSnapshot,
  type CardSlice,
  type SearchContextLines,
  type SearchSnapshot,
  type SnapshotAnchor,
  type SnapshotFile,
  type SnapshotMatch,
  type SnapshotOrigin,
  type SourceDiff,
} from "./search-snapshot.js";

// Document key/title helpers (shared with the shell's DocumentRefs).
export { docKeyFor, docTitleFor } from "@brink-lang/editor";

export type {
  KeyHint,
  TabTarget,
  DocumentSessions,
  ProjectSession,
  FileConflict,
} from "./types.js";

// ElementType/LineInfo (#368): the duplicate enum that used to live in
// types.ts is deleted — both now come from the real @brink-lang/editor
// module. `ElementType` is re-exported as `ElementTypeEnum` for call-site
// compatibility (existing consumers import the value under that name).
export type { LineInfo, DialectGeometry } from "@brink-lang/editor";
export { ElementType as ElementTypeEnum } from "@brink-lang/editor";

// External-conflict merge state (#320, Track V): the conflict slice plus the
// deterministic sorted-paths helper for badging conflicted files.
export type {
  ProblemSeverityBucket,
  ProblemsPrefs,
  ProblemsSlice,
} from "./slices/problems.js";
export {
  PROBLEMS_STORAGE_KEY,
  loadProblemsPrefs,
  saveProblemsPrefs,
} from "./slices/problems.js";
export { conflictPaths } from "./slices/conflict.js";
export type { ConflictSlice } from "./slices/conflict.js";
