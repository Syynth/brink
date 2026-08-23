// @vitest-environment jsdom
/**
 * Pins the concurrency fix from the 2026-08-21 review of PR #2927:
 * `closeProject()` (`main.tsx`) must clear the module-level "a project is
 * open" state (`current`, `currentRoot`, `currentEntryFile`) SYNCHRONOUSLY,
 * before awaiting `awaitSaveAllBeforeQuit` — not only after the whole
 * function resolves.
 *
 * Before this fix, a second overlapping call to `closeProject()` — e.g. a
 * second Close Project click during the (up to ~3s) save wait, since the
 * `menu:close-project` listener is fire-and-forget `() => void
 * closeProject()` with nothing disabling the menu item meanwhile — would
 * still read `current !== null` (the first call hadn't nulled it yet), pass
 * the `if (current === null) return;` guard, and run `handle.unmount()` a
 * second time against a handle the first call was already tearing down (or
 * had already torn down).
 *
 * This suite calls `closeProject()` twice back-to-back without awaiting the
 * first call before making the second — run-to-completion semantics mean
 * the second call's synchronous prefix (the guard check) runs before the
 * first call ever reaches its first `await`, which is exactly what exposes
 * the bug: with the state cleared eagerly, the second call's guard sees
 * `current === null` and returns immediately; with the old code (state
 * cleared only at the end), the guard still saw the stale non-null
 * `current` and proceeded into a second `unmount()`.
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
const mountedUnmounts: Array<ReturnType<typeof vi.fn>> = [];
const mountStudio = vi.fn((..._args: unknown[]) => {
  // Always dirty, so the guarded save-wait runs its full ~3s cap under
  // fake timers — the same window a real second click would land in.
  const api: FakeApi = {
    dispatch: vi.fn(() => true),
    getDirtyFiles: vi.fn(() => ["dirty.ink"]),
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

describe("closeProject() rejects a second overlapping call instead of double-unmounting (2026-08-21 review)", () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    mountedApis.length = 0;
    mountedUnmounts.length = 0;
    vi.resetModules();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("only the first of two back-to-back calls unmounts the handle", async () => {
    const { openProject, closeProject } = await import("../main.js");

    await openProject("/projects/a");
    expect(mountedApis).toHaveLength(1);
    const [unmountA] = mountedUnmounts;
    expect(unmountA).toBeDefined();

    // Fire both calls synchronously, exactly as two rapid Close Project
    // clicks (or a click racing an internal `openProject` teardown) would:
    // neither is awaited before the other starts.
    const first = closeProject();
    const second = closeProject();

    await vi.advanceTimersByTimeAsync(3200);
    await Promise.all([first, second]);

    // The regression this test exists for: the old code let the second
    // call's guard see the still-non-null `current` and run `unmount()`
    // again.
    expect(unmountA).toHaveBeenCalledTimes(1);
  });
});
