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
import type {
  CommandRegistry,
  NotificationCenter,
  NotificationHandle,
  NotificationInput,
} from "@brink/studio-shell";
import {
  ElementTypeEnum,
  type SessionStatus,
  type StudioState,
  type StudioStore,
} from "@brink/studio-store";

// ── StudioPublicState (spec §8.2) ────────────────────────────────────

/** Cursor-line element info, by stable element-type name. */
export interface PublicElementInfo {
  /** Element-type name, e.g. "KnotHeader", "NarrativeText", "Choice". */
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
  /** Story session status (spec §7.6); "none" when no session exists. */
  sessionStatus: SessionStatus;
}

/** The store inputs the public state derives from (for change detection). */
interface PublicInputs {
  activeDocKey: string;
  cursor: StudioState["cursor"];
  currentLineInfo: StudioState["currentLineInfo"];
  diagnostics: StudioState["diagnostics"];
  sessionStatus: SessionStatus;
}

function publicInputs(s: StudioState): PublicInputs {
  return {
    activeDocKey: s.activeDocKey,
    cursor: s.cursor,
    currentLineInfo: s.currentLineInfo,
    diagnostics: s.diagnostics,
    sessionStatus: s.sessionStatus,
  };
}

function sameInputs(a: PublicInputs, b: PublicInputs): boolean {
  return (
    a.activeDocKey === b.activeDocKey &&
    a.cursor === b.cursor &&
    a.currentLineInfo === b.currentLineInfo &&
    a.diagnostics === b.diagnostics &&
    a.sessionStatus === b.sessionStatus
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
    element: info === null ? null : { type: ElementTypeEnum[info.type], depth: info.depth },
    diagnostics: s.diagnostics,
    compileStatus: s.diagnostics.errors > 0 ? "errors" : "ok",
    sessionStatus: s.sessionStatus,
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
