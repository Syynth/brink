// @vitest-environment jsdom
/**
 * Pins #2486: reopening a project must not leave the PREVIOUS project's
 * autosave ticker running against the NEW one.
 *
 * `docs/desktop-shell-spec.md`'s autosave row documents that the ticker "is
 * armed when a project opens and cleared on project close or reopen" —
 * i.e. a reopen (`openProject` → `closeProject()` → mount the new project,
 * `main.tsx` ~151) never leaves two `setInterval`s alive at once. That
 * property rested entirely on `openProject` calling `closeProject()` first,
 * which cleared `autosaveTimer` (`main.tsx`'s `closeProject`) — nothing
 * pinned it (a refactor that dropped or reordered that call would silently
 * double the autosave cadence, data-loss-adjacent since the STALE timer
 * would hold a reference to the old project's `StudioHandle.api` and could
 * fire `dispatch("file.saveAll")` against it after that project's `unmount`
 * already ran).
 *
 * `openProject`/`closeProject` are exported from `main.tsx` for exactly this
 * test (see the export comments there) — driving them directly, rather than
 * hunting for the module's fire-and-forget `listen()` wiring, is the only
 * way to `await` the real async open/close chain instead of racing it.
 * Every dependency `main.tsx` touches (Tauri IPC, `mountStudio`, the file
 * provider) is mocked; this needs the jsdom environment (unlike the rest of
 * this package's `node`-environment suite, see `vitest.config.ts`) because
 * `openProject`/`closeProject`/`renderLanding` all read `document`.
 *
 * `AUTOSAVE_MS` is imported from `main.tsx` rather than restated here
 * (#2517) — a prior version of this file redeclared its own `120_000`
 * local, which meant a change to the production interval couldn't fail
 * anything in this file; the pinning test below asserts the imported
 * binding's exact value, the same one `openProject`'s `setInterval` call
 * reads. It's pulled via the same dynamic `await import("../main.js")`
 * every other test here already uses, not a static top-level import: a
 * static import executes before this file's OWN top-level `class
 * FakeTauriFileProvider` declaration below it (ES import hoisting runs
 * ahead of other module-body statements regardless of source position),
 * which would fire the `../tauri-provider.js` mock factory — it references
 * that class — before the class exists, throwing a TDZ `ReferenceError`.
 * The third teardown path — app quit — is NOT this file's concern; see
 * `autosave-quit.test.ts` for that (#2517).
 *
 * Fake timers only (house rule): a real 120s wait is not an option, and a
 * short-vs-short real-timer race is exactly the kind of CI-jitter-flaky
 * test this project has already been burned by (see `quit.test.ts`'s
 * redispatch-interval tests). `vi.useFakeTimers()` is installed before
 * either project opens, since `setInterval` is called synchronously inside
 * `openProject` — a timer armed on a real clock before fake timers are
 * installed would never be advanceable by `vi.advanceTimersByTime`.
 *
 * NOTE on the legitimate `dispatch("file.saveAll")` call(s) project A gets:
 * `closeProject` itself awaits a guarded save of anything dirty right
 * before `unmount()` (main.tsx's `closeProject` doc comment, #2444) — one
 * unconditional dispatch, plus redispatches while the dirty set persists
 * (this mock's `getDirtyFiles()` never clears, so the wait runs its full
 * ~3s cap and redispatches repeatedly). Real, correct, and unrelated to the
 * ticker. The assertions below snapshot the call count immediately after
 * the reopen/close settles, not "never called at all", so they isolate
 * "does the ticker fire again" from that unrelated close-time flush.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// ── Tauri IPC surface: never exercised by this scenario, just needs to
// resolve without throwing so module import + the menu/window wiring at the
// bottom of main.tsx don't blow up. ──
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(vi.fn())) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: vi.fn(), onFocusChanged: vi.fn(), destroy: vi.fn(() => Promise.resolve()) }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

/** One fake `StudioApi` per `mountStudio` call, so project A's and project
 * B's `dispatch`/`getDirtyFiles` are distinct spies — the only way to tell
 * "A's ticker never fired again" apart from "A's ticker fired but had
 * nothing dirty to save". */
interface FakeApi {
  dispatch: ReturnType<typeof vi.fn>;
  getDirtyFiles: ReturnType<typeof vi.fn>;
}
const mountedApis: FakeApi[] = [];
const mountStudio = vi.fn((..._args: unknown[]) => {
  const index = mountedApis.length;
  // Always dirty, so a still-alive ticker WOULD dispatch file.saveAll if it
  // ticked — a getDirtyFiles() that stayed empty would make "no further
  // dispatches" prove nothing about the interval itself.
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

describe("desktop autosave ticker is replaced, not duplicated, on project reopen (#2486)", () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    mountedApis.length = 0;
    vi.resetModules();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("clears project A's interval before project B's is armed, so A's tick never fires again", async () => {
    const { openProject, AUTOSAVE_MS } = await import("../main.js");

    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");
    const clearIntervalSpy = vi.spyOn(globalThis, "clearInterval");

    await openProject("/projects/a");
    expect(mountedApis).toHaveLength(1);
    // First open: closeProject() runs but `current` was still null, so it
    // returns before touching any timer — nothing to clear yet.
    expect(setIntervalSpy).toHaveBeenCalledTimes(1);
    expect(clearIntervalSpy).toHaveBeenCalledTimes(0);

    // Project B opens WITHOUT an intervening explicit close — the exact
    // scenario #2486 has no test for: openProject("/projects/b") drives
    // closeProject() internally (main.tsx ~151) before arming a new ticker.
    // closeProject now AWAITS the guarded save (#2444, `awaitSaveAllBeforeQuit`
    // in quit.ts) before unmounting A — and this mock's getDirtyFiles()
    // never clears, so that wait runs the full ~3s cap under fake timers,
    // redispatching `file.saveAll` on the way. Start the promise, drive the
    // fake clock through the cap, then await it — mirroring
    // `autosave-quit.test.ts`'s handling of the same real, un-mocked wait.
    const openBPromise = openProject("/projects/b");
    await vi.advanceTimersByTimeAsync(3200);
    await openBPromise;
    expect(mountedApis).toHaveLength(2);
    expect(setIntervalSpy).toHaveBeenCalledTimes(2);
    expect(clearIntervalSpy).toHaveBeenCalledTimes(1);

    const [apiA, apiB] = mountedApis;
    expect(apiA).toBeDefined();
    expect(apiB).toBeDefined();

    // closeProject's guarded save-wait dispatched file.saveAll on A at
    // least once (unconditionally — no longer gated on getDirtyFiles(),
    // #2444) — and, since this mock's dirty set never clears, redispatched
    // repeatedly until the cap. Snapshot the count so the assertion below
    // is about NEW calls only.
    const apiACallsAtReopen = apiA?.dispatch.mock.calls.length ?? -1;
    expect(apiACallsAtReopen).toBeGreaterThanOrEqual(1);

    // Advance well past several autosave ticks.
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 3);

    // The regression this test exists for: a still-alive interval A holding
    // a reference to project A's (unmounted) StudioHandle would rack up
    // additional dispatch("file.saveAll") calls here, on top of the one
    // legitimate close-time flush above.
    expect(apiA?.dispatch.mock.calls.length).toBe(apiACallsAtReopen);
    expect(apiB?.dispatch).toHaveBeenCalledWith("file.saveAll");
  });

  // The reopen case above passes even if the clear is moved OUT of
  // `closeProject` and into `openProject` just before the `setInterval` arm
  // — the review of PR #2512 proved that empirically. That relocation would
  // still leave an EXPLICIT close (the `menu:close-project` event, which
  // calls `closeProject()` with no reopen behind it) with project A's ticker
  // alive, dispatching `file.saveAll` through an unmounted `StudioHandle`.
  // So the spec row's "cleared on project close OR reopen" needs both halves
  // pinned; this is the close half.
  it("clears the interval on an explicit close with no reopen behind it", async () => {
    const { openProject, closeProject, AUTOSAVE_MS } = await import("../main.js");

    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");
    const clearIntervalSpy = vi.spyOn(globalThis, "clearInterval");

    await openProject("/projects/a");
    expect(mountedApis).toHaveLength(1);
    expect(setIntervalSpy).toHaveBeenCalledTimes(1);

    // closeProject now awaits the guarded save (#2444) — same
    // start-promise/advance-clock/await pattern as the reopen case above,
    // since this mock's dirty set never clears and the wait runs the full
    // ~3s cap under fake timers.
    const closePromise = closeProject();
    await vi.advanceTimersByTimeAsync(3200);
    await closePromise;
    expect(clearIntervalSpy).toHaveBeenCalledTimes(1);
    // Nothing is re-armed by a bare close — unlike the reopen path.
    expect(setIntervalSpy).toHaveBeenCalledTimes(1);

    const [apiA] = mountedApis;
    expect(apiA).toBeDefined();
    // Same close-flush snapshot as above, for the same reason — at least
    // one dispatch (unconditional), possibly more (redispatched while
    // dirty persists).
    const apiACallsAtClose = apiA?.dispatch.mock.calls.length ?? -1;
    expect(apiACallsAtClose).toBeGreaterThanOrEqual(1);

    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 3);

    expect(apiA?.dispatch.mock.calls.length).toBe(apiACallsAtClose);
  });

  // #2517: pins the exact cadence value, not merely that a constant exists.
  // A test asserting only `typeof AUTOSAVE_MS === "number"`, or one that
  // redeclares its own `120_000` instead of importing the real binding,
  // would pass unchanged if the production interval were shortened — this
  // asserts the literal `setInterval` in `openProject` actually reads.
  it("pins the autosave cadence at 120000ms (2 minutes, the 2026-08-07 ruling)", async () => {
    const { AUTOSAVE_MS } = await import("../main.js");
    expect(AUTOSAVE_MS).toBe(120_000);
  });
});
