/**
 * File-rename/move tests (#164 Stage 3, PR B): ProjectSession.renameFile
 * (session re-key + provider rename/fallback + INCLUDE-referrer rewrite +
 * created/deleted/modified egress) and the store's renameFile / moveFile
 * actions with inverse-rename undo.
 *
 * Runs against the brink-web mock, whose `rename_file` returns a MoveResult
 * with the moved content verbatim plus a cross_file_edit for any file whose
 * INCLUDE names the old basename — enough to exercise the apply/egress
 * plumbing. (The real inbound/outbound INCLUDE math is covered by Rust unit
 * tests in brink-ide.)
 *
 * `ProjectSession.renameFile` runs off the paint path (#2776, spec §7.7.4):
 * it yields via `scheduleIdleWork` before calling the gated wasm op, so
 * under this file's `vi.useFakeTimers()` every call below must let the
 * pending timer run (`await vi.runAllTimersAsync()`) before awaiting the
 * returned promise — otherwise it never settles. The paint-path ordering
 * itself (busy-state committed before the deferred call runs) is pinned in
 * `symbol-structural-ops.test.ts`'s "run off the paint path" describe block
 * and this file's own "renames off the paint path" block below; these tests
 * cover the rename/move/undo *behavior*, not that ordering.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession, type FileChange } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";

const MAIN = "INCLUDE lib.ink\n-> start\n=== start ===\nHello.\n-> END\n";
const MAIN_RENAMED = "INCLUDE util.ink\n-> start\n=== start ===\nHello.\n-> END\n";
const LIB = "=== helper ===\n-> END\n";

interface Egress {
  provider: InMemoryFileProvider;
  project: ProjectSession;
  batches: FileChange[][];
}

async function makeProject(
  files: Record<string, string> = { "main.ink": MAIN, "lib.ink": LIB },
): Promise<Egress> {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const batches: FileChange[][] = [];
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    onFilesChanged: (changes) => batches.push(changes),
  });
  await project.initialize();
  return { provider, project, batches };
}

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * Every rename/move/undo call below is blocked on `ProjectSession.renameFile`'s
 * `scheduleIdleWork` yield (#2776) — under this file's fake timers that yield
 * never fires on its own, so `pending` would hang forever without this. Runs
 * every due timer (looping until none remain, so a batch like `moveFiles`'
 * per-file re-scheduling still drains) and then awaits the now-settled
 * result — a promise rejection included, so `.rejects` matchers compose with
 * this normally.
 */
async function settleRename<T>(pending: Promise<T>): Promise<T> {
  // Attach a handler to `pending` SYNCHRONOUSLY (before the first `await`
  // below) so a rejection settled while `runAllTimersAsync` is draining
  // timers is never briefly unobserved — Node/Vitest flags that window as
  // an unhandled rejection even though `return pending` below does
  // eventually attach its own handler once this function is awaited.
  pending.catch(() => {});
  await vi.runAllTimersAsync();
  return pending;
}

// ── ProjectSession.renameFile ───────────────────────────────────────

describe("ProjectSession.renameFile", () => {
  it("re-keys the file, rewrites referrers, and emits created/deleted/modified", async () => {
    const { provider, project, batches } = await makeProject();

    const result = await settleRename(project.renameFile("lib.ink", "util.ink"));
    expect(result).toEqual({ referrers: ["main.ink"], safe: true, introducedDiagnostics: [] });

    const session = project.getSession();
    expect(session.getFileSource("util.ink")).toBe(LIB);
    expect(session.getFileSource("lib.ink")).toBeNull();
    expect(project.getFiles()["main.ink"]).toBe(MAIN_RENAMED);
    expect(await provider.requestFile("util.ink")).toBe(LIB);
    expect(await provider.requestFile("lib.ink")).toBeNull();

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([
      [
        { path: "lib.ink", type: "deleted" },
        { path: "main.ink", type: "modified", content: MAIN_RENAMED },
        { path: "util.ink", type: "created", content: LIB },
      ],
    ]);
  });

  it("falls back to create+delete when the provider lacks renameFile", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN, "lib.ink": LIB });
    (provider as { renameFile?: unknown }).renameFile = undefined;
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();

    await settleRename(project.renameFile("lib.ink", "util.ink"));
    expect(await provider.requestFile("util.ink")).toBe(LIB);
    expect(await provider.requestFile("lib.ink")).toBeNull();
  });

  it("canRenameFiles reflects provider capability", async () => {
    const { project } = await makeProject();
    expect(project.canRenameFiles()).toBe(true);
  });

  it("throws when the destination already exists", async () => {
    const { project } = await makeProject();
    await expect(settleRename(project.renameFile("lib.ink", "main.ink"))).rejects.toThrow();
  });
});

// ── Store renameFile / moveFile + undo ──────────────────────────────

describe("store.renameFile", () => {
  it("re-keys the open tabs in place, renames, and undo restores the original path + includes", async () => {
    const { project } = await makeProject();
    const renameDoc = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _renameDocPath: renameDoc,
    });

    await settleRename(store.getState().renameFile("lib.ink", "util.ink"));

    expect(renameDoc).toHaveBeenCalledWith("lib.ink", "util.ink");
    const session = project.getSession();
    expect(session.getFileSource("util.ink")).toBe(LIB);
    expect(session.getFileSource("lib.ink")).toBeNull();
    expect(project.getFiles()["main.ink"]).toBe(MAIN_RENAMED);
    expect(store.getState().undoStack).toHaveLength(1);
    expect(store.getState().undoStack[0]!.kind).toBe("rename");

    // Undo is the inverse rename — file back at lib.ink, INCLUDE restored.
    // (Undo replays through the same `applyRename` helper, so it is blocked
    // on the same deferred wasm call and needs the same timer-settle.)
    await settleRename(store.getState().undo());
    expect(session.getFileSource("lib.ink")).toBe(LIB);
    expect(session.getFileSource("util.ink")).toBeNull();
    expect(project.getFiles()["main.ink"]).toBe(MAIN);
    expect(store.getState().undoStack).toHaveLength(0);
  });

  it("a failed rename (name collision) leaves the open tabs and file intact", async () => {
    const { project } = await makeProject();
    const renameDoc = vi.fn();
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _renameDocPath: renameDoc,
      _notify: notify,
    });

    // lib.ink → main.ink collides; the rename must fail without side effects.
    await settleRename(store.getState().renameFile("lib.ink", "main.ink"));

    expect(renameDoc).not.toHaveBeenCalled(); // tabs untouched
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ severity: "error" }));
    expect(store.getState().undoStack).toHaveLength(0);
    const session = project.getSession();
    expect(session.getFileSource("lib.ink")).toBe(LIB); // file still there
    expect(project.getFiles()["main.ink"]).toBe(MAIN); // referrer untouched
  });

  it("moveFile relocates into a folder and round-trips on undo", async () => {
    const { project } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    await settleRename(store.getState().moveFile("lib.ink", "scenes/lib.ink"));
    const session = project.getSession();
    expect(session.getFileSource("scenes/lib.ink")).toBe(LIB);
    expect(session.getFileSource("lib.ink")).toBeNull();
    // (Directory-aware INCLUDE rewriting is verified by the Rust op tests; the
    // mock only rewrites by basename, so the move's referrer edit is a no-op
    // here — this test covers the session re-key + undo round-trip.)

    await settleRename(store.getState().undo());
    expect(session.getFileSource("lib.ink")).toBe(LIB);
    expect(session.getFileSource("scenes/lib.ink")).toBeNull();
    expect(store.getState().undoStack).toHaveLength(0);
  });
});

describe("store.moveFiles (batch)", () => {
  const A = "=== a ===\n-> END\n";
  const B = "=== b ===\n-> END\n";

  it("moves several files into a folder as one undoable step", async () => {
    const { project } = await makeProject({
      "main.ink": "INCLUDE a.ink\nINCLUDE b.ink\n-> END\n",
      "a.ink": A,
      "b.ink": B,
    });
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const session = project.getSession();

    await settleRename(store.getState().moveFiles(["a.ink", "b.ink"], "scenes/"));

    expect(session.getFileSource("scenes/a.ink")).toBe(A);
    expect(session.getFileSource("scenes/b.ink")).toBe(B);
    expect(session.getFileSource("a.ink")).toBeNull();
    expect(session.getFileSource("b.ink")).toBeNull();
    expect(store.getState().undoStack).toHaveLength(1); // one batch entry

    // One undo restores both.
    await settleRename(store.getState().undo());
    expect(session.getFileSource("a.ink")).toBe(A);
    expect(session.getFileSource("b.ink")).toBe(B);
    expect(session.getFileSource("scenes/a.ink")).toBeNull();
    expect(store.getState().undoStack).toHaveLength(0);
  });

  it("skips a colliding file but moves the rest, still one undo", async () => {
    const { project } = await makeProject({
      "main.ink": "-> END\n",
      "a.ink": A,
      "scenes/a.ink": "=== existing ===\n-> END\n",
      "b.ink": B,
    });
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments(), _notify: vi.fn() });
    const session = project.getSession();

    // a.ink → scenes/a.ink collides; b.ink → scenes/b.ink succeeds.
    await settleRename(store.getState().moveFiles(["a.ink", "b.ink"], "scenes/"));

    expect(session.getFileSource("a.ink")).toBe(A); // collision: stayed put
    expect(session.getFileSource("scenes/b.ink")).toBe(B); // moved
    expect(store.getState().undoStack).toHaveLength(1); // the one success, batched
  });
});

// ── Off the paint path (#2776) ───────────────────────────────────────

describe("store.renameFile runs off the paint path (#2776)", () => {
  it("commits the pending busy-state synchronously, before the deferred wasm call runs", async () => {
    // Same load-bearing property `symbol-structural-ops.test.ts` pins for
    // moveStitch/promoteStitch/demoteKnot (#2767): the paint-worthy state
    // change must land in the SAME synchronous tick as the triggering call,
    // before the gated wasm rename (deferred to the next idle slot) ever
    // runs. Checking `structuralOpPending` before any timer/microtask has
    // been allowed to fire proves the ordering directly.
    const { project } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    const pending = store.getState().renameFile("lib.ink", "util.ink");

    expect(store.getState().structuralOpPending).toBe("Renaming lib.ink → util.ink");
    // The heavy call has NOT run yet: the file is untouched.
    expect(project.getSession().getFileSource("lib.ink")).toBe(LIB);
    expect(project.getSession().getFileSource("util.ink")).toBeNull();

    await settleRename(pending);

    expect(project.getSession().getFileSource("util.ink")).toBe(LIB);
    // The busy state clears once the deferred call settles.
    expect(store.getState().structuralOpPending).toBeNull();
  });

  it("clears the busy state on a refused rename too", async () => {
    const { project } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments(), _notify: vi.fn() });

    // lib.ink → main.ink collides — the deferred call refuses.
    await settleRename(store.getState().renameFile("lib.ink", "main.ink"));

    expect(store.getState().structuralOpPending).toBeNull();
  });

  it("does not drop a queued rename when an unrelated edit lands on a different file while it is pending", async () => {
    // Mirrors the #2769-review lesson `symbol-structural-ops.test.ts` pins
    // for moveStitch: the wasm mock's `rename_file` reads `this.files` live
    // when the deferred call actually runs, never a snapshot captured before
    // the idle wait, so an edit to a file the rename does not touch must not
    // cause the queued rename to be dropped or corrupted.
    const { project } = await makeProject({
      "main.ink": MAIN,
      "lib.ink": LIB,
      "scratch.ink": "// untouched\n",
    });
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const session = project.getSession();

    const pending = store.getState().renameFile("lib.ink", "util.ink");

    // A concurrent, unrelated edit lands before the deferred call fires.
    session.updateFile("scratch.ink", "// a concurrent edit\n");

    await settleRename(pending);

    // The queued rename landed...
    expect(session.getFileSource("util.ink")).toBe(LIB);
    expect(session.getFileSource("lib.ink")).toBeNull();
    // ...and the unrelated edit was not reverted either.
    expect(session.getFileSource("scratch.ink")).toBe("// a concurrent edit\n");
  });
});
