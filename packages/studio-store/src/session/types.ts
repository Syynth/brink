/**
 * Session channel — the `SessionProvider` seam (docs/live-inspector-spec.md §3).
 *
 * A **session** is the studio's view of a running story VM. Today that VM is
 * the studio's own wasm `StoryRunner` (the {@link SessionProvider} of
 * `kind: "local"`); tomorrow it can be a VM running inside a game (RPG Maker
 * MZ, Bevy), turning the same session-bound views into a live inspector with
 * zero per-view work.
 *
 * The store binds to **one** provider. The provider is the source of truth for
 * the reactive session data and the only thing that can drive the session — and
 * only to the extent its {@link SessionCapability capabilities} allow. The
 * store mirrors {@link SessionSnapshot snapshots} into its reactive slice
 * fields; views are unchanged and never touch the provider.
 */

import type { Choice, DebugState, ProgramModel } from "@brink/wasm-types";
import type { StoreNotification } from "../index.js";
import type { OutputSource } from "../slices/output.js";

// ── Status ──────────────────────────────────────────────────────────

/** Story session status (spec §7.6). */
export type SessionStatus =
  | "none"
  | "running"
  | "awaiting-choice"
  | "done"
  | "ended"
  | "error";

/** Map a runtime line type to the session status it leaves us in. */
export function statusOfLine(type: string): SessionStatus {
  switch (type) {
    case "choices":
      return "awaiting-choice";
    case "end":
      return "ended";
    // `-> DONE` is a turn boundary, not the end — the player can still
    // continue past it (#6), which `sessionCanContinue` reflects.
    case "done":
      return "done";
    default:
      return "running";
  }
}

/**
 * Whether the session can advance another line (drives the player's
 * "Continue" button and the `story.continue` command's `when`). True
 * mid-flow and at a `-> DONE` turn boundary; false at choices/end/error.
 */
export function sessionCanContinue(status: SessionStatus): boolean {
  return status === "running" || status === "done";
}

// ── Snapshot ────────────────────────────────────────────────────────

/**
 * What the session-bound views select from. Push- or pull-sourced, normalized
 * to a single snapshot (spec §3). The store mirrors each snapshot into the
 * reactive slice fields; `prevDebugState` (diff highlighting) is derived by the
 * store from successive snapshots — not the provider's problem (spec §3.1).
 */
export interface SessionSnapshot {
  /** Lifecycle status — "none" means no session exists (placeholder UIs). */
  status: SessionStatus;
  /** Append-only transcript text for the current run (today's `sessionText`). */
  transcript: string[];
  /** Pending choices; non-empty only when status is "awaiting-choice". */
  choices: Choice[];
  /** Name-resolved runtime snapshot (location / globals / call stack / visits). */
  debugState: DebugState | null;
  /** Identity of the RUNNING program — `ProgramModel.checksum` (spec §5). */
  programChecksum: string | null;
  /**
   * Structured model of the compiled program for the Program Explorer.
   * Compile-bound (spec §4): captured when a program loads, survives `stop`.
   */
  programModel: ProgramModel | null;
  /** The compiled program as `.inkt` text (#91). Compile-bound like the model. */
  programInkt: string | null;
}

/** The "no session" snapshot — the store's initial mirror and post-dispose state. */
export const EMPTY_SNAPSHOT: SessionSnapshot = {
  status: "none",
  transcript: [],
  choices: [],
  debugState: null,
  programChecksum: null,
  programModel: null,
  programInkt: null,
};

// ── Capabilities ────────────────────────────────────────────────────

/** Drive verbs. A provider advertises only those it supports (spec §3.2). */
export type SessionCapability =
  | "start"
  | "restart"
  | "stop"
  | "choose"
  | "continue";

/** The full capability set — what the local (wasm) provider advertises. */
export const ALL_CAPABILITIES: ReadonlySet<SessionCapability> = new Set([
  "start",
  "restart",
  "stop",
  "choose",
  "continue",
]);

// ── Provider ────────────────────────────────────────────────────────

/**
 * The session channel (spec §3). The store binds to one provider, mirrors its
 * snapshots into the reactive slice, and drives it only through advertised
 * capabilities. The wasm `StoryRunnerHandle` is an implementation detail of the
 * local provider, not a store field.
 */
export interface SessionProvider {
  readonly kind: "local" | "remote";
  readonly capabilities: ReadonlySet<SessionCapability>;

  /** Current data. The store mirrors this into the reactive slice fields. */
  getSnapshot(): SessionSnapshot;
  /** Subscribe to snapshot changes. Returns an unsubscribe. */
  subscribe(listener: (snapshot: SessionSnapshot) => void): () => void;

  // Drive operations — each callable ONLY if the matching capability is
  // present (the command layer gates first, spec §4), so they need not be
  // defensive. `start` takes program bytes for the local provider; remote
  // providers that can (re)start ignore bytes and act on their own program.
  start?(bytes?: Uint8Array): void;
  restart?(): void;
  stop?(): void;
  choose?(index: number): void;
  continue?(): void;

  dispose(): void;
}

/**
 * Studio-level services a provider reaches without importing the store: the
 * shell notification bridge (spec §7.5) and the Output log. Wired by the
 * session slice at bind time so a provider never depends on `StudioState`.
 */
export interface ProviderCallbacks {
  notify(notification: StoreNotification): void;
  appendOutput(source: OutputSource, text: string): void;
}
