// @vitest-environment jsdom
/**
 * Pins #2517 (item 2 of the two gaps that issue records): app quit is the
 * THIRD teardown path for the autosave ticker, alongside project close and
 * reopen (`autosave-reopen.test.ts`, #2486), and it must clear
 * `autosaveTimer` too — before, not after, `awaitSaveAllBeforeQuit`'s
 * (up to ~3s) wait for the final save.
 *
 * This is a real bug fix, not just a coverage gap (see `handleQuitRequested`'s
 * doc comment in `main.tsx`): before this fix, `autosaveTimer` was left
 * armed for the full duration of the quit-time save-wait. An autosave tick
 * landing in that window would fire `dispatch("file.saveAll")` against a
 * project that's already mid-teardown — redundant alongside
 * `awaitSaveAllBeforeQuit`'s own dispatch/redispatch at best, a write racing
 * the window's destruction at worst. The 120s cadence made this rare in
 * practice (the save-wait caps at ~3s), but nothing prevented it, and the
 * structurally parallel `closeProject` already clears the same timer for
 * exactly this reason.
 *
 * `openProject`/`handleQuitRequested` are exported from `main.tsx` for
 * exactly this test (see the export comments there) — same rationale as
 * `autosave-reopen.test.ts`: driving the real async chain directly, rather
 * than the module's fire-and-forget `listen()` wiring.
 *
 * Fake timers only (house rule, same as `autosave-reopen.test.ts`): a real
 * 120s wait plus a real ~3s quit cap is not an option. `awaitSaveAllBeforeQuit`
 * (quit.ts) is NOT mocked — it's driven for real, polling/redispatching on
 * `setTimeout`, which fake timers intercept just like the autosave
 * `setInterval`; `vi.advanceTimersByTimeAsync` steps through both.
 *
 * `AUTOSAVE_MS` is pulled from the same dynamic `await import("../main.js")`
 * as `openProject`/`handleQuitRequested`, not a static top-level import: a
 * static import runs before this file's own top-level `class
 * FakeTauriFileProvider` declaration below it (ES import hoisting), which
 * fires the `../tauri-provider.js` mock factory — it references that class
 * — before the class exists, throwing a TDZ `ReferenceError` (see the same
 * note in `autosave-reopen.test.ts`).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// `getCurrentWindow()` is called more than once (module-load `onCloseRequested`
// wiring, then again inside `handleQuitRequested`) — a factory returning a
// fresh object each call would hand back a fresh `destroy` mock each time
// too, so `vi.hoisted` keeps one shared spy the test can assert call counts
// against, the same pattern used elsewhere in this package (e.g.
// `packages/wasm/src/__tests__/*.test.ts`).
const { destroyMock } = vi.hoisted(() => ({ destroyMock: vi.fn(() => Promise.resolve()) }));

// ── Tauri IPC surface: never exercised by this scenario, just needs to
// resolve without throwing so module import + the menu/window wiring at the
// bottom of main.tsx don't blow up. ──
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(vi.fn())) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: vi.fn(), onFocusChanged: vi.fn(), destroy: destroyMock }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

interface FakeApi {
  dispatch: ReturnType<typeof vi.fn>;
  getDirtyFiles: ReturnType<typeof vi.fn>;
}
const mountedApis: FakeApi[] = [];
const mountStudio = vi.fn((..._args: unknown[]) => {
  const index = mountedApis.length;
  // Always dirty — same rationale as autosave-reopen.test.ts: a still-alive
  // ticker (the unfixed bug) must be GUARANTEED to have something to
  // dispatch when it ticks, or "no extra dispatches" would prove nothing.
  const api: FakeApi = {
    dispatch: vi.fn(() => true),
    getDirtyFiles: vi.fn(() => [`dirty-${index}.ink`]),
  };
  mountedApis.push(api);
  return Promise.resolve({
    api: { ...api, notify: vi.fn(), select: vi.fn(), getStoryBytes: vi.fn(() => null) },
    entryFile: "main.ink",
    unmount: vi.fn(),
  });
});
vi.mock("@brink-lang/studio", () => ({ mountStudio: (...args: unknown[]) => mountStudio(...args) }));

class FakeTauriFileProvider {
  constructor(private readonly root: string) {}
  listFiles(): Promise<string[]> {
    return Promise.resolve(["main.ink"]);
  }
  readFile(_path: string): Promise<string> {
    return Promise.resolve("-> END\n");
  }
  ringBackups(): Promise<void> {
    return Promise.resolve();
  }
}
vi.mock("../tauri-provider.js", () => ({
  TauriFileProvider: FakeTauriFileProvider,
  pickProjectFolder: vi.fn(() => Promise.resolve(null)),
  projectAnchorExists: vi.fn(() => Promise.resolve(true)),
  readAppSettings: vi.fn(() => Promise.resolve({ reopenLastProject: false })),
  writeAppSettings: vi.fn(() => Promise.resolve()),
  previousExitClean: vi.fn(() => Promise.resolve(true)),
  pickProjectFile: vi.fn(() => Promise.resolve(null)),
  discoverProjectConfig: vi.fn(() => Promise.resolve(null)),
  createProject: vi.fn(() => Promise.resolve("")),
  pruneRecent: vi.fn(() => Promise.resolve([])),
  pushRecent: vi.fn(() => Promise.resolve([])),
  readRecents: vi.fn(() => Promise.resolve([])),
  saveBytesDialog: vi.fn(() => Promise.resolve(null)),
}));

describe("desktop autosave ticker is cleared on quit (#2517)", () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    mountedApis.length = 0;
    destroyMock.mockClear();
    vi.resetModules();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("clears the interval before awaiting the final save, so a tick can never fire against a project mid-quit", async () => {
    const { openProject, handleQuitRequested, AUTOSAVE_MS } = await import("../main.js");

    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");
    const clearIntervalSpy = vi.spyOn(globalThis, "clearInterval");

    await openProject("/projects/a");
    expect(mountedApis).toHaveLength(1);
    expect(setIntervalSpy).toHaveBeenCalledTimes(1);
    expect(clearIntervalSpy).toHaveBeenCalledTimes(0);

    const quitPromise = handleQuitRequested();
    // The clear must happen synchronously, before `handleQuitRequested`'s
    // first `await` yields — i.e. before any part of the (up to ~3s) save
    // wait has had a chance to run. If the clear were only reached AFTER
    // `awaitSaveAllBeforeQuit` (or omitted entirely), this assertion — made
    // before advancing the fake clock or awaiting anything — would still
    // see 0 calls here.
    expect(clearIntervalSpy).toHaveBeenCalledTimes(1);

    // Drive `awaitSaveAllBeforeQuit`'s real poll/redispatch loop (quit.ts,
    // #2434) to completion — dirty stays non-empty for the whole run, so it
    // redispatches until the ~3s cap, then `handleQuitRequested` destroys
    // the window.
    await vi.advanceTimersByTimeAsync(3200);
    await quitPromise;

    expect(destroyMock).toHaveBeenCalledTimes(1);

    const [apiA] = mountedApis;
    expect(apiA).toBeDefined();
    // `awaitSaveAllBeforeQuit`'s own legitimate dispatch/redispatch calls
    // during the save-wait — unrelated to the autosave ticker, snapshot so
    // the assertion below is about calls AFTER quit completed.
    const dispatchCallsAtQuit = apiA?.dispatch.mock.calls.length ?? -1;
    expect(dispatchCallsAtQuit).toBeGreaterThanOrEqual(1);

    // The regression this test exists for: an interval left armed through
    // teardown, still holding a reference to project A's `StudioApi` and
    // still seeing it as dirty, would rack up additional
    // `dispatch("file.saveAll")` calls here.
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 3);
    expect(apiA?.dispatch.mock.calls.length).toBe(dispatchCallsAtQuit);
  });

  it("destroys the window even when no project is open", async () => {
    const { handleQuitRequested } = await import("../main.js");
    await handleQuitRequested();
    expect(destroyMock).toHaveBeenCalledTimes(1);
  });
});
