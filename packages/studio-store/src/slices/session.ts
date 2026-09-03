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
import type {
  Choice,
  DebugState,
  LinesTable,
  ProgramModel,
  SizeReport,
  SourceLocation,
} from "@brink/wasm-types";
import type { ExternalValue } from "@brink-lang/web";

import {
  ALL_CAPABILITIES,
  DEFAULT_SESSION_ID,
  EMPTY_SNAPSHOT,
  isDebugSessionProvider,
  type SessionCapability,
  type SessionEntry,
  type SessionId,
  type SessionProvider,
  type SessionSnapshot,
  type SessionStatus,
} from "../session/types.js";
import { LocalSessionProvider } from "../session/local-provider.js";
import { isPeekSessionProvider } from "../session/types.js";
import type { ProvenancePoint, TranscriptLine } from "../session/types.js";
export type { ProvenancePoint, TranscriptLine } from "../session/types.js";

// Re-exported for back-compat: consumers import these from the store root.
export {
  statusOfLine,
  sessionCanContinue,
  sessionDegraded,
  ALL_CAPABILITIES,
  DEFAULT_SESSION_ID,
  type SessionStatus,
  type SessionSnapshot,
  type SessionProvider,
  type SessionCapability,
  type SessionEntry,
  type SessionId,
} from "../session/types.js";
export {
  LocalSessionProvider,
  REPLAY_DIVERGED_MESSAGE,
} from "../session/local-provider.js";
export { FlowSessionProvider } from "../session/flow-provider.js";

// ── Slice ───────────────────────────────────────────────────────────

/** A CSS `font-family` list, or "" when it is not one: family names, quotes,
 *  commas, hyphens and spaces only — never a `;`, `{`, `}` or `url(` that
 *  could turn a setting into a stylesheet. Exported for the settings UI. */
export function sanitizeFontFamily(input: string): string {
  const trimmed = input.trim();
  if (trimmed === "") return "";
  return /^[A-Za-z0-9 _'",\-]+$/.test(trimmed) ? trimmed : "";
}

export interface SessionSlice {
  /** Lifecycle status — "none" means no session exists (placeholder UIs). */
  sessionStatus: SessionStatus;
  /**
   * Run generation: bumps on every start, restart and session switch. The
   * Player keys its timeline on it so a restart REMOUNTS the transcript —
   * the first line fades back in exactly as it does after Stop → Run
   * (feedback 2026-09-02), instead of the old rows being reused in place.
   */
  sessionRun: number;
  /** Append-only transcript TEXT for the current run (cleared on
   * restart) — derived from `sessionLines`; kept as the stable
   * text-only view for consumers that only need strings. */
  sessionText: string[];
  /** The structured transcript (W7/#3300): line rows with tags and
   * source provenance — what the rebuilt Player renders. */
  sessionLines: TranscriptLine[];
  /**
   * Paced auto-reveal cadence in ms (W7/#3300 F13, RULED — the App
   * setting "Auto reveal: paced / all at once"): with auto on, a reveal
   * delivers the run one line at a time at this cadence; 0 = one batch.
   * Persisted by the app boundary; applied to the provider at bind.
   */
  sessionPacedMs: number;
  /** Set the paced cadence (Settings) — pushes through to the provider. */
  setSessionPaced(delayMs: number): void;
  /** Follow in editor (#3437, ruled 2026-09-02 "it should follow the player
   *  much more closely"): while on, every line the Player reveals scrolls
   *  the editor to its source and bands it. App-scope, persisted with the
   *  Player settings. */
  followInEditor: boolean;
  setFollowInEditor(on: boolean): void;
  /** Follow pauses when the author edits the followed document; Run /
   *  Restart or flipping the toggle resumes it. */
  followPaused: boolean;
  setFollowPaused(paused: boolean): void;
  /** The source of the transcript row under the pointer (#3437): the
   *  editor bands it with the hover band, distinct from the follow band. */
  sessionHoverSource: SourceLocation | null;
  /** Peek (ruled 2026-09-03): the sources one forecast continue call
   *  would hit — set while Continue or a choice card is hovered, cleared
   *  on leave and whenever the transcript moves (the forecast is for the
   *  state it was taken in). `null` = no forecast showing. */
  sessionPeek: SourceLocation[] | null;
  /** Forecast what pressing Continue delivers next (no-op without the
   *  `peek` capability, or when Continue cannot be pressed). */
  peekContinue(): void;
  /** Forecast what picking choice `index` delivers first. */
  peekChoice(index: number): void;
  /** Drop the forecast. */
  clearPeek(): void;
  setSessionHoverSource(source: SourceLocation | null): void;
  /** Player reading knobs (#3438, app scope, persisted with the Player
   *  settings). Empty string / 0 = the theme's default; the CSS variable
   *  stays unset so player.css's fallback applies. */
  playerFontFamily: string;
  setPlayerFontFamily(family: string): void;
  /** Line spacing as a multiple of the font size, ×10 (12–22); 0 = default. */
  playerLineHeight: number;
  setPlayerLineHeight(tenths: number): void;
  /** Measure in `ch` (48–96); 0 = default. */
  playerMeasure: number;
  setPlayerMeasure(ch: number): void;
  /** Reading aids (#3438). */
  showProvenance: boolean;
  setShowProvenance(on: boolean): void;
  showChoiceMarkers: boolean;
  setShowChoiceMarkers(on: boolean): void;
  /** The host's font list (#3439 — the desktop app enumerates the machine's
   *  fonts); `null` = the curated list. */
  hostFonts: readonly string[] | null;
  setHostFonts(fonts: readonly string[] | null): void;
  /**
   * The Player's prose size in px (W13/#3306, RULED — the reading
   * surface's size is not the UI's size, the `--bs-editor-font-size`
   * precedent). 0 = follow the app type scale (`--bs-font-prose`).
   * Mirrored onto the root as `--bs-player-font-size`; persisted by the
   * app boundary alongside the paced-reveal setting.
   */
  playerFontSize: number;
  /** Mirror of the provider's last hot-reload timestamp (W15/#3308). */
  sessionReloadedAt: number | null;
  /** Set the Player prose size (Settings); clamped, 0 resets to scale. */
  setPlayerFontSize(px: number): void;
  /** One-shot fast-forward (RULED 2026-08-30) — run to the next stop. */
  revealMaximally(): void;
  /**
   * Byte-range → editor terms converter for transcript provenance
   * (W7/#3300): `TranscriptLine.source` carries UTF-8 BYTE offsets in
   * the compiled file; the Player needs a 0-based line (hover chip) and
   * a UTF-16 span (the reveal). Registered by the app boundary, which
   * can read file text; `null` until then (provenance affordances hide).
   */
  _resolveSourceBytes:
    | ((file: string, byteStart: number, byteEnd: number) => ProvenancePoint | null)
    | null;
  /** Register the provenance converter (app boundary). */
  setSourceByteResolver(
    resolver: (file: string, byteStart: number, byteEnd: number) => ProvenancePoint | null,
  ): void;
  /** Pending choices; non-empty only when status is "awaiting-choice". */
  sessionChoices: Choice[];
  /**
   * Reveal mode (#3011). `false` — the default — reveals one line at a time;
   * `true` runs to the next pause. Mirrored from the provider snapshot, so the
   * Player's checkbox shows what reveals actually do rather than a separate
   * copy that can drift.
   */
  sessionAuto: boolean;
  /** Paused by the debugger (W5/#3298) — mirrors `SessionSnapshot.paused`.
   * What enables the transport's step controls and the paused chip. */
  sessionPaused: boolean;

  /**
   * The session registry (docs/multi-session-spec.md, #182) — ordered, the
   * primary "local:default" first. The single-session studio is "exactly one
   * entry, always active"; a source (local now, remote later) populates it.
   */
  sessions: SessionEntry[];
  /** The session the views follow; `null` only before the first session. */
  activeSessionId: SessionId | null;

  /**
   * The *active* session's provider. Non-reactive ref by convention
   * (`_` prefix); views select the mirrored reactive fields, never the
   * provider. Kept in sync with `activeSessionId`.
   */
  _provider: SessionProvider | null;
  /** Unsubscribe from the active provider's snapshot stream. */
  _providerUnsub: (() => void) | null;
  /** Monotonic id source for secondary local sessions (deterministic). */
  _sessionSeq: number;
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
   * The compiled lines table (#3339) — per-scope line entries, captured
   * runner-free from the same compile as `programModel`. The Program
   * Explorer joins it against the knot tree for per-knot line counts, and
   * the Line tables view renders it whole.
   */
  programLines: LinesTable | null;
  /** The `.inkb` size report (#3339 Size view) — compile-bound like the
   *  other program products. */
  programSize: SizeReport | null;
  /**
   * Identity of the running program — `ProgramModel.checksum` (spec §5).
   * Mirrored from the provider snapshot; the basis for degraded mode (#181).
   */
  programChecksum: string | null;
  /**
   * Drive verbs the bound provider advertises (spec §3.2/§4). The command
   * layer ANDs these into the `story.*` `when` predicates, so an observe-only
   * provider makes the drive commands vanish from the palette/strips/headers
   * with no per-view branching. Defaults to the full local set — the studio
   * can always start a *local* session until a narrower (remote) provider
   * binds; a remote provider replaces this with whatever the game permits.
   */
  capabilities: ReadonlySet<SessionCapability>;

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
  /** Pause the running session at its current boundary (W5/#3298). */
  pauseSession(): void;
  /**
   * Set the reveal mode (#3011). No-op on a provider without the `auto`
   * capability, which is also why the Player hides the toggle for those.
   */
  setSessionAuto(auto: boolean): void;

  /**
   * Open a new local session (#182) — an independent runner with isolated
   * globals, started at the program root or at `path` ("play from here").
   * Registered alongside the others and made active. No-op without a program.
   */
  openSession(opts?: { label?: string; path?: string; args?: ExternalValue[] }): void;
  /**
   * Open a new shared-context flow (#200) — a concurrent flow of the **primary**
   * session's story that **shares** its globals / visit counts / rng, started at
   * the root or `path`. Registered + made active. No-op without a live primary.
   */
  openFlow(opts?: { label?: string; path?: string }): void;
  /** Close a session by id. The primary (`local:default`) cannot be closed. */
  closeSession(id: SessionId): void;
  /** Make `id` the active session — repoints every session-bound view. */
  setActiveSession(id: SessionId): void;

  /** Bind a provider as the primary session, making it active (back-compat). */
  _bindProvider(provider: SessionProvider): void;
  /** Refresh the State View from the current provider snapshot (no-op if none). */
  _refreshDebugState(): void;
  /** Free the runner and program inspection state (app teardown). */
  disposeSession(): void;
}

export const createSessionSlice: StateCreator<StudioState, [], [], SessionSlice> = (
  set,
  get,
) => {
  // Wire studio services into a local provider so it can notify + log without
  // importing the store (spec §3 ProviderCallbacks).
  const wire = (provider: SessionProvider): void => {
    if (provider instanceof LocalSessionProvider) {
      provider.setCallbacks({
        notify: (n) => get()._notify?.(n),
        appendOutput: (source, text) => get().appendOutput(source, text),
      });
    }
  };

  // Make `id` the active session: subscribe its provider and mirror its
  // snapshot. A switch is a different execution timeline, so `prevDebugState`
  // resets rather than diffing across sessions.
  const setActive = (id: SessionId): void => {
    const entry = get().sessions.find((e) => e.id === id);
    if (!entry) return;
    get()._providerUnsub?.();
    const unsub = entry.provider.subscribe((snap) => {
      mirror(set, snap);
      // Watch cadence (W17/#3310): every stop/turn boundary — the hook
      // itself keys on the stop, so per-line reveals don't storm evals.
      get()._watchOnMirror();
    });
    set({
      _provider: entry.provider,
      _providerUnsub: unsub,
      activeSessionId: entry.id,
      capabilities: entry.provider.capabilities,
    });
    set((s) => ({ sessionRun: s.sessionRun + 1 }));
    mirror(set, entry.provider.getSnapshot(), true);
    // Debug session slice (#3232): a switch is a different provider, so its
    // armed breakpoints/capability must be re-read, not carried over.
    get()._refreshDebugCapability();
  };

  return {
    sessionStatus: "none",
    sessionRun: 0,
    sessionText: [],
    sessionLines: [],
    sessionPacedMs: 150,
    followInEditor: true,
    followPaused: false,
    sessionHoverSource: null,
    sessionPeek: null,
    playerFontSize: 0,
    playerFontFamily: "",
    playerLineHeight: 0,
    playerMeasure: 0,
    showProvenance: true,
    showChoiceMarkers: true,
    hostFonts: null,
    sessionReloadedAt: null,
    _resolveSourceBytes: null,
    sessionChoices: [],
    sessionAuto: false,
    sessionPaused: false,
    sessions: [],
    activeSessionId: null,
    _provider: null,
    _providerUnsub: null,
    _sessionSeq: 1,
    _sessionBytes: null,
    debugState: null,
    prevDebugState: null,
    programModel: null,
    programInkt: null,
    programLines: null,
    programSize: null,
    programChecksum: null,
    capabilities: ALL_CAPABILITIES,

    _bindProvider(provider) {
      wire(provider);
      // Register (or replace) the primary entry, then make it active.
      set((s) => {
        const old = s.sessions.find((e) => e.id === DEFAULT_SESSION_ID);
        if (old && old.provider !== provider) old.provider.dispose();
        const others = s.sessions.filter((e) => e.id !== DEFAULT_SESSION_ID);
        return {
          sessions: [{ id: DEFAULT_SESSION_ID, label: "Main", provider }, ...others],
        };
      });
      setActive(DEFAULT_SESSION_ID);
      // The paced cadence survives provider swaps — re-apply at bind.
      provider.setPacedReveal?.(get().sessionPacedMs);
    },

    setSourceByteResolver(resolver) {
      set({ _resolveSourceBytes: resolver });
    },

    revealMaximally() {
      get()._provider?.continueMaximally?.();
    },

    setSessionPaced(delayMs) {
      const ms = Math.max(0, delayMs);
      set({ sessionPacedMs: ms });
      get()._provider?.setPacedReveal?.(ms);
    },

    setFollowInEditor(on) {
      // Flipping the toggle is an explicit "follow now": it also lifts a
      // pause an edit put in place.
      set({ followInEditor: on, followPaused: false });
    },

    setFollowPaused(paused) {
      if (get().followPaused !== paused) set({ followPaused: paused });
    },

    peekContinue() {
      const provider = get()._provider;
      if (!provider || !isPeekSessionProvider(provider)) return;
      const result = provider.peekContinue();
      set({ sessionPeek: result && result.sources.length > 0 ? result.sources : null });
    },
    peekChoice(index) {
      const provider = get()._provider;
      if (!provider || !isPeekSessionProvider(provider)) return;
      const result = provider.peekChoice(index);
      set({ sessionPeek: result && result.sources.length > 0 ? result.sources : null });
    },
    clearPeek() {
      if (get().sessionPeek !== null) set({ sessionPeek: null });
    },

    setSessionHoverSource(source) {
      const cur = get().sessionHoverSource;
      if (cur === source) return;
      if (
        cur !== null &&
        source !== null &&
        cur.file === source.file &&
        cur.range_start === source.range_start &&
        cur.range_end === source.range_end
      ) {
        return;
      }
      set({ sessionHoverSource: source });
    },

    setPlayerFontFamily(family) {
      set({ playerFontFamily: sanitizeFontFamily(family) });
    },
    setPlayerLineHeight(tenths) {
      // 0 resets; otherwise 1.2–2.2 in tenths.
      const v = tenths <= 0 ? 0 : Math.min(22, Math.max(12, Math.round(tenths)));
      set({ playerLineHeight: v });
    },
    setPlayerMeasure(ch) {
      const v = ch <= 0 ? 0 : Math.min(96, Math.max(48, Math.round(ch)));
      set({ playerMeasure: v });
    },
    setShowProvenance(on) {
      set({ showProvenance: on });
    },
    setShowChoiceMarkers(on) {
      set({ showChoiceMarkers: on });
    },
    setHostFonts(fonts) {
      set({ hostFonts: fonts === null ? null : [...fonts] });
    },

    setPlayerFontSize(px) {
      // Below the readable floor collapses to 0 = "follow the app scale"
      // (so stepping down from 10px lands on the reset, not a stuck
      // clamp); the ceiling matches the editor knob's philosophy.
      const clamped = px < 10 ? 0 : Math.min(32, Math.round(px));
      set({ playerFontSize: clamped });
    },

    startSession(bytes) {
      // A recompile/reload replaces the primary's `Story`, so any shared flows
      // (#200) become stale — dispose + drop them before reloading.
      const staleFlows = get().sessions.filter((e) => e.id.startsWith("flow:"));
      if (staleFlows.length > 0) {
        for (const e of staleFlows) e.provider.dispose();
        set((s) => ({ sessions: s.sessions.filter((e) => !e.id.startsWith("flow:")) }));
      }

      // The auto-start / story.start path always targets the primary session.
      let entry = get().sessions.find((e) => e.id === DEFAULT_SESSION_ID);
      if (!entry) {
        get()._bindProvider(new LocalSessionProvider()); // registers + activates
        entry = get().sessions.find((e) => e.id === DEFAULT_SESSION_ID);
      } else if (get().activeSessionId !== DEFAULT_SESSION_ID) {
        setActive(DEFAULT_SESSION_ID);
      }
      set((s) => ({ _sessionBytes: bytes, sessionRun: s.sessionRun + 1 }));
      entry?.provider.start?.(bytes);
      // A start swaps the provider's internal wasm session — the runtime
      // breakpoint set dies with the old one, so the anchors must re-arm
      // on the new program (W4/W5 #3297/#3298: found live — a solid gutter
      // dot over an empty runtime set is a breakpoint that never hits).
      get()._syncSourceBreakpoints();
      get()._syncDataBreakpoints();
    },

    openSession(opts) {
      const bytes = get().storyBytes ?? get()._sessionBytes;
      if (!bytes) return; // no program to play
      const seq = get()._sessionSeq;
      const id = `local:${seq}`;
      const label = opts?.label ?? opts?.path ?? `Session ${seq}`;
      const provider = new LocalSessionProvider({
        persist: false, // secondary sessions are transient, isolated playthroughs
        startPath: opts?.path ? { path: opts.path, args: opts.args } : undefined,
      });
      wire(provider);
      set((s) => ({
        sessions: [...s.sessions, { id, label, provider }],
        _sessionSeq: seq + 1,
      }));
      setActive(id);
      provider.start(bytes);
      // Same re-arm-on-new-session rule as startSession above.
      get()._syncSourceBreakpoints();
      get()._syncDataBreakpoints();
    },

    openFlow(opts) {
      // Spawn a shared flow on the primary session's runner — it shares that
      // story's globals (#200). Needs a live primary local session.
      const primary = get().sessions.find((e) => e.id === DEFAULT_SESSION_ID)?.provider;
      if (!(primary instanceof LocalSessionProvider) || !primary.hasLiveRunner()) return;
      const seq = get()._sessionSeq;
      const id = `flow:${seq}`;
      const provider = primary.spawnFlow(`flow${seq}`, opts?.path);
      if (!provider) return;
      const label = opts?.label ?? opts?.path ?? `Flow ${seq}`;
      set((s) => ({
        sessions: [...s.sessions, { id, label, provider }],
        _sessionSeq: seq + 1,
      }));
      setActive(id);
      provider.start();
    },

    setActiveSession(id) {
      setActive(id);
    },

    closeSession(id) {
      if (id === DEFAULT_SESSION_ID) return; // the primary session always stays
      const entry = get().sessions.find((e) => e.id === id);
      if (!entry) return;
      const wasActive = get().activeSessionId === id;
      if (wasActive) get()._providerUnsub?.();
      entry.provider.dispose();
      set((s) => ({ sessions: s.sessions.filter((e) => e.id !== id) }));
      if (wasActive) {
        // Fall back to the most-recently-added remaining session.
        const remaining = get().sessions;
        setActive(remaining[remaining.length - 1]?.id ?? DEFAULT_SESSION_ID);
      }
    },

    restartSession() {
      const provider = get()._provider;
      // Reset a live runner in place; otherwise (stopped, or a failed load) a
      // restart means a fresh start on the latest available program — preferring
      // the newest compile, falling back to the session's own bytes.
      if (provider instanceof LocalSessionProvider && provider.hasLiveRunner()) {
        set((s) => ({ sessionRun: s.sessionRun + 1 }));
        provider.restart();
        return;
      }
      const bytes = get().storyBytes ?? get()._sessionBytes;
      if (bytes) get().startSession(bytes);
    },

    stopSession() {
      get()._provider?.stop?.();
      // The provider's underlying runner is gone — its breakpoints/last
      // outcome went with it (#3232).
      get()._refreshDebugCapability();
    },

    chooseOption(index) {
      get()._provider?.choose?.(index);
    },

    revealNext() {
      get()._provider?.continue?.();
    },

    pauseSession() {
      // The pause verb (W5/#3298, ruled first-class): only meaningful on a
      // debug-capable provider; a no-op elsewhere, like every gated verb.
      const provider = get()._provider;
      if (provider !== null && isDebugSessionProvider(provider)) provider.pause();
    },

    setSessionAuto(auto) {
      get()._provider?.setAuto?.(auto);
    },

    _refreshDebugState() {
      const provider = get()._provider;
      if (!provider) {
        set({ debugState: null, prevDebugState: null });
        return;
      }
      mirror(set, provider.getSnapshot());
      get()._watchOnMirror();
    },

    disposeSession() {
      get()._providerUnsub?.();
      for (const entry of get().sessions) entry.provider.dispose();
      set({
        sessions: [],
        activeSessionId: null,
        _provider: null,
        _providerUnsub: null,
        _sessionSeq: 1,
        _sessionBytes: null,
        sessionStatus: "none",
        sessionText: [],
    sessionLines: [],
        sessionChoices: [],
        debugState: null,
        prevDebugState: null,
        programModel: null,
        programInkt: null,
        programLines: null,
        programSize: null,
        programChecksum: null,
        // Back to the default local capability set — the next session is local
        // until a narrower provider binds.
        capabilities: ALL_CAPABILITIES,
      });
      // No provider left to be debug-capable about (#3232).
      get()._refreshDebugCapability();
    },
  };
};

// ── Snapshot mirror ─────────────────────────────────────────────────

type SetFn = {
  (partial: Partial<StudioState>): void;
  (updater: (state: StudioState) => Partial<StudioState>): void;
};

/**
 * Mirror a provider snapshot into the reactive slice fields. `prevDebugState`
 * is derived here from the previous snapshot's debug state (spec §3.1): the
 * provider emits one snapshot per logical advance, so carrying the prior
 * `debugState` forward gives the State View its step diff. On a session switch
 * (`resetPrev`) the prior is dropped — a different execution timeline.
 */
function mirror(set: SetFn, snap: SessionSnapshot, resetPrev = false): void {
  set((s) => ({
    sessionStatus: snap.status,
    sessionText: snap.transcript.map((l) => l.text),
    sessionLines: snap.transcript,
    // A forecast is for the state it was taken in: the transcript moving
    // drops it (the Player re-peeks while the pointer stays).
    sessionPeek:
      snap.transcript === s.sessionLines && snap.status === s.sessionStatus
        ? s.sessionPeek
        : null,
    sessionReloadedAt: snap.reloadedAt,
    sessionChoices: snap.choices,
    sessionAuto: snap.auto,
    prevDebugState: resetPrev ? null : s.debugState,
    debugState: snap.debugState,
    // A frame selection belongs to the stack it was made in (W8/#3301):
    // any runtime advance replaces the snapshot and drops it back to top.
    selectedFrameIdx: snap.debugState !== s.debugState ? null : s.selectedFrameIdx,
    programModel: snap.programModel,
    programInkt: snap.programInkt,
    programChecksum: snap.programChecksum,
    // W5/#3298: paused-ness and the last debug outcome ride the snapshot,
    // so a breakpoint hit during an ordinary reveal reaches the same store
    // fields an explicit debug verb writes.
    sessionPaused: snap.paused,
    debugLastOutcome: snap.debugOutcome,
    debugStatus: statusOfOutcome_(snap.paused, snap.debugOutcome),
  }));
}

/** `debugStatus` from the mirrored snapshot (W5/#3298): `paused` is the
 * authoritative bit (a breakpoint hit or step boundary), a non-null
 * outcome that ended free-running is `stopped`, and no outcome yet is
 * `none` — mirroring the debug slice's own `statusOfOutcome` derivation
 * from before the drive loops unified. */
function statusOfOutcome_(
  paused: boolean,
  outcome: import("@brink/wasm-types").DebugRunOutcome | null,
): "none" | "paused" | "stopped" {
  if (paused) return "paused";
  if (outcome === null) return "none";
  return "stopped";
}

export { EMPTY_SNAPSHOT };
