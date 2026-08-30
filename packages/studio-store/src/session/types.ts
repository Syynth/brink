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

import type {
  Breakpoint,
  Choice,
  DebugRunOutcome,
  DebugSourceLocation,
  DebugState,
  ProgramAddress,
  ProgramModel,
  StepMode,
} from "@brink/wasm-types";
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

/**
 * Whether the running program is out of sync with the studio's latest compile
 * (live-inspector degraded mode, spec §5, #181). True only when both
 * checksums are known *and* differ — an unknown checksum (no session, or a
 * failed compile) is **not** degraded, it's simply absent. When degraded,
 * source-position features (graph current-location highlight, visit badges)
 * disable; name-keyed views (the State View) stay live.
 *
 * Locally this never fires (a successful recompile hot-reloads the session, so
 * the running program is always the latest compile). It is reached by a remote
 * provider whose game runs an older program than the studio's source.
 */
export function sessionDegraded(
  programChecksum: string | null,
  compiledChecksum: string | null,
): boolean {
  return (
    programChecksum !== null &&
    compiledChecksum !== null &&
    programChecksum !== compiledChecksum
  );
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
  /**
   * Reveal mode (#3011). `false` — the default — advances ONE line per reveal;
   * `true` runs to the next pause. Mirrored into the slice so the Player's
   * "auto" checkbox reflects provider state rather than keeping its own copy
   * that could drift from what reveals actually do.
   *
   * A provider that can only advance one line at a time (the flow provider —
   * its runner exposes no run-to-pause verb) reports `false` and ignores
   * `setAuto`.
   */
  auto: boolean;
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
  auto: false,
};

// ── Capabilities ────────────────────────────────────────────────────

/** Drive verbs. A provider advertises only those it supports (spec §3.2). */
export type SessionCapability =
  | "start"
  | "restart"
  | "stop"
  | "choose"
  | "continue"
  // Can switch between one-line and run-to-pause reveals (#3011). A provider
  // that only ever advances one line does NOT advertise this — the Player
  // hides the toggle rather than offering a control that does nothing.
  | "auto"
  // D8's debug control bridged through wasm (#3232): pause/step/breakpoints
  // *and* program→source position resolution, bundled as ONE capability —
  // see `DebugSessionProvider` below for why the two aren't split.
  | "debug";

/** The full capability set — what the local (wasm) provider advertises. */
export const ALL_CAPABILITIES: ReadonlySet<SessionCapability> = new Set([
  "start",
  "restart",
  "stop",
  "choose",
  "continue",
  "auto",
  "debug",
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
  /**
   * Set the reveal mode (#3011). Only callable with the `auto` capability.
   * Takes effect on the NEXT reveal — it does not retroactively expand or
   * collapse what is already in the transcript.
   */
  setAuto?(auto: boolean): void;

  dispose(): void;
}

/**
 * The debug-control capability extension (#3232): D8's breakpoints/pause/
 * step (`Story::debug_run`/`debug_step`/`BreakpointSet`, issue #3186) bound
 * onto a session, plus the program→source position resolver D9 (#3187)
 * landed without a `SessionProvider` capability of its own — "hardcoded
 * around it" against a raw wasm handle rather than gated through this
 * interface. The issue folds both into ONE extension rather than adding
 * position-resolution and pause/step/breakpoints as two separate ad-hoc
 * capabilities: a provider that can step a session is exactly the provider
 * whose positions are worth resolving (there is no useful "can resolve
 * positions but can't pause" or "can pause but can't resolve positions"
 * split for a debugger), so a single `"debug"` capability flag and a single
 * interface cover both.
 *
 * Only the local (wasm) provider implements this today; a future remote
 * provider (a game's own VM) may not — `isDebugSessionProvider` is the
 * narrowing guard callers use before touching any method here.
 */
export interface DebugSessionProvider extends SessionProvider {
  /** See `StorySessionHandle.resolveDebugPosition`'s doc (`@brink-lang/web`,
   * D9 #3187) for the full program-identity-gating contract. */
  resolveDebugPosition(containerIdx: number, offset: number): DebugSourceLocation | null;
  /** See `StorySessionHandle.resolveSourceLine`'s doc (W2/#3295): the
   * program address to break on for a 0-based line of `file`, against the
   * RUNNING session's program; `null` = unbound (no executable code, no
   * DebugInfo, or no live session). */
  resolveSourceLine(file: string, line: number): ProgramAddress | null;
  /** See `StorySessionHandle.hasDebugInfo`'s doc (W2/#3295): the honest
   * discriminator between "no DebugInfo section" and "nothing at that
   * position". `false` with no live session. */
  hasDebugInfo(): boolean;
  /** See `StorySessionHandle.debugBreakpointAdd`'s doc. */
  debugBreakpointAdd(containerIdx: number, offset: number, name?: string): number;
  /** See `StorySessionHandle.debugBreakpointRemove`'s doc. */
  debugBreakpointRemove(id: number): boolean;
  /** See `StorySessionHandle.debugBreakpointSetEnabled`'s doc. */
  debugBreakpointSetEnabled(id: number, enabled: boolean): boolean;
  /** See `StorySessionHandle.debugBreakpoints`'s doc. */
  debugBreakpoints(): Breakpoint[];
  /** See `StorySessionHandle.debugRun`'s doc. */
  debugRun(budgetCeiling?: number): DebugRunOutcome;
  /** See `StorySessionHandle.debugStep`'s doc. */
  debugStep(mode: StepMode, budgetCeiling?: number): DebugRunOutcome;
}

/** Narrow a bound `SessionProvider` to `DebugSessionProvider` — checks the
 * `"debug"` capability flag, which the local provider always advertises
 * alongside implementing every method above (kept as two facts — a
 * capability flag the command layer's `when` predicates gate on, and the
 * methods themselves — rather than one, so a provider can be probed for the
 * capability without a `Symbol`/`instanceof` check on an interface). */
export function isDebugSessionProvider(
  provider: SessionProvider,
): provider is DebugSessionProvider {
  return provider.capabilities.has("debug");
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

// ── Multi-session registry (docs/multi-session-spec.md, #182) ────────

/** Stable id for a session in the registry. */
export type SessionId = string;

/**
 * One session in the multi-session registry (spec §3): a labelled
 * {@link SessionProvider}. The session-bound views follow the *active* entry;
 * the registry stays provider-agnostic, so a future remote source can register
 * N flow-backed entries without touching views.
 */
export interface SessionEntry {
  id: SessionId;
  /** Picker label (e.g. "Main", a knot name, a flow/entity name). */
  label: string;
  provider: SessionProvider;
}

/** The always-present primary local session — the auto-started default. */
export const DEFAULT_SESSION_ID = "local:default";
