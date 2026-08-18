/**
 * `ProjectSession.destroy()` must abort a gated call still waiting on its
 * `scheduleIdleWork` yield, not merely discard the handle (issue #2794 — a
 * gap #2788's adversarial re-review found in the paint-path-defer family:
 * "the enrolment family's gap, not this PR's").
 *
 * Before this fix, `renameFile`'s `await new Promise((resolve) =>
 * scheduleIdleWork(resolve))` had no `this.destroyed` check afterward, and
 * the idle handle was never `cancelIdleWork`'d on teardown. An unmount
 * landing inside the ≤300ms idle window left the scheduled callback to fire
 * anyway and go on to call `this.session.renameFile(...)` — a wasm handle
 * `destroy()` had already freed. That was CONTAINED, not unreachable: the
 * throw is caught by `applyRename` (`studio-store`'s `binder.ts`) and
 * surfaces as an error notification — but containment is not a fix, and the
 * same shape applies to every future gated call this class defers, not just
 * `renameFile` (see `deferGatedCall`'s doc comment in
 * `project-session.ts`). #2794's follow-up review found that gap still open
 * in `studio-ui`'s `runGatedStructuralOp` (the symbol-menu `moveStitch`/
 * `promoteStitch`/`demoteKnot` ops), which rolled its own bare
 * `scheduleIdleWork` yield instead of this guard — fixed by switching it to
 * `deferGatedCall` (now public for exactly this reuse) and adding coverage in
 * `packages/brink-studio/src/__tests__/symbol-structural-ops.test.ts`
 * ("runGatedStructuralOp swallows a destroy()-during-defer race").
 *
 * Uses a hand-built stub `session` (not a real `EditorSessionHandle`).
 * `vitest.config.ts`'s `resolve.alias` repoints `@brink-lang/web` at a local
 * do-nothing stub for this package's suite (see that file's header) so this
 * needs no built `crates/brink-web/www/pkg` — the fix under test lives
 * entirely in `ProjectSession`'s bookkeeping around the `scheduleIdleWork`
 * yield, never in the wasm op itself. Imports `ProjectSession` directly from
 * `../project-session.js`, not through the package's `index.ts` barrel.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ProjectSession } from "../project-session.js";
import { InMemoryFileProvider } from "../provider.js";

/** A minimal stub satisfying every `session` call `ProjectSession` makes
 *  across `initialize()`/`renameFile()`/`destroy()`. Cast past structural
 *  typing (`as unknown as EditorSessionHandle`) at the call site — the real
 *  class has many more members this suite has no need to fake. */
function makeStubSession(renameFile = vi.fn(() => ({ ok: true, new_source: "renamed content", cross_file_edits: [] }))) {
  return {
    generation: 0,
    updateFile: vi.fn(),
    removeFile: vi.fn(),
    getFileSource: vi.fn(() => null),
    discoverProjectConfig: vi.fn(() => []),
    getFileIncludes: vi.fn(() => []),
    listFiles: vi.fn(() => []),
    renameFile,
    compileProject: vi.fn(),
    free: vi.fn(),
  };
}

type StubSession = ReturnType<typeof makeStubSession>;

function makeProjectSession(session: StubSession) {
  const provider = new InMemoryFileProvider({ "main.ink": "-> END\n", "lib.ink": "-> END\n" });
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
    session: session as any,
  });
  return { project, provider };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("ProjectSession.destroy() during a deferred gated call (#2794)", () => {
  it("rejects the deferred renameFile instead of ever reaching the freed session, when destroy() lands inside the idle window", async () => {
    const session = makeStubSession();
    const { project } = makeProjectSession(session);

    const pending = project.renameFile("lib.ink", "main2.ink");
    // Attach a rejection handler synchronously so Node/Vitest never sees an
    // unhandled rejection while destroy() settles it below.
    pending.catch(() => {});

    // destroy() lands INSIDE the idle window — before the scheduleIdleWork
    // callback (a setTimeout(...,0) under fake timers/jsdom) has fired.
    project.destroy();

    await expect(pending).rejects.toThrow(/destroyed/i);
    // The deferred call must never have reached the (now freed) session.
    expect(session.renameFile).not.toHaveBeenCalled();
    expect(session.free).toHaveBeenCalledOnce();
  });

  it("cancels the idle handle on destroy() — no dangling timer left armed", async () => {
    const session = makeStubSession();
    const { project } = makeProjectSession(session);

    const pending = project.renameFile("lib.ink", "main2.ink");
    pending.catch(() => {});

    expect(vi.getTimerCount()).toBeGreaterThan(0);
    project.destroy();

    // cancelIdleWork must have actually cancelled the handle, not merely
    // stopped caring about it — otherwise the callback still fires later
    // and (absent the reject-on-destroy fix above) would still try to touch
    // the freed session.
    expect(vi.getTimerCount()).toBe(0);

    // Running whatever timers remain (there should be none) must not call
    // into the freed session either way.
    await vi.runAllTimersAsync();
    expect(session.renameFile).not.toHaveBeenCalled();
  });

  it("a renameFile call started AFTER destroy() also rejects, without scheduling a new idle callback", async () => {
    const session = makeStubSession();
    const { project } = makeProjectSession(session);

    project.destroy();
    const timersBefore = vi.getTimerCount();

    const pending = project.renameFile("lib.ink", "main2.ink");
    await expect(pending).rejects.toThrow(/destroyed/i);
    expect(vi.getTimerCount()).toBe(timersBefore); // nothing new scheduled
    expect(session.renameFile).not.toHaveBeenCalled();
  });

  it("a renameFile call that settles BEFORE destroy() is unaffected — the guard only fires on a genuine race", async () => {
    const session = makeStubSession();
    const { project } = makeProjectSession(session);

    const pending = project.renameFile("lib.ink", "main2.ink");
    await vi.runAllTimersAsync();
    await expect(pending).resolves.toEqual([]);
    expect(session.renameFile).toHaveBeenCalledExactlyOnceWith("lib.ink", "main2.ink");

    // destroy() afterward is the ordinary teardown path — no pending work
    // left to reject, so it must not throw or otherwise misbehave.
    expect(() => project.destroy()).not.toThrow();
  });
});

/**
 * `ProjectSession.destroy()` during the LARGER window that opens right
 * after `deferGatedCall`'s idle yield settles — issue #2802.
 *
 * #2794/#2798 (above) close only the ≤300ms `scheduleIdleWork` window.
 * Every method here goes on to `await` the host provider itself (Tauri IPC
 * — unbounded, and typically far longer than that window), then resumes
 * touching `this.session`/`this.changes` with no re-check. `destroy()`
 * cannot reject that await — it is not a tracked idle handle — so a
 * teardown landing during it used to reach a freed wasm handle one `await`
 * later: the exact use-after-free #2794 set out to close, unclosed for this
 * shape.
 *
 * Each case below uses a stub provider whose relevant method resolves
 * *strictly after* `destroy()` has already run, so the assertions actually
 * exercise the post-await continuation — not merely the idle-yield window
 * the #2794 suite above already covers.
 */
describe("ProjectSession.destroy() during the post-host-IO-await window (#2802)", () => {
  /** A promise this test controls the settlement of, so `destroy()` can be
   *  made to land strictly before the provider call resolves. */
  function makeDeferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((res) => {
      resolve = res;
    });
    return { promise, resolve };
  }

  /** Advance past whatever microtask hops separate "call the async method"
   *  from "it is now suspended on the provider await under test" — plain
   *  `Promise` continuations, unaffected by `vi.useFakeTimers()` (which
   *  fakes timers, not microtasks). */
  async function flushMicrotasks(times = 5): Promise<void> {
    for (let i = 0; i < times; i++) {
      await Promise.resolve();
    }
  }

  it("renameFile: destroy() landing during the provider.renameFile await stops the post-await continuation from touching the freed session", async () => {
    const session = makeStubSession();
    const deferred = makeDeferred<void>();
    const provider = new InMemoryFileProvider({ "main.ink": "-> END\n", "lib.ink": "-> END\n" });
    // Overwrite renameFile with one this test controls the settlement of —
    // resolves strictly AFTER destroy() runs below.
    provider.renameFile = vi.fn(() => deferred.promise);
    const project = new ProjectSession({
      provider,
      entryFile: "main.ink",
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
      session: session as any,
    });

    // A project-config-named destination so the post-await continuation
    // would (absent the fix) call `applyProjectConfig` -> `discoverProjectConfig`
    // on the freed session — the exact hazard the issue names.
    const pending = project.renameFile("lib.ink", "brink.toml");
    pending.catch(() => {});

    // Clear the idle-yield window first (#2794's guard, already proven
    // above) so this test lands squarely in the NEW window.
    await vi.runAllTimersAsync();
    await flushMicrotasks();

    // The session-level rename already ran (it's synchronous, before the
    // provider await) — that part is not the hazard under test.
    expect(session.renameFile).toHaveBeenCalledOnce();

    project.destroy();
    expect(session.free).toHaveBeenCalledOnce();

    // Only now does the host IPC "complete" — strictly after destroy().
    deferred.resolve();

    await expect(pending).rejects.toThrow(/destroyed/i);
    // The post-await continuation must never have reached the freed
    // session: no `discoverProjectConfig` (applyProjectConfig's call),
    // and no further `getFileSource`/`updateFile` beyond what already ran
    // synchronously before the provider await.
    expect(session.discoverProjectConfig).not.toHaveBeenCalled();
  });

  it("deleteFile: destroy() landing during the provider.deleteFile await stops removeFile from reaching the freed session", async () => {
    const session = makeStubSession();
    const deferred = makeDeferred<void>();
    const provider = new InMemoryFileProvider({ "main.ink": "-> END\n", "lib.ink": "-> END\n" });
    provider.deleteFile = vi.fn(() => deferred.promise);
    const project = new ProjectSession({
      provider,
      entryFile: "main.ink",
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
      session: session as any,
    });

    const pending = project.deleteFile("lib.ink");
    pending.catch(() => {});
    await flushMicrotasks();

    project.destroy();
    deferred.resolve();

    await expect(pending).rejects.toThrow(/destroyed/i);
    expect(session.removeFile).not.toHaveBeenCalled();
  });

  it("requestFile: destroy() landing during the provider.requestFile await stops updateFile from reaching the freed session", async () => {
    const session = makeStubSession();
    const deferred = makeDeferred<string | null>();
    const provider = new InMemoryFileProvider({ "main.ink": "-> END\n" });
    provider.requestFile = vi.fn(() => deferred.promise);
    const project = new ProjectSession({
      provider,
      entryFile: "main.ink",
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
      session: session as any,
    });

    const pending = project.requestFile("newly-included.ink");
    pending.catch(() => {});
    await flushMicrotasks();

    project.destroy();
    deferred.resolve("late content");

    await expect(pending).rejects.toThrow(/destroyed/i);
    expect(session.updateFile).not.toHaveBeenCalled();
  });

  it("resolveIncludes (via refreshIncludes): destroy() landing during the provider.requestFile await stops updateFile from reaching the freed session", async () => {
    const deferred = makeDeferred<string | null>();
    const session = {
      generation: 0,
      updateFile: vi.fn(),
      removeFile: vi.fn(),
      getFileSource: vi.fn(() => null),
      discoverProjectConfig: vi.fn(() => []),
      getFileIncludes: vi.fn(() => [{ loaded: false, resolved: "included.ink" }]),
      listFiles: vi.fn(() => [{ path: "main.ink", mounted: false }]),
      renameFile: vi.fn(),
      compileProject: vi.fn(),
      free: vi.fn(),
    };
    const provider = new InMemoryFileProvider({ "main.ink": "-> END\n" });
    provider.requestFile = vi.fn(() => deferred.promise);
    const project = new ProjectSession({
      provider,
      entryFile: "main.ink",
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
      session: session as any,
    });

    const pending = project.refreshIncludes();
    pending.catch(() => {});
    await flushMicrotasks();

    project.destroy();
    deferred.resolve("included content");

    await expect(pending).rejects.toThrow(/destroyed/i);
    expect(session.updateFile).not.toHaveBeenCalled();
  });

  it("initialize: destroy() landing during the provider.readFile await stops the file-load loop from touching the freed session", async () => {
    const deferred = makeDeferred<string>();
    const session = makeStubSession();
    const provider = new InMemoryFileProvider();
    provider.listFiles = vi.fn(async () => ["main.ink"]);
    provider.readFile = vi.fn(() => deferred.promise);
    const project = new ProjectSession({
      provider,
      entryFile: "main.ink",
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
      session: session as any,
    });

    const pending = project.initialize();
    pending.catch(() => {});
    await flushMicrotasks();
    expect(provider.readFile).toHaveBeenCalledWith("main.ink");

    project.destroy();
    deferred.resolve("main content");

    await expect(pending).rejects.toThrow(/destroyed/i);
    expect(session.updateFile).not.toHaveBeenCalled();
  });
});
