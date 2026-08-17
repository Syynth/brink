/**
 * The Binder's structural symbol menu, end to end (#2577).
 *
 * `dispatchSymbolAction` (`packages/studio-ui/src/symbolMenuActions.ts`) is the
 * one dispatcher behind every knot/stitch context-menu refactor — the Binder's
 * menu and its drag-drop, the editor, and the Story Graph all route through it.
 * Seven of its branches (`reorderStitch`, `reorderKnot`, `reorderStitches`,
 * `reorderKnots`, `moveStitch`, `promoteStitch`, `demoteKnot`) call ops that the
 * studio's wasm mock had NO METHOD for at all, so before #2577 this file could
 * not have existed: every case below threw
 * `session.reorderStitch is not a function` — the op was untestable, not merely
 * untested.
 *
 * These run the real `ProjectSession` + real studio store against the mock (the
 * unit suite's `brink-web` alias), so what is exercised is the production
 * dispatcher and the production apply path, not a re-implementation.
 *
 * ⚠ The refusal cases below pin what the dispatcher does TODAY: `result.ok`
 * false → it returns silently, applying nothing and telling the user nothing
 * *distinct from a refusal*. That silence is #2544's production-side
 * reporting contract (the rename surfaces already notify; these seven do
 * not), which needs a maintainer ruling — so it is pinned here as observed
 * behavior, not asserted as correct. The value of pinning it is that #2544
 * now has a test to flip.
 *
 * #2767 adds a SEPARATE, non-#2544 signal around the three gated ops
 * (`moveStitch`/`promoteStitch`/`demoteKnot`): a synchronous
 * `structuralOpPending` busy-state commit before the deferred wasm call runs,
 * cleared once it settles — success or refusal alike (`runGatedStructuralOp`
 * in `symbolMenuActions.ts`; store field in `studio-store`'s `symbol-menu`
 * slice; rendered by `StructuralOpSegment` in the status bar, spec §7.7.4).
 * It is a progress affordance, not the #2544 refusal report, and — per spec
 * §7.5's "out of scope: progress notifications" — is not a notification and
 * does not appear in `raised` below.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import {
  createStudioStore,
  type DocumentSessions as StoreDocs,
  type StoreNotification,
} from "@brink/studio-store";
import { dispatchSymbolAction } from "@brink/studio-ui";

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

/** A store wired the way `mount.tsx` wires it, plus a capture of every
 *  notification raised (the channel a refusal would report through). */
async function makeStore(files: Record<string, string>) {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const store = createStudioStore();
  store.setState({ _project: project, _documents: stubDocuments() });
  const raised: StoreNotification[] = [];
  store.getState().setNotifier((n) => raised.push(n));
  return { store, project, raised };
}

const MAIN = [
  "=== one ===",
  "First.",
  "= alpha",
  "A.",
  "= beta",
  "B.",
  "",
  "=== two ===",
  "Second.",
  "",
].join("\n");

/** Order of the stitch headers as they appear in a file's source. */
function stitchOrder(source: string): string[] {
  return [...source.matchAll(/^=\s+(\w+)/gm)].map((m) => m[1]!);
}

/** Order of the knot headers as they appear in a file's source. */
function knotOrder(source: string): string[] {
  return [...source.matchAll(/^===\s+(\w+)\s*===/gm)].map((m) => m[1]!);
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("dispatchSymbolAction: the structural ops the mock had no method for (#2577)", () => {
  it("reorderStitch moves a stitch down and writes the file back", async () => {
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderStitch",
      path: "main.ink",
      knot: "one",
      stitch: "alpha",
      direction: 1,
    });

    const src = project.getSession().getFileSource("main.ink")!;
    expect(stitchOrder(src)).toEqual(["beta", "alpha"]);
    // The knot header and the preamble-free body around it are untouched.
    expect(knotOrder(src)).toEqual(["one", "two"]);
    expect(src).toContain("=== one ===\nFirst.\n");
    // A successful apply raises exactly its one informational toast.
    expect(raised.filter((n) => n.severity === "error")).toHaveLength(0);
  });

  it("reorderKnot moves a knot down", async () => {
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderKnot",
      path: "main.ink",
      knot: "one",
      direction: 1,
    });

    expect(knotOrder(project.getSession().getFileSource("main.ink")!)).toEqual(["two", "one"]);
  });

  it("reorderStitches applies a whole drag-drop permutation", async () => {
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderStitches",
      path: "main.ink",
      knot: "one",
      order: ["beta", "alpha"],
    });

    expect(stitchOrder(project.getSession().getFileSource("main.ink")!)).toEqual([
      "beta",
      "alpha",
    ]);
  });

  it("moveStitch relocates a stitch and requalifies its diverts across files", async () => {
    const { store, project } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> one.alpha\n",
    });
    const state = store.getState();

    // moveStitch is gated (runs the full breakage reanalysis, #2767) and now
    // defers its wasm call to the next idle slot — see symbolMenuActions.ts's
    // runGatedStructuralOp. Under fake timers that deferred macrotask never
    // fires on its own; runAllTimersAsync both advances it and drains the
    // microtask queue so the awaited dispatch settles.
    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });
    await vi.runAllTimersAsync();
    await pending;

    const session = project.getSession();
    const src = session.getFileSource("main.ink")!;
    // `alpha` now sits after `two`'s header, and `one` keeps only `beta`.
    expect(src.indexOf("= alpha")).toBeGreaterThan(src.indexOf("=== two ==="));
    expect(src.indexOf("= beta")).toBeLessThan(src.indexOf("=== two ==="));
    // The cross-file divert travelled with it.
    expect(session.getFileSource("other.ink")).toContain("-> two.alpha");
  });

  it("promoteStitch lifts a stitch to a knot and unqualifies its diverts", async () => {
    const { store, project } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> one.alpha\n",
    });
    const state = store.getState();

    // promoteStitch is gated too — see the moveStitch test's comment above.
    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "promoteStitch",
      path: "main.ink",
      knot: "one",
      stitch: "alpha",
    });
    await vi.runAllTimersAsync();
    await pending;

    const session = project.getSession();
    const src = session.getFileSource("main.ink")!;
    // Promoted immediately after its former parent — the real op's position.
    expect(knotOrder(src)).toEqual(["one", "alpha", "two"]);
    expect(stitchOrder(src)).toEqual(["beta"]);
    expect(session.getFileSource("other.ink")).toContain("-> alpha");
  });

  it("demoteKnot folds a knot into another as its last stitch", async () => {
    const { store, project } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> two\n",
    });
    const state = store.getState();

    // demoteKnot is gated too — see the moveStitch test's comment above.
    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "demoteKnot",
      path: "main.ink",
      knot: "two",
      destKnot: "one",
    });
    await vi.runAllTimersAsync();
    await pending;

    const session = project.getSession();
    const src = session.getFileSource("main.ink")!;
    expect(knotOrder(src)).toEqual(["one"]);
    expect(stitchOrder(src)).toEqual(["alpha", "beta", "two"]);
    expect(session.getFileSource("other.ink")).toContain("-> one.two");
  });
});

describe("a refused structural op applies nothing (#2577; reporting is #2544)", () => {
  it("moveStitch onto an occupied name leaves every file untouched", async () => {
    // `two` already owns an `alpha`, so the destination-collision check — the
    // one the real op runs before it even resolves the source stitch — refuses.
    const COLLIDING = `${MAIN}= alpha\nOther A.\n`;
    const OTHER = "-> one.alpha\n";
    const { store, project, raised } = await makeStore({
      "main.ink": COLLIDING,
      "other.ink": OTHER,
    });
    const state = store.getState();
    const before = project.getSession().getFileSource("main.ink")!;

    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });
    // Synchronously, before anything is deferred: the busy-state affordance
    // is set (see the "run off the paint path" describe block below for the
    // ordering proof) — but it is store state, not a notification.
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");
    await vi.runAllTimersAsync();
    await pending;

    expect(project.getSession().getFileSource("main.ink")).toBe(before);
    // Nothing partial: the cross-file requalification did not happen either.
    expect(project.getSession().getFileSource("other.ink")).toBe(OTHER);
    // The busy state clears once the (refused) op settles.
    expect(store.getState().structuralOpPending).toBeNull();

    // ⚠ #2544: the dispatcher's `if (result.ok && result.path)` swallows the
    // refusal itself — no DISTINCT refusal notification, nothing telling the
    // user *why* nothing happened. The rename surfaces DO report
    // (`notifyRenameRefusal`, #2528/#2543); these seven do not. Pinned, not
    // endorsed — flipping it is #2544's ruling to make. #2767's pending
    // busy-state affordance is not a notification (spec §7.5) and does not
    // change this: `raised` stays empty.
    expect(raised).toHaveLength(0);
  });

  it("promoteStitch onto an existing knot name refuses without a partial write", async () => {
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const before = project.getSession().getFileSource("main.ink")!;

    // Promoting `alpha` is fine, but a stitch named `two` would collide with
    // the existing top-level knot — the real op's first check.
    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "promoteStitch",
      path: "main.ink",
      knot: "one",
      stitch: "two",
    });
    expect(store.getState().structuralOpPending).toBe("Promote two to knot");
    await vi.runAllTimersAsync();
    await pending;

    expect(project.getSession().getFileSource("main.ink")).toBe(before);
    expect(store.getState().structuralOpPending).toBeNull();
    // Same #2544 note as the moveStitch refusal test above.
    expect(raised).toHaveLength(0);
  });

  it("reorderStitches with a non-permutation refuses", async () => {
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const before = project.getSession().getFileSource("main.ink")!;

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderStitches",
      path: "main.ink",
      knot: "one",
      order: ["alpha", "alpha"],
    });

    expect(project.getSession().getFileSource("main.ink")).toBe(before);
  });
});

describe("moveStitch/promoteStitch/demoteKnot run off the paint path (#2767)", () => {
  it("commits the pending busy-state synchronously, before the deferred wasm call runs", async () => {
    // The load-bearing property #722/#2761 established and #2767 extends
    // here: a paint-worthy state change must land in the SAME synchronous
    // tick as the triggering event, before the heavy analysis call (deferred
    // to the next idle slot) ever executes. Asserting `structuralOpPending`
    // is set BEFORE awaiting anything proves that ordering directly, rather
    // than trusting it because the final state looks right. This is store
    // state (rendered by the status-bar `StructuralOpSegment`, spec §7.7.4),
    // not a notification — `raised` stays empty throughout.
    const { store, project, raised } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> one.alpha\n",
    });
    const state = store.getState();

    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });

    // No timer/microtask has been allowed to run yet — this is still the
    // original synchronous call stack.
    expect(store.getState().structuralOpPending).toBe("Move alpha to two");
    expect(raised).toHaveLength(0);
    // ...and the heavy call has NOT run yet: the file is untouched.
    expect(project.getSession().getFileSource("main.ink")).toContain("=== one ===");
    expect(stitchOrder(project.getSession().getFileSource("main.ink")!)).toEqual([
      "alpha",
      "beta",
    ]);

    await vi.runAllTimersAsync();
    await pending;

    // Now the deferred call has landed: `alpha` moved out from under `one`
    // (whose only remaining stitch is `beta`) into `two`. `stitchOrder` scans
    // the whole file, so this is "beta, then alpha" — `two`'s header still
    // follows `one`'s in the file — not "just beta"; the moveStitch test
    // above pins the same move with `indexOf` position checks instead.
    expect(stitchOrder(project.getSession().getFileSource("main.ink")!)).toEqual([
      "beta",
      "alpha",
    ]);
    expect(project.getSession().getFileSource("other.ink")).toContain("-> two.alpha");
    // The busy state clears once the deferred call settles.
    expect(store.getState().structuralOpPending).toBeNull();
  });

  it("does not drop a queued move when an unrelated edit lands on the SAME file while it is pending", async () => {
    // Finding from #2769's review: an earlier version of this fix re-checked
    // `session.generation` (bumped by every content-mutating call) and
    // dropped the queued op on ANY change, not just a change that actually
    // made the queued op stale. That guarded no real hazard, because
    // `compute` calls the wasm op fresh — against whatever the session's
    // live source is when the deferred call actually runs, never against a
    // snapshot captured before the idle wait — and the op refuses cleanly on
    // its own if its target has genuinely moved out from under it (see the
    // "a refused structural op applies nothing" describe block above). So an
    // edit that does not touch the move's own knots/stitches must NOT cause
    // the move to be silently skipped; it must land, alongside the
    // concurrent edit, exactly as if both happened in the source order they
    // actually occurred in.
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const session = project.getSession();
    const beforeMove = session.getFileSource("main.ink")!;

    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });

    // A concurrent edit lands before the deferred call fires — any
    // content-mutating wasm call bumps `generation`, which is exactly what
    // the removed check reacted to.
    session.updateFile("main.ink", `${beforeMove}\n// a concurrent edit\n`);

    await vi.runAllTimersAsync();
    await pending;

    // The queued move landed (source recomputed live, not dropped), AND the
    // concurrent edit's trailing comment survived — neither clobbered the
    // other, because the move op ran against the file's THEN-current source.
    const after = session.getFileSource("main.ink")!;
    expect(after).toContain("// a concurrent edit");
    expect(after.indexOf("= alpha")).toBeGreaterThan(after.indexOf("=== two ==="));
    expect(after.indexOf("= beta")).toBeLessThan(after.indexOf("=== two ==="));
  });

  it("does not drop a queued move on a keystroke-shaped generation bump (openDocument/updateDocument) in a different file", async () => {
    // The false-drop path the review flagged is not limited to `updateFile`:
    // `ink-editor`'s `elementTypeField` calls `handle.pushSource()` — which
    // goes through `EditorSessionHandle.updateDocument` — on every CodeMirror
    // transaction in ANY mounted editor view, bumping `generation` on every
    // single keystroke anywhere in the project, not just on the file the
    // pending move targets. Exercise that exact call shape directly (rather
    // than through the editor package) on a THIRD file the move never
    // touches, and confirm the move still applies.
    const { store, project } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> one.alpha\n",
      "scratch.ink": "// untouched\n",
    });
    const state = store.getState();
    const session = project.getSession();

    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });

    // A keystroke-shaped bump: open a document handle and push new content,
    // exactly the `updateDocument` call `pushSource` makes — on a file
    // wholly unrelated to the pending move.
    const doc = session.openDocument("scratch.ink");
    expect(doc).not.toBeNull();
    session.updateDocument(doc!, "// untouched\n// a keystroke landed elsewhere\n");

    await vi.runAllTimersAsync();
    await pending;

    // The move still landed...
    const mainSrc = session.getFileSource("main.ink")!;
    expect(mainSrc.indexOf("= alpha")).toBeGreaterThan(mainSrc.indexOf("=== two ==="));
    expect(mainSrc.indexOf("= beta")).toBeLessThan(mainSrc.indexOf("=== two ==="));
    expect(session.getFileSource("other.ink")).toContain("-> two.alpha");
    // ...and the unrelated keystroke's content was not reverted either.
    expect(session.getFileSource("scratch.ink")).toBe(
      "// untouched\n// a keystroke landed elsewhere\n",
    );
  });

  it("reorderStitch (a non-gated op) runs synchronously with no pending busy-state", async () => {
    // Control case: reorders skip the gate entirely (StructuralResult::
    // safe_source), so they must NOT go through the idle-deferred path —
    // no pending state, no idle hop, the file changes before `await`
    // resolves anything beyond the dispatcher's own promise.
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderStitch",
      path: "main.ink",
      knot: "one",
      stitch: "alpha",
      direction: 1,
    });

    expect(stitchOrder(project.getSession().getFileSource("main.ink")!)).toEqual([
      "beta",
      "alpha",
    ]);
    // Never set — reorderStitch never calls runGatedStructuralOp at all.
    expect(store.getState().structuralOpPending).toBeNull();
    // Only applyMoveResult's own success toast, no pending affordance.
    expect(raised.every((n) => n.severity === "info" && !n.message.endsWith("…"))).toBe(true);
  });
});

describe("structuralOpPending compare-and-clear (#2794)", () => {
  // `packages/brink-studio/src/__tests__/structural-op-pending-overlap.test.ts`
  // exercises a genuinely concurrent overlap between the two writers (real
  // time separation, via a controllable idle-queue mock) and is the primary
  // regression coverage for the race itself. These two tests instead pin the
  // pieces that make that fix correct without needing to fight fake-timer
  // batching: the primitive's exact semantics, and that each production
  // writer is actually wired to it (not to the old unconditional clear).

  it("clearStructuralOpPending only clears when the live value still equals the description this call set", () => {
    // Direct simulation of the hazardous sequence, at the primitive level:
    // op A sets "A"; op B starts before A finishes and overwrites to "B"; A's
    // own clear (called from ITS finally) must be a no-op since "A" is no
    // longer live; B's own clear (from ITS finally) must apply, since "B"
    // still is. Before #2794 there was no `clearStructuralOpPending` at
    // all — every caller called `setStructuralOpPending(null)` unconditionally,
    // which this sequence would have made a no-op on nothing (there is no
    // unconditional-clear equivalent left to call), i.e. this test could not
    // even be written against the pre-fix shape.
    const store = createStudioStore();
    store.getState().setStructuralOpPending("A");
    store.getState().setStructuralOpPending("B");

    store.getState().clearStructuralOpPending("A");
    expect(store.getState().structuralOpPending).toBe("B");

    store.getState().clearStructuralOpPending("B");
    expect(store.getState().structuralOpPending).toBeNull();
  });

  it("clearing a description that was never (or is no longer) live is a no-op", () => {
    const store = createStudioStore();
    store.getState().setStructuralOpPending("A");
    store.getState().clearStructuralOpPending("something else entirely");
    expect(store.getState().structuralOpPending).toBe("A");

    // Nothing pending at all — clearing whatever description is still a
    // harmless no-op, not an error.
    const empty = createStudioStore();
    empty.getState().clearStructuralOpPending("A");
    expect(empty.getState().structuralOpPending).toBeNull();
  });

  it("runGatedStructuralOp (symbol-menu ops) clears via clearStructuralOpPending with its own description", async () => {
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const clearSpy = vi.spyOn(state, "clearStructuralOpPending");
    const setSpy = vi.spyOn(state, "setStructuralOpPending");

    const pending = dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });
    await vi.runAllTimersAsync();
    await pending;

    expect(project.getSession().getFileSource("main.ink")).toBeDefined();
    expect(setSpy).toHaveBeenCalledExactlyOnceWith("Move alpha to two");
    // Not `setStructuralOpPending(null)` in a `finally` — that unconditional
    // shape is exactly the last-writer-wins race #2794 fixed.
    expect(clearSpy).toHaveBeenCalledExactlyOnceWith("Move alpha to two");
  });

  it("runGatedStructuralOp swallows a destroy()-during-defer race instead of reaching a freed session (#2794 follow-up)", async () => {
    // Mirrors `project-session-destroy.test.ts`'s first case, one layer up:
    // before this follow-up, `runGatedStructuralOp` rolled its own bare
    // `scheduleIdleWork` yield entirely outside `ProjectSession`, so
    // `destroy()` landing inside the idle window could still reach
    // `compute()`'s captured (now-freed) `session` handle — the same hazard
    // #2794 closed for `renameFile`, just less contained: the dispatch is
    // fire-and-forget `void dispatchSymbolAction(...)`
    // (`useSymbolMenuActions.ts`, `Binder.tsx`), so an uncaught rejection
    // here would be an unhandled promise rejection with no catch, not
    // `applyRename`'s caught-and-notified one.
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const session = project.getSession();
    const moveSpy = vi.spyOn(session, "moveStitch");
    const applyMoveResultSpy = vi.spyOn(state, "applyMoveResult");

    const pending = dispatchSymbolAction(state, applyMoveResultSpy, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });

    // destroy() lands INSIDE the idle window — before the deferred callback
    // (a scheduleIdleWork/setTimeout(...,0) under fake timers) has fired.
    project.destroy();

    // The void dispatch resolves quietly instead of rejecting — nothing
    // upstream has a catch for it — and `applyMoveResult` never runs, since
    // the swallowed refusal's `ok` is false.
    await expect(pending).resolves.toBeUndefined();
    expect(moveSpy).not.toHaveBeenCalled();
    expect(applyMoveResultSpy).not.toHaveBeenCalled();
    // The `finally` in `runGatedStructuralOp` still clears the pending
    // indicator even though the op itself never ran.
    expect(store.getState().structuralOpPending).toBeNull();
  });

  it("applyRename (Binder rename/move) clears via clearStructuralOpPending with its own description", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": "-> END\n", "lib.ink": "-> END\n" });
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const state = store.getState();
    const clearSpy = vi.spyOn(state, "clearStructuralOpPending");
    const setSpy = vi.spyOn(state, "setStructuralOpPending");

    const pending = state.renameFile("lib.ink", "util.ink");
    await vi.runAllTimersAsync();
    await pending;

    expect(setSpy).toHaveBeenCalledExactlyOnceWith("Renaming lib.ink → util.ink");
    expect(clearSpy).toHaveBeenCalledExactlyOnceWith("Renaming lib.ink → util.ink");
  });
});

/**
 * The two remaining ops #2577 added, driven through the real `@brink-lang/web`
 * wrapper (`ProjectSession.getSession()`) rather than the mock class directly,
 * so the wrapper's own JSON parse + typing is in the loop.
 *
 * `renameDir` has no studio consumer yet — the Binder's folder rename does not
 * call it — so this is the op's first exercise from TypeScript at all; the
 * `DirMoveResult` shape it answers is the one PR #2573 generated into the
 * refusal fixture with nothing to compare against.
 */
describe("renameDir and resolveCodeAction through the wasm wrapper (#2577)", () => {
  it("renameDir relocates every file under the folder and re-points inbound INCLUDEs", async () => {
    const { project } = await makeStore({
      "main.ink": "INCLUDE chapters/one.ink\n-> one\n",
      "chapters/one.ink": "=== one ===\nHi.\n-> END\n",
    });

    const result = project.getSession().renameDir("chapters", "acts");

    expect(result.ok).toBe(true);
    expect(result.moved_files).toEqual([
      {
        old_path: "chapters/one.ink",
        new_path: "acts/one.ink",
        new_source: "=== one ===\nHi.\n-> END\n",
      },
    ]);
    expect(result.cross_file_edits).toEqual([
      { path: "main.ink", new_source: "INCLUDE acts/one.ink\n-> one\n" },
    ]);
    expect(result.safe).toBe(true);
    expect(result.introduced_diagnostics).toEqual([]);
  });

  it("renameDir trims trailing slashes off both prefixes, like the real op", async () => {
    const { project } = await makeStore({
      "main.ink": "INCLUDE chapters/one.ink\n-> one\n",
      "chapters/one.ink": "=== one ===\nHi.\n-> END\n",
    });

    // brink_ide::dir_rename::rename_dir trims trailing slashes off both
    // prefixes before matching (crates/internal/brink-ide/src/dir_rename.rs:
    // 123-124); a trailing slash on either side must still succeed here.
    const result = project.getSession().renameDir("chapters/", "acts/");

    expect(result.ok).toBe(true);
    expect(result.moved_files).toEqual([
      {
        old_path: "chapters/one.ink",
        new_path: "acts/one.ink",
        new_source: "=== one ===\nHi.\n-> END\n",
      },
    ]);
    expect(result.cross_file_edits).toEqual([
      { path: "main.ink", new_source: "INCLUDE acts/one.ink\n-> one\n" },
    ]);
  });

  it("renameDir refuses an empty folder in the real op's wording", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });

    const result = project.getSession().renameDir("ghost", "other");

    expect(result.ok).toBe(false);
    expect(result.error).toBe("no files found under directory 'ghost'");
    // A refusal still ships the whole payload — see structural-refusal-shape.
    expect(result.moved_files).toEqual([]);
    expect(result.safe).toBe(true);
  });

  it("resolveCodeAction resolves a SortKnots action against the active file", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });
    const session = project.getSession();
    session.setActiveFile("main.ink");

    // MAIN's knots are already in order, and the real op reports a rewrite
    // that changes nothing as a refusal, not as a successful no-op.
    const noop = session.resolveCodeAction({ action: "SortKnots" }, 0);
    expect(noop.ok).toBe(false);
    expect(noop.error).toBe("code action produced no change");

    const reversed = await makeStore({
      "main.ink": "=== zeta ===\nZ.\n\n=== alpha ===\nA.\n",
    });
    const other = reversed.project.getSession();
    other.setActiveFile("main.ink");

    const sorted = other.resolveCodeAction({ action: "SortKnots" }, 0);
    expect(sorted.ok).toBe(true);
    expect(sorted.path).toBe("main.ink");
    expect(knotOrder(sorted.new_source!)).toEqual(["alpha", "zeta"]);
  });

  it("resolveCodeAction refuses an action tag it does not know", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });
    const session = project.getSession();
    session.setActiveFile("main.ink");

    const result = session.resolveCodeAction({ action: "Nonsense" }, 0);

    expect(result.ok).toBe(false);
    expect(result.error).toBe("invalid code-action data: unknown variant `Nonsense`");
  });
});
