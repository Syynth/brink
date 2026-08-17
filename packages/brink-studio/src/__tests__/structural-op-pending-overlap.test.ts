/**
 * A genuine overlap between `structuralOpPending`'s two writers — a Binder
 * rename (`applyRename` in `studio-store`'s `binder.ts`) and a symbol-menu op
 * (`runGatedStructuralOp` in `studio-ui`'s `symbolMenuActions.ts`) both in
 * flight at once — proving the compare-and-clear fix (#2794) at the exact
 * point the pre-fix unconditional `setStructuralOpPending(null)` misbehaved:
 * the moment op A's `finally` has run but op B is STILL pending, not merely
 * the final state.
 *
 * That intermediate moment is NOT observable with real timers alone.
 * `scheduleIdleWork`'s jsdom fallback is `setTimeout(work, 0)`; two calls
 * made in the same synchronous tick are both due at the same virtual
 * instant, and `vi.advanceTimersToNextTimerAsync()`/`runAllTimersAsync()`
 * fire (and fully settle, including cascading microtask-only awaits) BOTH
 * of them before returning control to the test — confirmed empirically
 * while developing this fix. Under that batching, the LAST op to have been
 * *set* always finds its own description still live when its `finally`
 * eventually runs, so the final state comes out right whether the clear is
 * unconditional or compare-and-clear; only the (invisible-to-a-final-state
 * assertion) intermediate flicker distinguishes them. `symbol-structural-
 * ops.test.ts`'s "compare-and-clear" tests instead exercise the primitive
 * and the wiring deterministically; THIS file gets genuine time separation
 * by controlling one side of the race directly.
 *
 * The trick: `runGatedStructuralOp` (the symbol-menu op's caller) and
 * `applyRename`'s `project.renameFile` (the Binder rename's caller) both
 * defer through the exact same `ProjectSession.deferGatedCall()` — that is
 * the point of #2794's follow-up fix (see `project-session.ts` and
 * `symbolMenuActions.ts`'s doc comments), and it retires the ONE asymmetry
 * an earlier version of this file exploited (mocking `@brink-lang/editor`'s
 * re-exported `scheduleIdleWork`, which only `symbolMenuActions.ts`'s bare
 * import resolved to — that import no longer exists; `runGatedStructuralOp`
 * now calls `deferGatedCall` directly, the same as `renameFile`). With one
 * shared method instead of two independently-mockable paths, this file
 * instead spies directly on the now-public `ProjectSession.deferGatedCall`
 * and gives its FIRST caller (whichever op starts first) the real
 * timer-backed implementation while parking the SECOND caller's yield on a
 * manually-controlled promise — deterministic ordering with no dependency on
 * which module resolved which import.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";
import { dispatchSymbolAction } from "@brink/studio-ui";

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

const MAIN = ["=== one ===", "First.", "= alpha", "A.", "", "=== two ===", "Second.", ""].join(
  "\n",
);

/** Order of the stitch headers as they appear in a file's source. */
function stitchOrder(source: string): string[] {
  return [...source.matchAll(/^=\s+(\w+)/gm)].map((m) => m[1]!);
}

/**
 * Spy on `project.deferGatedCall` so its first call runs the real,
 * timer-backed implementation (settled by `vi.runAllTimersAsync()`) and its
 * second call parks on a promise this function's caller controls directly —
 * genuine time separation between two ops that now share one defer method.
 * Returns the release function for the parked (second) call.
 */
function deferSecondCallManually(project: ProjectSession): () => void {
  const original = project.deferGatedCall.bind(project);
  let release!: () => void;
  const parked = new Promise<void>((resolve) => {
    release = resolve;
  });
  const spy = vi.spyOn(project, "deferGatedCall");
  spy.mockImplementationOnce(() => original());
  spy.mockImplementationOnce(() => parked);
  return release;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("structuralOpPending: a genuine overlap between the two writers (#2794)", () => {
  it("the Binder rename settling first does not clear the still-pending symbol-menu op's indicator", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN, "lib.ink": "-> END\n" });
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const state = store.getState();

    // First caller (the Binder rename) gets the real, timer-backed defer;
    // second caller (the symbol-menu move) parks until released below.
    const releaseMove = deferSecondCallManually(project);

    // The Binder rename starts first — real timer.
    const renamePending = state.renameFile("lib.ink", "util.ink");
    expect(store.getState().structuralOpPending).toBe("Renaming lib.ink → util.ink");

    // The symbol-menu move starts next, overwriting the pending description
    // — the "overlapping Binder drag-move and symbol-menu op" case the issue
    // names. Its defer call is the parked one — it stays pending until this
    // test explicitly releases it below.
    const movePending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");

    // Let the rename's real timer run all the way to completion. The move's
    // defer is parked on a plain promise, not a timer, so this settles ONLY
    // the rename.
    await vi.runAllTimersAsync();
    await renamePending;

    // The rename's `finally` ran and tried to clear ITS OWN description —
    // but the live value is "Move alpha to two" now, so the
    // compare-and-clear must have been a no-op. Before #2794 this was an
    // unconditional `setStructuralOpPending(null)`, which would have wiped
    // the still-in-flight move's indicator right here.
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");

    // Now release the move's parked defer — its `compute()` is synchronous,
    // so this settles the whole op, including its own `finally`-clear.
    releaseMove();
    await movePending;

    // The move's own clear DOES apply — it is the one still live.
    expect(store.getState().structuralOpPending).toBeNull();

    // Both operations actually landed — neither was dropped or corrupted by
    // the overlap.
    expect(project.getSession().getFileSource("lib.ink")).toBeNull();
    expect(project.getSession().getFileSource("util.ink")).toBe("-> END\n");
    // MAIN's only stitch is `alpha`, under `one` — the move relocates it
    // under `two`, leaving `one` with none.
    expect(stitchOrder(project.getSession().getFileSource("main.ink")!)).toEqual(["alpha"]);
  });

  it("the reverse overlap (symbol-menu op settling first) holds the same invariant", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN, "lib.ink": "-> END\n" });
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const state = store.getState();

    // This time the symbol-menu move starts first, so it is the FIRST
    // caller of `deferGatedCall` — real timer. The rename starts second and
    // parks.
    const releaseRename = deferSecondCallManually(project);

    const movePending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");

    const renamePending = state.renameFile("lib.ink", "util.ink");
    expect(store.getState().structuralOpPending).toBe("Renaming lib.ink → util.ink");

    // Settle the move FIRST this time — its defer is the real timer-backed
    // one, so running timers settles it without touching the rename's
    // parked defer at all.
    await vi.runAllTimersAsync();
    await movePending;

    // The move's clear tried to remove "Move alpha to two" — but the live
    // value is now the rename's description, so it must be a no-op.
    expect(store.getState().structuralOpPending).toBe("Renaming lib.ink → util.ink");

    releaseRename();
    await renamePending;

    // The rename's own clear DOES apply.
    expect(store.getState().structuralOpPending).toBeNull();
    expect(project.getSession().getFileSource("util.ink")).toBe("-> END\n");
  });
});
