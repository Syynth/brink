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
 * `renameFile` (see `deferForGatedCall`'s doc comment in
 * `project-session.ts`).
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
