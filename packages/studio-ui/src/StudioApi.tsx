/**
 * StudioApi — the curated embedder facade (docs/studio-shell-spec.md §8.2).
 *
 * Host components (spec §8) receive this via React context — never the raw
 * Zustand store, so store internals stay free to change (consumer-first API
 * principle). The facade is four verbs:
 *
 * - `insertText` — at the cursor in the focused editor view, through
 *   DocumentSessions' focused-view path (the same plumbing as element
 *   conversion);
 * - `dispatch` — command dispatch; navigation rides `editor.reveal` (§6.1),
 *   so host panels get it with no extra API surface;
 * - `notify` — the shell notification service (§7.5);
 * - `select`/`subscribe` — reads over `StudioPublicState`, an explicit,
 *   versioned subset of studio state. Anything a host needs that isn't in
 *   it is a deliberate API addition, not a store leak.
 */

import { createContext, useContext, type ReactNode } from "react";
import type { DialogueDialect } from "@brink-lang/editor";
import type {
  CommandRegistry,
  NotificationCenter,
  NotificationHandle,
  NotificationInput,
} from "@brink/studio-shell";
import {
  type SessionStatus,
  type StudioState,
  type StudioStore,
} from "@brink/studio-store";

// ── StudioPublicState (spec §8.2) ────────────────────────────────────

/**
 * Cursor-line element info, by stable element-kind string.
 *
 * BREAKING CHANGE (0.8.0, #368, ruled 2026-07-05): `type` used to be the
 * PascalCase name of a numeric `ElementType` enum member (e.g.
 * `"KnotHeader"`, `"NarrativeText"`, `"Choice"`). `ElementType` is now an
 * open kebab-case string union (`"knot-header"`, `"narrative"`, `"choice"`,
 * …) — CSS classes derive as `brink-<kind>`, the same string. See the
 * PascalCase→kebab mapping table in docs/editor-consumer-guide.md.
 */
export interface PublicElementInfo {
  /** Element kind, e.g. "knot-header", "narrative", "choice". Open string
   *  union — a registered dialect's declared kinds (e.g. "character") flow
   *  through unchanged; new kinds are additive, not breaking. */
  type: string;
  /** Nesting depth (choices/gathers); 1 for top-level. */
  depth: number;
}

/**
 * The explicit, versioned subset of studio state hosts can observe
 * (spec §8.2). Every field is a deliberate exposure; the shape only changes
 * with a `version` bump. Derived internally from the studio store — the
 * store itself is never handed out.
 */
export interface StudioPublicState {
  /** Public-state contract version. Bumped on breaking shape changes. */
  version: 1;
  /** Path of the focused editor's file (e.g. "main.ink"), or null. */
  activeFile: string | null;
  /** 1-based cursor position in the focused editor. */
  cursor: { line: number; col: number };
  /** Element info for the cursor line, or null when unknown. */
  element: PublicElementInfo | null;
  /** Diagnostics summary from the latest compile. */
  diagnostics: { errors: number; warnings: number };
  /** Compile status: "ok" when the latest compile had no errors. */
  compileStatus: "ok" | "errors";
  /** The project's resolved dialogue dialect (#3393) — `brink.toml
   *  [dialogue]` with the preset merged and affix sugar expanded — or
   *  `null` when the project declares none. Additive (no version bump):
   *  what a host's Export writes beside the story as `dialect.json`. */
  projectDialect: DialogueDialect | null;
  /** Story session status (spec §7.6); "none" when no session exists. */
  sessionStatus: SessionStatus;
  /**
   * Count of files whose session content diverges from the last-saved /
   * last-notified baseline (#154). A cheap derived summary — `0` means
   * everything is synced with the host; per-file detail (and contents)
   * live behind the facade (`getDirtyFiles` / `getFiles`), never in state.
   * Additive field — the version stays 1 per the versioning policy.
   */
  dirtyFiles: number;
}

/** The store inputs the public state derives from (for change detection). */
interface PublicInputs {
  activeDocKey: string;
  cursor: StudioState["cursor"];
  currentLineInfo: StudioState["currentLineInfo"];
  diagnostics: StudioState["diagnostics"];
  sessionStatus: SessionStatus;
  dirtyFiles: number;
}

function publicInputs(s: StudioState): PublicInputs {
  return {
    activeDocKey: s.activeDocKey,
    cursor: s.cursor,
    currentLineInfo: s.currentLineInfo,
    diagnostics: s.diagnostics,
    sessionStatus: s.sessionStatus,
    dirtyFiles: s.dirtyFiles,
  };
}

function sameInputs(a: PublicInputs, b: PublicInputs): boolean {
  return (
    a.activeDocKey === b.activeDocKey &&
    a.cursor === b.cursor &&
    a.currentLineInfo === b.currentLineInfo &&
    a.diagnostics === b.diagnostics &&
    a.sessionStatus === b.sessionStatus &&
    a.dirtyFiles === b.dirtyFiles
  );
}

/** Derive the public state from the full store state (pure). */
export function derivePublicState(s: StudioState): StudioPublicState {
  // activeDocKey is a document key ("main.ink" or "main.ink::knot"); the
  // public field is the file path.
  const sep = s.activeDocKey.indexOf("::");
  const activeFile =
    s.activeDocKey === "" ? null : sep < 0 ? s.activeDocKey : s.activeDocKey.slice(0, sep);
  const info = s.currentLineInfo;
  return {
    version: 1,
    activeFile,
    cursor: s.cursor,
    // `info.type` is already the stable kebab-case kind string (#368) — no
    // enum-name lookup needed (the old PascalCase reverse-mapping this
    // replaced only worked because `ElementType` used to be a numeric enum).
    element: info === null ? null : { type: info.type, depth: info.depth },
    diagnostics: s.diagnostics,
    compileStatus: s.diagnostics.errors > 0 ? "errors" : "ok",
    projectDialect: s.projectDialect,
    sessionStatus: s.sessionStatus,
    dirtyFiles: s.dirtyFiles,
  };
}

// ── StudioApi ────────────────────────────────────────────────────────

/** The curated host facade (spec §8.2). */
export interface StudioApi {
  /**
   * Insert text at the cursor in the focused editor view (replacing any
   * selection). No-op when no editor is focused.
   */
  insertText(text: string): void;
  /**
   * Dispatch a command by id (§6). Returns true if it ran. Navigation goes
   * through `dispatch("editor.reveal", location)` (§6.1).
   */
  dispatch(commandId: string, args?: unknown): boolean;
  /** Raise a notification (§7.5). The handle can dismiss/amend it later. */
  notify(n: NotificationInput): NotificationHandle;
  /** Read a value from the current public state. */
  select<T>(sel: (s: StudioPublicState) => T): T;
  /**
   * Subscribe to a selected value; `cb` fires when it changes (Object.is).
   * Returns an unsubscribe function.
   */
  subscribe<T>(sel: (s: StudioPublicState) => T, cb: (value: T) => void): () => void;
  /**
   * Snapshot of every project file's current session content, by path
   * (#154 pull egress). Contents deliberately do NOT live in
   * `StudioPublicState` — they are big and change per keystroke; pull them
   * on demand (or receive pushes via the `onFilesChanged` mount option).
   */
  getFiles(): Record<string, string>;
  /**
   * Paths whose session content diverges from the last-saved/last-notified
   * baseline — the per-file detail behind `StudioPublicState.dirtyFiles`.
   */
  getDirtyFiles(): string[];
  /**
   * Paths deleted externally while the studio keeps an editor buffer for
   * them (issue #2371, "External deletion of an open file: keep the view,
   * mark orphaned") — never auto-closed, always dirty, cleared by a save or
   * by the file reappearing on disk. A host renders this as a tab badge
   * ("deleted on disk") or strikethrough; not part of `StudioPublicState`
   * for the same reason `getDirtyFiles` isn't — pull on demand.
   */
  getOrphanedFiles(): string[];
  /**
   * The latest successful compile's story bytes (issue #2391, "Export Story
   * (.inkb)"), or `null` when the latest compile failed (or none has run
   * yet). Same pull-on-demand shape as `getFiles`/`getDirtyFiles` — bytes
   * are big and change on every compile, so they stay out of
   * `StudioPublicState`. A host drives `dispatch("compile.run")` first (the
   * same surface the Player's Run button uses) to get a fresh compile, then
   * reads this to get the artifact.
   */
  getStoryBytes(): Uint8Array | null;
}

export interface StudioApiDeps {
  store: StudioStore;
  commands: CommandRegistry;
  notifications: NotificationCenter;
}

/**
 * Build the facade over the app's store, command registry, and notification
 * center (main.tsx bootstrap). The derived public state is cached and only
 * recomputed when one of its store inputs changes, so selector results over
 * it are reference-stable between relevant changes.
 */
export function createStudioApi({ store, commands, notifications }: StudioApiDeps): StudioApi {
  let cachedInputs: PublicInputs | null = null;
  let cachedState: StudioPublicState | null = null;

  const publicState = (): StudioPublicState => {
    const state = store.getState();
    const inputs = publicInputs(state);
    if (cachedState === null || cachedInputs === null || !sameInputs(cachedInputs, inputs)) {
      cachedInputs = inputs;
      cachedState = derivePublicState(state);
    }
    return cachedState;
  };

  return {
    insertText(text) {
      store.getState()._documents?.insertAtCursor(text);
    },
    dispatch(commandId, args) {
      return commands.dispatch(commandId, args);
    },
    notify(n) {
      return notifications.notify(n);
    },
    select(sel) {
      return sel(publicState());
    },
    subscribe(sel, cb) {
      let previous = sel(publicState());
      return store.subscribe(() => {
        const next = sel(publicState());
        if (!Object.is(previous, next)) {
          previous = next;
          cb(next);
        }
      });
    },
    getFiles() {
      return store.getState()._project?.getFiles() ?? {};
    },
    getDirtyFiles() {
      return store.getState()._project?.dirtyPaths() ?? [];
    },
    getOrphanedFiles() {
      return store.getState()._project?.orphanedPaths() ?? [];
    },
    getStoryBytes() {
      return store.getState().storyBytes;
    },
  };
}

// ── React context ────────────────────────────────────────────────────

const StudioApiContext = createContext<StudioApi | null>(null);

/** Provides the facade to host components (mounted around the app tree). */
export function StudioApiProvider({ api, children }: { api: StudioApi; children: ReactNode }) {
  return <StudioApiContext.Provider value={api}>{children}</StudioApiContext.Provider>;
}

/** The host components' door to the studio (spec §8.2). */
export function useStudioApi(): StudioApi {
  const api = useContext(StudioApiContext);
  if (api === null) {
    throw new Error("useStudioApi() requires a <StudioApiProvider> ancestor");
  }
  return api;
}
