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
  DebugLine,
  DebugSourceLocation,
  DebugState,
  LoadReport,
  ProgramAddress,
  ProgramModel,
  ProjectSource,
  SaveState,
  SpeculationResult,
  StructuralTranscript,
  SourceLocation,
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
/** One transcript row (W7/#3300 — the Player's line-row model).
 *
 * `kind`: `"line"` = story output (tags/provenance may ride along);
 * `"marker"` = the chosen-choice echo (`> text`); `"notice"` = a
 * studio-side message (load/runtime errors, replay notices). */
export interface TranscriptLine {
  text: string;
  kind: "line" | "marker" | "notice";
  /** Per-line tags (`OutputLine.tags`) — the Player's tags toggle. */
  tags: string[];
  /** Where the line came from in the author's source (transcript
   * provenance, spec §F9) — byte range, convert before editor use. On a
   * `marker` row this is the CHOICE's own source (#3435). */
  source?: SourceLocation;
  /** On a choice echo (`kind: "marker"`): how the choice was written —
   *  `*` once-only or `+` sticky (#3435). The Player draws the glyph. */
  choiceKind?: "once" | "sticky";
  /** The knot / `knot.stitch` this line came from (#3389 follow-up):
   *  the runtime's `currentPath()` read just before the continue that
   *  delivered it. Absent on restored history and on the first line of a
   *  run from the root. The Player resets speaker runs when it changes. */
  path?: string;
}

/** A transcript line's source, in editor terms (W7/#3300): 0-based
 * line plus a UTF-16 code-unit span — converted from
 * `TranscriptLine.source`'s byte range by the app boundary's registered
 * resolver (`setSourceByteResolver`). */
export interface ProvenancePoint {
  /** 0-based line the range starts on. */
  line: number;
  /** 0-based line the range ends on (≥ `line`): a glue-joined or
   *  cue + aside + dialogue line spans several source lines. */
  endLine: number;
  start: number;
  end: number;
}

/** Story-output row helper — the shape both drive roads append. */
export function transcriptLine(
  text: string,
  tags: string[] = [],
  source?: SourceLocation,
  path?: string | null,
): TranscriptLine {
  return {
    text,
    kind: "line",
    tags,
    ...(source ? { source } : {}),
    ...(path ? { path } : {}),
  };
}

/** Studio-side message row helper (errors, notices). */
export function transcriptNotice(text: string): TranscriptLine {
  return { text, kind: "notice", tags: [] };
}

export interface SessionSnapshot {
  /** Lifecycle status — "none" means no session exists (placeholder UIs). */
  status: SessionStatus;
  /** Append-only transcript for the current run — structured since
   * W7/#3300 (line rows + tags + provenance). The slice derives the
   * text-only `sessionText` mirror from it. */
  transcript: TranscriptLine[];
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
   * Paused by the debugger (W5/#3298): a breakpoint/watchpoint hit, an
   * explicit step, or the pause verb. Orthogonal to `status` — a paused
   * session is still `"running"` in lifecycle terms; this flag is what
   * enables the step controls and the "Paused — file:line" chip. Cleared
   * when a debug run resumes free-running or the session stops/restarts.
   */
  paused: boolean;
  /**
   * The most recent debug-advance outcome (W5/#3298), whether it came from
   * an explicit step verb or the unified Player advance routing through
   * the debug loop (armed breakpoints). `null` before any debug-driven
   * advance this session. The Player's status chip reads the stop reason
   * from here.
   */
  debugOutcome: DebugRunOutcome | null;
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
  /** Unix ms of the last successful hot-reload (W15/#3308) — the chip's
   * brief "reloaded" affirmation; `null` before any reload. */
  reloadedAt: number | null;
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
  paused: false,
  debugOutcome: null,
  auto: false,
  reloadedAt: null,
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
  | "debug"
  // Peek (ruled 2026-09-03): can fork the live story at its exact position
  // and run one continue call on the fork — the Player's hover forecasts.
  | "peek";

/** The full capability set — what the local (wasm) provider advertises. */
export const ALL_CAPABILITIES: ReadonlySet<SessionCapability> = new Set([
  "start",
  "restart",
  "stop",
  "choose",
  "continue",
  "auto",
  "debug",
  "peek",
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
  /** Configure the paced auto-reveal cadence (W7/#3300 F13, RULED):
   * with auto on and a positive delay, a reveal advances the run one
   * line at a time in rapid succession; 0 = one batch. Optional — a
   * provider without a paced pump ignores the setting. */
  setPacedReveal?(delayMs: number): void;
  /** One-shot fast-forward (RULED 2026-08-30): run to the next stop,
   * honoring the paced setting; nothing sticky. Optional. */
  continueMaximally?(): void;
  /** Capture the durable game state (W14/#3307); `null` without a live
   * session. Optional — observe-only providers skip checkpoints. */
  saveState?(): SaveState | null;
  /** Export the STRUCTURAL transcript (RULED 2026-08-30): the runtime's
   * part stream as human-readable JSON, re-renderable against any later
   * compile. `null` without a live session. Optional. */
  exportTranscript?(): StructuralTranscript | null;
  /** Load a checkpoint and divert to its recorded knot (W14/#3307) —
   * see the local provider's doc; returns the `LoadReport` (surfaced,
   * never silent) or `null` without a live session. `transcript` is the
   * story-so-far in STRUCTURAL form (RULED 2026-08-30): re-rendered
   * against the session's CURRENT program on restore, so an edited
   * line's restored row shows the edited text. */
  loadCheckpoint?(
    state: SaveState,
    knotPath: string | null,
    verb?: "Loaded" | "Reloaded",
    transcript?: StructuralTranscript | null,
  ): LoadReport | null;

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
  /** See `StorySessionHandle.resolveDebugLine`'s doc (W6/#3299): the
   * `file:line` (0-based) + covering byte range of a position, or
   * `null`. */
  resolveDebugLine(containerIdx: number, offset: number): DebugLine | null;
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
  /** Step to the next source line (#3264, W5/#3298) — the STATEMENT-tier
   * step the transport's Step Over/Into/Out drive (2026-08-30 Continue
   * ruling: the author tier is `debugRunToLine`); bounded by armed
   * breakpoints. Leaves the session paused (except at choices/terminal). */
  debugStepLine(mode: StepMode, budgetCeiling?: number): DebugRunOutcome;
  /** Run until the next CONTENT line is delivered (2026-08-30 Continue
   * ruling — the granularity ladder's top tier), or a breakpoint/choices/
   * terminal stop comes first. The crossed line is IN the outcome's
   * `lines` (no one-advance lag, #3321). Needs no debug line info. */
  debugRunToLine(budgetCeiling?: number): DebugRunOutcome;
  /** Break-on-write data breakpoints (W18/#3311, RULED): arm a watched
   * global by author name — a write stops the run/Continue tiers with
   * the watchpoint named in the stop reason. `false` = unknown global or
   * already armed. */
  debugWatchpointAdd(name: string): boolean;
  /** Disarm; `false` = wasn't armed. */
  debugWatchpointRemove(name: string): boolean;
  /** Armed data breakpoints, in arm order. */
  debugWatchpoints(): string[];
  /** Watch evaluation (W17/#3310, spec §F18): evaluate an expression or
   * divert/content fragment against the session's CURRENT durable state,
   * side-effect-proof (a discard-on-drop speculation over a scratch
   * runner — nothing it does touches the session). `null` = no live
   * session to evaluate against. Optional — only the local provider
   * implements it. */
  evaluateWatch?(
    source: string,
    opts?: { projectSource?: ProjectSource; budget?: { steps?: number; lines?: number } },
  ): Promise<SpeculationResult> | null;
  /** Live value editing (W16/#3309, RULED: paused-only, scalars only) —
   * a global. `false` = refused with nothing written. */
  editGlobal(name: string, input: string): boolean;
  /** Live value editing — a frame local (snapshot's innermost-first
   * frame index + slot). Same refusal contract. */
  editTemp(frameIdx: number, slot: number, input: string): boolean;
  /** The pause verb (W5/#3298, ruled: pause/resume is first-class): the
   * session enters the paused state at its current boundary — Continue
   * (`debugRunToLine`) delivers the next content line and resumes play;
   * the statement steps advance and stay paused. */
  pause(): void;
}

/** What one forecast continue call would produce (peek, ruled
 *  2026-09-03): the source of the line it delivers — or of every choice it
 *  presents — and the knot/stitch the fork was in when it ran, read
 *  before the advance exactly as a transcript row's `path` is. */
export interface PeekResult {
  sources: SourceLocation[];
  path: string | null;
}

/** A provider that can forecast (peek, ruled 2026-09-03): fork the live
 *  story at its exact position, run ONE continue call on the fork —
 *  never the auto run, externals sandboxed — and report what it hit. The
 *  fork is discarded; the live session never moves. */
export interface PeekSessionProvider extends SessionProvider {
  /** What pressing Continue would deliver next; `null` when it cannot be
   *  pressed (a choice point, an ended story, no live session). */
  peekContinue(): PeekResult | null;
  /** What picking choice `index` would deliver first; `null` unless the
   *  session waits on that choice. */
  peekChoice(index: number): PeekResult | null;
}

/** Narrow a bound `SessionProvider` to `PeekSessionProvider` (the `"peek"`
 *  capability flag, same two-facts posture as `isDebugSessionProvider`). */
export function isPeekSessionProvider(
  provider: SessionProvider,
): provider is PeekSessionProvider {
  return provider.capabilities.has("peek");
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
