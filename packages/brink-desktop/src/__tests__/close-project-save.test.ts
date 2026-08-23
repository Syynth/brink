// @vitest-environment jsdom
/**
 * Pins #2444: `closeProject()` (main.tsx) must dispatch `file.saveAll`
 * UNCONDITIONALLY before unmounting — not gated on `getDirtyFiles().length
 * > 0` — and must AWAIT that save (via `awaitSaveAllBeforeQuit`, quit.ts)
 * rather than fire-and-forget it.
 *
 * Before this fix, `closeProject` dispatched `file.saveAll` only when
 * `getDirtyFiles()` reported something dirty, and never awaited the
 * dispatch — `current.unmount()` ran immediately after. Both gaps mirror
 * exactly what #2434/PR #2437 fixed for `awaitSaveAllBeforeQuit` on the
 * quit path: `getDirtyFiles()` only reflects the 500ms trailing debounce
 * that `ProjectSession.notifyFileChanged` runs on, so a keystroke made just
 * before Close Project could be invisible to that gate and get skipped
 * entirely (not merely delayed).
 *
 * This test's `getDirtyFiles()` NEVER reports anything dirty — the old,
 * gated code would see that and skip the dispatch outright. The fixed code
 * calls `awaitSaveAllBeforeQuit`, which dispatches once unconditionally
 * regardless of what `getDirtyFiles()` currently says (see quit.ts's own
 * "dispatches file.saveAll even when getDirtyFiles reports empty" test for
 * the same contract on the quit path). Since the dirty set is empty from
 * the start, the guarded wait's poll loop never actually needs to sleep —
 * real timers are fine here, no `vi.useFakeTimers()` needed.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(vi.fn())) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: vi.fn(), destroy: vi.fn(() => Promise.resolve()) }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

interface FakeApi {
  dispatch: ReturnType<typeof vi.fn>;
  getDirtyFiles: ReturnType<typeof vi.fn>;
}
const mountedApis: FakeApi[] = [];
/** Each mounted handle's `unmount` mock, parallel to `mountedApis` — kept
 * reachable by the test (rather than buried inside the resolved handle) so
 * the second test below can push into `callOrder` from it. */
const mountedUnmounts: Array<ReturnType<typeof vi.fn>> = [];
const mountStudio = vi.fn((..._args: unknown[]) => {
  // Deliberately ALWAYS clean — the old, gated `closeProject` would never
  // dispatch at all in this scenario, which is exactly the regression this
  // file pins against.
  const api: FakeApi = {
    dispatch: vi.fn(() => true),
    getDirtyFiles: vi.fn(() => []),
  };
  mountedApis.push(api);
  const unmount = vi.fn();
  mountedUnmounts.push(unmount);
  return Promise.resolve({
    api: { ...api, notify: vi.fn(), select: vi.fn(), getStoryBytes: vi.fn(() => null) },
    entryFile: "main.ink",
    unmount,
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

describe("closeProject dispatches file.saveAll unconditionally and awaits it (#2444)", () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    mountedApis.length = 0;
    mountedUnmounts.length = 0;
    vi.resetModules();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("still dispatches file.saveAll on an explicit close when getDirtyFiles() never reports anything dirty", async () => {
    const { openProject, closeProject } = await import("../main.js");

    await openProject("/projects/a");
    expect(mountedApis).toHaveLength(1);
    const [apiA] = mountedApis;
    expect(apiA).toBeDefined();
    expect(apiA?.dispatch).not.toHaveBeenCalled();

    await closeProject();

    // The old, gated implementation would see getDirtyFiles() === [] and
    // skip the dispatch entirely — this is the regression this test exists
    // to catch.
    expect(apiA?.dispatch).toHaveBeenCalledWith("file.saveAll");
    expect(apiA?.dispatch).toHaveBeenCalledTimes(1);
  });

  it("awaits the save before unmount() runs, on reopen (openProject's internal close)", async () => {
    const { openProject } = await import("../main.js");

    await openProject("/projects/a");
    const [apiA] = mountedApis;
    const [unmountA] = mountedUnmounts;
    expect(apiA).toBeDefined();
    expect(unmountA).toBeDefined();

    const callOrder: string[] = [];
    apiA?.dispatch.mockImplementation((commandId: string) => {
      callOrder.push(`dispatch:${commandId}`);
      return true;
    });
    unmountA?.mockImplementation(() => {
      callOrder.push("unmount");
    });

    await openProject("/projects/b");

    // Both must be observed to have happened, AND in that order — proving
    // closeProject actually AWAITS the save before tearing down the handle,
    // not merely that both eventually happened somewhere in the reopen.
    // Before the fix this pins, an unchanged callOrder assertion here would
    // pass just as well against a closeProject that unmounted first and
    // saved after.
    expect(callOrder).toContain("dispatch:file.saveAll");
    expect(callOrder).toContain("unmount");
    expect(callOrder.indexOf("dispatch:file.saveAll")).toBeLessThan(callOrder.indexOf("unmount"));
  });
});
