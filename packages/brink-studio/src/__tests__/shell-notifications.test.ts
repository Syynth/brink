/**
 * Notification service (docs/studio-shell-spec.md §7.5, shell issue 3.3).
 *
 * Covers the NotificationCenter semantics — severity-default timeouts
 * (info 5s / warning 8s / error sticky) with explicit override, the
 * visible-3 + overflow stack, the capped session history, unread counting
 * with reset-on-open, hover pause/resume, handle dismiss/update, and
 * command-only actions — plus the store→shell bridge: binder operations
 * notify through the injected `_notify` with an Undo action that dispatches
 * the `binder.undo` command (gated on a non-empty undo stack).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  CommandRegistry,
  NotificationCenter,
  NOTIFICATION_HISTORY_LIMIT,
  MAX_VISIBLE_NOTIFICATIONS,
  SEVERITY_TIMEOUTS,
} from "@brink/studio-shell";
import { createStudioStore, type StoreNotification } from "@brink/studio-store";
import type { MoveResult } from "@brink/wasm-types";

describe("NotificationCenter", () => {
  let center: NotificationCenter;

  beforeEach(() => {
    vi.useFakeTimers();
    center = new NotificationCenter();
  });

  afterEach(() => {
    center.dispose();
    vi.useRealTimers();
  });

  // ── Timeouts ──────────────────────────────────────────────────────

  it("auto-dismisses info after 5s and warning after 8s (severity defaults)", () => {
    center.notify({ severity: "info", message: "saved" });
    center.notify({ severity: "warning", message: "diverged" });
    expect(center.getState().visible).toHaveLength(2);

    vi.advanceTimersByTime(4999);
    expect(center.getState().visible).toHaveLength(2);

    vi.advanceTimersByTime(1); // 5000 — info dismisses
    expect(center.getState().visible.map((n) => n.message)).toEqual(["diverged"]);

    vi.advanceTimersByTime(3000); // 8000 — warning dismisses
    expect(center.getState().visible).toHaveLength(0);
    // Both stay in history.
    expect(center.getState().history).toHaveLength(2);
  });

  it("keeps errors sticky (no auto-dismiss)", () => {
    expect(SEVERITY_TIMEOUTS.error).toBeNull();
    center.notify({ severity: "error", message: "boom" });
    vi.advanceTimersByTime(60_000);
    expect(center.getState().visible.map((n) => n.message)).toEqual(["boom"]);
  });

  it("lets an explicit timeoutMs override the severity default", () => {
    center.notify({ severity: "error", message: "transient error", timeoutMs: 1000 });
    center.notify({ severity: "info", message: "pinned info", timeoutMs: 0 }); // ≤0 = sticky

    vi.advanceTimersByTime(1000);
    expect(center.getState().visible.map((n) => n.message)).toEqual(["pinned info"]);

    vi.advanceTimersByTime(60_000);
    expect(center.getState().visible.map((n) => n.message)).toEqual(["pinned info"]);
  });

  // ── Stack: visible 3 + overflow ───────────────────────────────────

  it("shows at most 3 toasts, newest first, with the rest as overflow", () => {
    for (let i = 1; i <= 5; i++) {
      center.notify({ severity: "error", message: `n${i}` });
    }
    const state = center.getState();
    expect(MAX_VISIBLE_NOTIFICATIONS).toBe(3);
    expect(state.visible.map((n) => n.message)).toEqual(["n5", "n4", "n3"]);
    expect(state.overflowCount).toBe(2);
  });

  it("reveals a collapsed notification when a visible one is dismissed", () => {
    const handles = [1, 2, 3, 4].map((i) =>
      center.notify({ severity: "error", message: `n${i}` }),
    );
    handles[3]!.dismiss();
    const state = center.getState();
    expect(state.visible.map((n) => n.message)).toEqual(["n3", "n2", "n1"]);
    expect(state.overflowCount).toBe(0);
  });

  // ── History ───────────────────────────────────────────────────────

  it("caps the session history, dropping the oldest (unbounded-growth guard)", () => {
    for (let i = 1; i <= NOTIFICATION_HISTORY_LIMIT + 5; i++) {
      center.notify({ severity: "info", message: `n${i}`, timeoutMs: 0 });
    }
    const { history } = center.getState();
    expect(history).toHaveLength(NOTIFICATION_HISTORY_LIMIT);
    expect(history[0]!.message).toBe(`n${NOTIFICATION_HISTORY_LIMIT + 5}`); // newest first
    expect(history.at(-1)!.message).toBe("n6"); // n1..n5 dropped
  });

  it("keeps dismissed notifications in history", () => {
    const handle = center.notify({ severity: "info", message: "gone" });
    handle.dismiss();
    expect(center.getState().visible).toHaveLength(0);
    expect(center.getState().history.map((n) => n.message)).toEqual(["gone"]);
  });

  it("clearHistory empties the history (and unread) but keeps active toasts", () => {
    center.notify({ severity: "error", message: "still here" });
    center.clearHistory();
    const state = center.getState();
    expect(state.history).toHaveLength(0);
    expect(state.unread).toBe(0);
    expect(state.visible.map((n) => n.message)).toEqual(["still here"]);
  });

  // ── Unread / popover open state ───────────────────────────────────

  it("counts unread and resets when the history popover opens", () => {
    center.notify({ severity: "info", message: "a" });
    center.notify({ severity: "info", message: "b" });
    expect(center.getState().unread).toBe(2);

    center.openHistory();
    expect(center.getState().unread).toBe(0);
    expect(center.getState().historyOpen).toBe(true);

    // Raised while the popover is open = being seen — stays read.
    center.notify({ severity: "info", message: "c" });
    expect(center.getState().unread).toBe(0);

    center.closeHistory();
    center.notify({ severity: "info", message: "d" });
    expect(center.getState().unread).toBe(1);
  });

  // ── Pause / resume (toast hover) ──────────────────────────────────

  it("pauses auto-dismiss on hover and resumes with the remaining delay", () => {
    const handle = center.notify({ severity: "info", message: "hover me" });

    vi.advanceTimersByTime(3000);
    center.pauseTimeout(handle.id); // 2000ms remaining

    vi.advanceTimersByTime(60_000); // paused — nothing happens
    expect(center.getState().visible).toHaveLength(1);

    center.resumeTimeout(handle.id);
    vi.advanceTimersByTime(1999);
    expect(center.getState().visible).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(center.getState().visible).toHaveLength(0);
  });

  it("pause/resume are no-ops for sticky or unknown notifications", () => {
    const handle = center.notify({ severity: "error", message: "sticky" });
    center.pauseTimeout(handle.id);
    center.resumeTimeout(handle.id);
    center.pauseTimeout("nope");
    center.resumeTimeout("nope");
    vi.advanceTimersByTime(60_000);
    expect(center.getState().visible).toHaveLength(1);
  });

  // ── Handle: dismiss / update ──────────────────────────────────────

  it("handle.update amends the notification and restarts its timer", () => {
    const handle = center.notify({ severity: "info", message: "working…" });
    vi.advanceTimersByTime(4000);

    handle.update({ message: "done", severity: "warning" });
    const updated = center.getState().visible[0]!;
    expect(updated.message).toBe("done");
    expect(updated.severity).toBe("warning");
    // History entry is amended too (same id, no duplicate).
    expect(center.getState().history.map((n) => n.message)).toEqual(["done"]);

    // Timer restarted from the new severity's 8s, not 1s left of the old 5s.
    vi.advanceTimersByTime(7999);
    expect(center.getState().visible).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(center.getState().visible).toHaveLength(0);
  });

  it("an explicit duplicate id replaces the active notification", () => {
    center.notify({ id: "compile", severity: "info", message: "compiling" });
    center.notify({ id: "compile", severity: "error", message: "failed" });
    const state = center.getState();
    expect(state.visible).toHaveLength(1);
    expect(state.visible[0]!.message).toBe("failed");
  });

  // ── Actions: commands only ────────────────────────────────────────

  it("stores actions as plain command dispatches — serializable, no callbacks", () => {
    center.notify({
      severity: "info",
      message: "moved",
      actions: [{ label: "Undo", commandId: "binder.undo", args: { n: 1 } }],
    });
    const n = center.getState().visible[0]!;
    expect(n.actions).toEqual([{ label: "Undo", commandId: "binder.undo", args: { n: 1 } }]);
    // The whole model round-trips through JSON (command-only actions —
    // a callback field would be dropped/break this).
    expect(JSON.parse(JSON.stringify(n))).toEqual(n);
  });

  // ── Subscription ──────────────────────────────────────────────────

  it("onDidChange fires on notify/dismiss/clear and stops after unsubscribe", () => {
    const listener = vi.fn();
    const unsubscribe = center.onDidChange(listener);

    const handle = center.notify({ severity: "info", message: "x" });
    expect(listener).toHaveBeenCalledTimes(1);
    handle.dismiss();
    expect(listener).toHaveBeenCalledTimes(2);
    handle.dismiss(); // already gone — no change, no emit
    expect(listener).toHaveBeenCalledTimes(2);
    center.clearHistory();
    expect(listener).toHaveBeenCalledTimes(3);

    unsubscribe();
    center.notify({ severity: "info", message: "y" });
    expect(listener).toHaveBeenCalledTimes(3);
  });

  it("getState snapshots are stable between changes (useSyncExternalStore)", () => {
    center.notify({ severity: "info", message: "x" });
    const a = center.getState();
    const b = center.getState();
    expect(a).toBe(b);
    center.notify({ severity: "info", message: "y" });
    expect(center.getState()).not.toBe(a);
  });
});

// ── Store → shell bridge (binder undo) ────────────────────────────────

describe("binder notification bridge", () => {
  /** Minimal project/editor doubles for applyMoveResult/undo. */
  function makeStoreHarness() {
    const store = createStudioStore();
    const sources = new Map<string, string>([["main.ink", "old source"]]);
    const session = {
      getFileSource: (path: string) => sources.get(path) ?? null,
      updateFile: vi.fn((path: string, source: string) => {
        sources.set(path, source);
      }),
    };
    const notify = vi.fn<(n: StoreNotification) => void>();
    store.setState({
      _project: { getSession: () => session } as never,
      _stateManager: { invalidateFile: vi.fn() } as never,
      _editorRef: { triggerCompile: vi.fn() } as never,
    });
    store.getState().setNotifier(notify);
    return { store, session, sources, notify };
  }

  const moveResult: MoveResult = {
    ok: true,
    path: "main.ink",
    new_source: "new source",
    cross_file_edits: [],
  };

  it("applyMoveResult notifies with an Undo action dispatching binder.undo", async () => {
    const { store, notify } = makeStoreHarness();

    await store.getState().applyMoveResult(moveResult, "Moved knot intro", ["main.ink"]);

    expect(notify).toHaveBeenCalledWith({
      severity: "info",
      source: "binder",
      message: "Moved knot intro",
      actions: [{ label: "Undo", commandId: "binder.undo" }],
    });
    expect(store.getState().undoStack).toHaveLength(1);
  });

  it("binder.undo restores the snapshot and is gated on a non-empty stack", async () => {
    const { store, session, sources, notify } = makeStoreHarness();
    const commands = new CommandRegistry();
    commands.register({
      id: "binder.undo",
      title: "Binder: Undo Last Operation",
      when: () => store.getState().undoStack.length > 0,
      run: () => void store.getState().undo(),
    });

    // Nothing to undo yet — the command is disabled and dispatch refuses.
    expect(commands.isEnabled("binder.undo")).toBe(false);
    expect(commands.dispatch("binder.undo")).toBe(false);

    await store.getState().applyMoveResult(moveResult, "Moved knot intro", ["main.ink"]);
    expect(sources.get("main.ink")).toBe("new source");
    expect(commands.isEnabled("binder.undo")).toBe(true);

    // What the notification's Undo button does: dispatch the command id.
    expect(commands.dispatch("binder.undo")).toBe(true);
    await vi.waitFor(() => {
      expect(store.getState().undoStack).toHaveLength(0);
    });
    expect(session.updateFile).toHaveBeenLastCalledWith("main.ink", "old source");
    expect(notify).toHaveBeenLastCalledWith({
      severity: "info",
      source: "binder",
      message: "Undid: Moved knot intro",
    });

    // Stack consumed — gated off again.
    expect(commands.isEnabled("binder.undo")).toBe(false);
  });
});
