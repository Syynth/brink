/**
 * Session slice — the story session as a first-class model
 * (docs/studio-shell-spec.md §7.6, docs/live-inspector-spec.md §3).
 *
 * The session is backed by a {@link SessionProvider}: the store binds to one
 * provider, mirrors its {@link SessionSnapshot snapshots} into the reactive
 * fields the views consume, and drives it only through the provider's
 * capabilities. The wasm `StoryRunner` is no longer a store field — it is an
 * implementation detail of the {@link LocalSessionProvider}. This is the seam
 * that lets a session instead be a VM running inside a game (#127, Phase 8).
 *
 *   none → running → awaiting-choice → (done | ended | error)
 *
 * Lifecycle belongs to commands (`story.start` / `story.restart` /
 * `story.stop` / `story.choose` / `story.continue`, registered at the app
 * boundary) — these slice actions are the implementation those commands call.
 * No view mutates the session directly, and none touch the provider. Player
 * *UI* state (fullscreen, …) stays in the player slice — different lifetime.
 *
 * Choice log + replay-on-recompile (the silent restore with divergence
 * truncation) are a local-provider concern (spec §6.1); the slice only mirrors
 * the resulting reactive state. `prevDebugState` (diff highlighting) is derived
 * here from successive snapshots (spec §3.1).
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { Choice, DebugState, ProgramModel } from "@brink/wasm-types";

import {
  EMPTY_SNAPSHOT,
  type SessionProvider,
  type SessionSnapshot,
  type SessionStatus,
} from "../session/types.js";
import { LocalSessionProvider } from "../session/local-provider.js";

// Re-exported for back-compat: consumers import these from the store root.
export {
  statusOfLine,
  sessionCanContinue,
  type SessionStatus,
  type SessionSnapshot,
  type SessionProvider,
  type SessionCapability,
} from "../session/types.js";
export {
  LocalSessionProvider,
  REPLAY_DIVERGED_MESSAGE,
} from "../session/local-provider.js";

// ── Slice ───────────────────────────────────────────────────────────

export interface SessionSlice {
  /** Lifecycle status — "none" means no session exists (placeholder UIs). */
  sessionStatus: SessionStatus;
  /** Append-only transcript for the current run (cleared on restart). */
  sessionText: string[];
  /** Pending choices; non-empty only when status is "awaiting-choice". */
  sessionChoices: Choice[];

  /**
   * The bound session provider. Non-reactive ref by convention (`_` prefix);
   * views select the mirrored reactive fields, never the provider.
   */
  _provider: SessionProvider | null;
  /** Unsubscribe from the bound provider's snapshot stream. */
  _providerUnsub: (() => void) | null;
  /**
   * Program identity: the bytes the session is running. Kept across
   * `stopSession` so `story.start` can begin a fresh session on the same
   * program even when the latest compile failed (which nulls `storyBytes`).
   */
  _sessionBytes: Uint8Array | null;

  /**
   * Structured, name-resolved runtime snapshot for the State View — refreshed
   * whenever the story advances. `null` until a story is running.
   */
  debugState: DebugState | null;
  /**
   * The snapshot from *before* the latest advance, so the State View can
   * highlight what changed this step. `null` on the first snapshot.
   */
  prevDebugState: DebugState | null;
  /**
   * Structured model of the compiled program for the Program Explorer —
   * captured once when a program loads (static). The Program Explorer is
   * compile-bound (spec §4), so this survives `stopSession`.
   */
  programModel: ProgramModel | null;
  /**
   * The compiled program as `.inkt` text — rendered by the read-only
   * Compiled Output document (#91). Compile-bound like `programModel`.
   */
  programInkt: string | null;
  /**
   * Identity of the running program — `ProgramModel.checksum` (spec §5).
   * Mirrored from the provider snapshot; the basis for degraded mode (#181).
   */
  programChecksum: string | null;

  /**
   * Start (or restart) a session on `bytes`. The single code path for both
   * the `story.start` command and the auto-start on successful compile —
   * replays any persisted choice log to restore position.
   */
  startSession(bytes: Uint8Array): void;
  /** Restart the current session's program from the beginning, fresh log. */
  restartSession(): void;
  /** End the session: free the runner, clear the log, status → "none". */
  stopSession(): void;
  /** Apply a choice by index (status must be "awaiting-choice"). */
  chooseOption(index: number): void;
  /** Reveal the next line from the runtime (or surface choices/end). */
  revealNext(): void;
  /** Bind a session provider, mirroring its snapshots into the slice. */
  _bindProvider(provider: SessionProvider): void;
  /** Refresh the State View from the current provider snapshot (no-op if none). */
  _refreshDebugState(): void;
  /** Free the runner and program inspection state (app teardown). */
  disposeSession(): void;
}

export const createSessionSlice: StateCreator<StudioState, [], [], SessionSlice> = (
  set,
  get,
) => ({
  sessionStatus: "none",
  sessionText: [],
  sessionChoices: [],
  _provider: null,
  _providerUnsub: null,
  _sessionBytes: null,
  debugState: null,
  prevDebugState: null,
  programModel: null,
  programInkt: null,
  programChecksum: null,

  _bindProvider(provider) {
    // Wire studio services into a local provider so it can notify + log
    // without importing the store (spec §3 ProviderCallbacks).
    if (provider instanceof LocalSessionProvider) {
      provider.setCallbacks({
        notify: (n) => get()._notify?.(n),
        appendOutput: (source, text) => get().appendOutput(source, text),
      });
    }
    // Drop any previously-bound provider's subscription (not the provider
    // itself — `startSession` reuses the live provider for hot-reload).
    get()._providerUnsub?.();
    const unsub = provider.subscribe((snap) => mirror(set, snap));
    set({ _provider: provider, _providerUnsub: unsub });
    mirror(set, provider.getSnapshot());
  },

  startSession(bytes) {
    let provider = get()._provider;
    if (!provider) {
      provider = new LocalSessionProvider();
      get()._bindProvider(provider);
    }
    set({ _sessionBytes: bytes });
    provider.start?.(bytes);
  },

  restartSession() {
    const provider = get()._provider;
    // Reset a live runner in place; otherwise (stopped, or a failed load) a
    // restart means a fresh start on the latest available program — preferring
    // the newest compile, falling back to the session's own bytes.
    if (provider instanceof LocalSessionProvider && provider.hasLiveRunner()) {
      provider.restart();
      return;
    }
    const bytes = get().storyBytes ?? get()._sessionBytes;
    if (bytes) get().startSession(bytes);
  },

  stopSession() {
    get()._provider?.stop?.();
  },

  chooseOption(index) {
    get()._provider?.choose?.(index);
  },

  revealNext() {
    get()._provider?.continue?.();
  },

  _refreshDebugState() {
    const provider = get()._provider;
    if (!provider) {
      set({ debugState: null, prevDebugState: null });
      return;
    }
    mirror(set, provider.getSnapshot());
  },

  disposeSession() {
    get()._providerUnsub?.();
    get()._provider?.dispose();
    set({
      _provider: null,
      _providerUnsub: null,
      _sessionBytes: null,
      sessionStatus: "none",
      sessionText: [],
      sessionChoices: [],
      debugState: null,
      prevDebugState: null,
      programModel: null,
      programInkt: null,
      programChecksum: null,
    });
  },
});

// ── Snapshot mirror ─────────────────────────────────────────────────

type SetFn = {
  (partial: Partial<StudioState>): void;
  (updater: (state: StudioState) => Partial<StudioState>): void;
};

/**
 * Mirror a provider snapshot into the reactive slice fields. `prevDebugState`
 * is derived here from the previous snapshot's debug state (spec §3.1): the
 * provider emits one snapshot per logical advance, so carrying the prior
 * `debugState` forward gives the State View its step diff.
 */
function mirror(set: SetFn, snap: SessionSnapshot): void {
  set((s) => ({
    sessionStatus: snap.status,
    sessionText: snap.transcript,
    sessionChoices: snap.choices,
    prevDebugState: s.debugState,
    debugState: snap.debugState,
    programModel: snap.programModel,
    programInkt: snap.programInkt,
    programChecksum: snap.programChecksum,
  }));
}

export { EMPTY_SNAPSHOT };
