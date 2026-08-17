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
 * The trick: mock `@brink-lang/editor`'s `scheduleIdleWork` so the
 * symbol-menu op's yield becomes a manually-drained queue entry instead of a
 * timer. `ProjectSession.renameFile` (also re-exported from that package)
 * calls its OWN copy of `scheduleIdleWork` via a *relative* import
 * (`./idle-schedule.js` inside `project-session.ts`) — a different resolved
 * module than the `"@brink-lang/editor"` specifier this file mocks, so that
 * call is UNTOUCHED and still yields on a real timer. That asymmetry is
 * exactly what makes the two writers separable: the Binder rename settles on
 * its own schedule while the symbol-menu op's yield stays parked until this
 * test explicitly drains it.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";
import { dispatchSymbolAction } from "@brink/studio-ui";

const { idleQueue } = vi.hoisted(() => ({ idleQueue: [] as Array<() => void> }));

vi.mock("@brink-lang/editor", async (importOriginal) => {
  const original = await importOriginal<typeof import("@brink-lang/editor")>();
  return {
    ...original,
    // Only `symbolMenuActions.ts`'s bare `import { scheduleIdleWork } from
    // "@brink-lang/editor"` resolves to this mocked specifier. See this
    // file's header for why `ProjectSession`'s own defer (a *relative*
    // import inside `project-session.ts`) is a different resolved module
    // and stays real.
    scheduleIdleWork: (work: () => void) => {
      idleQueue.push(work);
      return idleQueue.length;
    },
    cancelIdleWork: () => {
      /* no-op: nothing in this file cancels a queued entry */
    },
  };
});

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

beforeEach(() => {
  vi.useFakeTimers();
  idleQueue.length = 0;
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

    // The Binder rename starts first — it yields on a REAL timer
    // (ProjectSession's own scheduleIdleWork, untouched by the mock above).
    const renamePending = state.renameFile("lib.ink", "util.ink");
    expect(store.getState().structuralOpPending).toBe("Renaming lib.ink → util.ink");

    // The symbol-menu move starts next, overwriting the pending description
    // — the "overlapping Binder drag-move and symbol-menu op" case the issue
    // names. Its yield lands in `idleQueue`, NOT a timer, so it stays parked
    // until this test explicitly drains it.
    const movePending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");
    expect(idleQueue).toHaveLength(1); // the move's yield, parked — no timer

    // Let the rename's real timer run all the way to completion. The move's
    // yield is not a timer at all, so this settles ONLY the rename.
    await vi.runAllTimersAsync();
    await renamePending;

    // The rename's `finally` ran and tried to clear ITS OWN description —
    // but the live value is "Move alpha to two" now, so the
    // compare-and-clear must have been a no-op. Before #2794 this was an
    // unconditional `setStructuralOpPending(null)`, which would have wiped
    // the still-in-flight move's indicator right here.
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");
    expect(idleQueue).toHaveLength(1); // the move's yield is STILL parked

    // Now drain the move's yield manually — its `compute()` is synchronous,
    // so this settles the whole op, including its own `finally`-clear.
    idleQueue.shift()!();
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

    const movePending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");
    expect(idleQueue).toHaveLength(1);

    const renamePending = state.renameFile("lib.ink", "util.ink");
    expect(store.getState().structuralOpPending).toBe("Renaming lib.ink → util.ink");

    // Settle the move FIRST this time — drain its (still lone) queue entry
    // directly, without touching the rename's real timer at all.
    idleQueue.shift()!();
    await movePending;

    // The move's clear tried to remove "Move alpha to two" — but the live
    // value is now the rename's description, so it must be a no-op.
    expect(store.getState().structuralOpPending).toBe("Renaming lib.ink → util.ink");

    await vi.runAllTimersAsync();
    await renamePending;

    // The rename's own clear DOES apply.
    expect(store.getState().structuralOpPending).toBeNull();
    expect(project.getSession().getFileSource("util.ink")).toBe("-> END\n");
  });
});
