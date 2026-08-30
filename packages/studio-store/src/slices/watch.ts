/**
 * Watch — the full mini-REPL (W17/#3310, spec §F18 RULED): entries are
 * arbitrary typed expressions (`gold >= pour(2)`) OR divert/content
 * fragments (`-> market.haggle` → a transcript preview of what it WOULD
 * produce), evaluated over the shipped speculation/fragment-eval engine
 * (F4.1–F4.3 + F5.1) — side-effect-proof by construction (discard-on-
 * drop sandbox, budgeted).
 *
 * Cadence (RULED): re-evaluated at every STOP/turn boundary — a pause,
 * a breakpoint hit, a choice point, done/ended, or a turn-index change —
 * never per revealed line. The fragment compile is paid once per entry
 * per program version (the runner's `compileFragment` cache keys by
 * checksum; the provider re-keys its scratch runner on hot reload).
 * Degraded suppresses re-evaluation like every position feature; stale
 * async results are dropped by generation.
 */

import type { StateCreator } from "zustand";
import type { SpeculationResult } from "@brink-lang/web";
import type { ProjectSource } from "@brink/wasm-types";
import type { StudioState } from "../index.js";
import { isDebugSessionProvider, sessionDegraded } from "../session/types.js";

export interface WatchEntry {
  id: string;
  source: string;
}

export type WatchResult =
  | { kind: "pending" }
  | { kind: "value"; display: string }
  | {
      kind: "transcript";
      lines: string[];
      /** Choice texts when the preview stopped at a choice point. */
      reachedChoices: string[];
      /** A budget stop — the preview is truncated, not complete. */
      truncated: boolean;
    }
  | { kind: "error"; message: string };

export interface WatchSlice {
  watchEntries: WatchEntry[];
  /** Latest result per entry id — `pending` while an eval is in flight. */
  watchResults: Record<string, WatchResult>;
  watchAdd(source: string): void;
  watchRemove(id: string): void;
  /** Re-evaluate every entry against the live session's current state.
   * No-op without a live debug-capable provider; suppressed while the
   * session is degraded (out-of-sync program). */
  watchReevalAll(): void;
  /** Mirror hook: called on every snapshot mirror; re-evaluates only when
   * the (status, paused, turn) stop-key actually changed to a stop. */
  _watchOnMirror(): void;
  /** Tier-1 fragments recompile against the project's CURRENT sources —
   * the store doesn't hold them, so the app boundary wires this (mount,
   * same pattern as the source-byte resolver). */
  setWatchProjectSource(provider: (() => ProjectSource | null) | null): void;
  _watchProjectSource: (() => ProjectSource | null) | null;
  _watchGen: number;
  _watchLastStopKey: string | null;
  _watchSeq: number;
}

/** SpeculationResult → the row's rendered form. */
export function watchResultOf(res: SpeculationResult): WatchResult {
  if (res.diagnostics.length > 0) {
    return { kind: "error", message: res.diagnostics[0] };
  }
  if (res.value !== undefined) {
    const v = res.value;
    const display =
      v.type === "string"
        ? `"${v.value}"`
        : v.type === "null"
          ? "null"
          : v.type === "list"
            ? `(${v.items.map((m) => m.name).join(", ")})`
            : v.type === "divert"
              ? `-> ${v.path ?? "?"}`
              : String(v.value);
    return { kind: "value", display };
  }
  return {
    kind: "transcript",
    lines: res.transcript.map((l) => l.text).filter((t) => t.trim().length > 0),
    reachedChoices: (res.reachedChoices ?? []).map((c) => c.text),
    truncated: res.stop === "step-budget" || res.stop === "line-budget",
  };
}

/** Watch previews are bounded (the "guard against unbounded growth"
 * rule): a runaway fragment stops at the budget and the row says so. */
const WATCH_BUDGET = { steps: 20_000, lines: 40 };

export const createWatchSlice: StateCreator<StudioState, [], [], WatchSlice> = (set, get) => ({
  watchEntries: [],
  watchResults: {},
  _watchProjectSource: null,
  _watchGen: 0,
  _watchLastStopKey: null,
  _watchSeq: 0,

  setWatchProjectSource(provider) {
    set({ _watchProjectSource: provider });
  },

  watchAdd(source) {
    const trimmed = source.trim();
    if (trimmed === "") return;
    const id = `w${(get()._watchSeq + 1).toString()}`;
    set((s) => ({
      _watchSeq: s._watchSeq + 1,
      watchEntries: [...s.watchEntries, { id, source: trimmed }],
      watchResults: { ...s.watchResults, [id]: { kind: "pending" } },
    }));
    get().watchReevalAll();
  },

  watchRemove(id) {
    set((s) => {
      const { [id]: _dropped, ...rest } = s.watchResults;
      return {
        watchEntries: s.watchEntries.filter((e) => e.id !== id),
        watchResults: rest,
      };
    });
  },

  watchReevalAll() {
    const st = get();
    if (st.watchEntries.length === 0) return;
    const provider = st._provider;
    if (
      !provider ||
      !isDebugSessionProvider(provider) ||
      typeof provider.evaluateWatch !== "function"
    ) {
      return;
    }
    // Degraded suppression (RULED — like every position feature): the
    // session runs an older compile; evaluating against it would lie.
    if (sessionDegraded(st.programChecksum, st.compiledChecksum)) return;

    const gen = st._watchGen + 1;
    set((s) => ({
      _watchGen: gen,
      watchResults: Object.fromEntries(
        s.watchEntries.map((e) => [e.id, { kind: "pending" } as WatchResult]),
      ),
    }));
    const projectSource = st._watchProjectSource?.() ?? undefined;
    for (const entry of st.watchEntries) {
      const pending = provider.evaluateWatch(entry.source, {
        projectSource,
        budget: WATCH_BUDGET,
      });
      if (pending === null) continue;
      pending
        .then((res) => {
          if (get()._watchGen !== gen) return; // stale round
          set((s) => ({
            watchResults: { ...s.watchResults, [entry.id]: watchResultOf(res) },
          }));
        })
        .catch((e: unknown) => {
          if (get()._watchGen !== gen) return;
          const message = e instanceof Error ? e.message : String(e);
          set((s) => ({
            watchResults: { ...s.watchResults, [entry.id]: { kind: "error", message } },
          }));
        });
    }
  },

  _watchOnMirror() {
    const st = get();
    if (st.watchEntries.length === 0) return;
    const stopped =
      st.sessionPaused ||
      st.sessionStatus === "awaiting-choice" ||
      st.sessionStatus === "done" ||
      st.sessionStatus === "ended";
    if (!stopped) return;
    const key = `${st.sessionStatus}|${st.sessionPaused ? "p" : "r"}|${st.debugState?.turn_index ?? -1}|${st.debugState?.position?.container_idx ?? -1}:${st.debugState?.position?.offset ?? -1}`;
    if (key === st._watchLastStopKey) return;
    set({ _watchLastStopKey: key });
    st.watchReevalAll();
  },
});
