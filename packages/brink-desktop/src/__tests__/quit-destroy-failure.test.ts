// @vitest-environment jsdom
/**
 * Pins #2401's narrower, concrete case: `handleQuitRequested`
 * (`main.tsx`) calls `getCurrentWindow().destroy()` AFTER the native side
 * has already committed to not closing on its own (`onCloseRequested`'s
 * `event.preventDefault()` for red-button close, or simply having reached
 * this function via the `menu:quit` event for ⌘Q). An unhandled `destroy()`
 * rejection there previously propagated out of `handleQuitRequested`
 * uncaught, leaving the app in whatever state `autosaveTimer`'s clear (a
 * few lines above the `await`) put it in — permanently disarmed, since
 * nothing re-armed it — with no recovery short of Force Quit.
 *
 * This suite drives `handleQuitRequested` directly against a `destroy()`
 * that rejects once, and asserts:
 *  1. The rejection does not propagate out of `handleQuitRequested` (the
 *     promise resolves, not rejects) — the caller (`onCloseRequested` /
 *     the `menu:quit` listener) never sees an unhandled rejection.
 *  2. `autosaveTimer` is re-armed rather than left permanently dead.
 *  3. The failure is surfaced to the author via the studio's notification
 *     surface, not just swallowed to the console.
 *  4. A subsequent quit attempt (once `destroy()` starts succeeding) still
 *     works — the app is not permanently wedged.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { destroyMock } = vi.hoisted(() => ({ destroyMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(vi.fn())) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: vi.fn(), destroy: destroyMock }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

interface FakeApi {
  dispatch: ReturnType<typeof vi.fn>;
  getDirtyFiles: ReturnType<typeof vi.fn>;
  notify: ReturnType<typeof vi.fn>;
}
const mountedApis: FakeApi[] = [];
const mountStudio = vi.fn((..._args: unknown[]) => {
  const api: FakeApi = {
    dispatch: vi.fn(() => true),
    getDirtyFiles: vi.fn(() => []),
    notify: vi.fn(),
  };
  mountedApis.push(api);
  return Promise.resolve({
    api: { ...api, select: vi.fn(), getStoryBytes: vi.fn(() => null) },
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
  pickProjectFile: vi.fn(() => Promise.resolve(null)),
  discoverProjectConfig: vi.fn(() => Promise.resolve(null)),
  createProject: vi.fn(() => Promise.resolve("")),
  readAppSettings: vi.fn(() => Promise.resolve({ reopenLastProject: false })),
  writeAppSettings: vi.fn(() => Promise.resolve()),
  previousExitClean: vi.fn(() => Promise.resolve(true)),
  pruneRecent: vi.fn(() => Promise.resolve([])),
  pushRecent: vi.fn(() => Promise.resolve([])),
  readRecents: vi.fn(() => Promise.resolve([])),
  saveBytesDialog: vi.fn(() => Promise.resolve(null)),
}));

describe("handleQuitRequested recovers from a rejected destroy() (#2401)", () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    mountedApis.length = 0;
    destroyMock.mockReset();
    vi.resetModules();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("does not throw, re-arms autosave, and notifies the author on a rejected destroy()", async () => {
    destroyMock.mockRejectedValueOnce(new Error("ipc failure"));

    const { openProject, handleQuitRequested } = await import("../main.js");

    await openProject("/projects/a");
    expect(mountedApis).toHaveLength(1);
    const [apiA] = mountedApis;
    expect(apiA).toBeDefined();

    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");

    // Must resolve, not reject — the caller (onCloseRequested / the
    // menu:quit listener) never awaits with a .catch of its own.
    await expect(handleQuitRequested()).resolves.toBeUndefined();

    expect(destroyMock).toHaveBeenCalledTimes(1);
    // autosaveTimer, cleared at the top of handleQuitRequested, must be
    // re-armed after the failure — not left permanently disarmed.
    expect(setIntervalSpy).toHaveBeenCalledTimes(1);
    // The failure must be visible to the author, not just console.error.
    expect(apiA?.notify).toHaveBeenCalledWith(
      expect.objectContaining({ severity: "error", source: "quit" }),
    );
  });

  it("a later quit attempt still succeeds once destroy() stops rejecting", async () => {
    destroyMock.mockRejectedValueOnce(new Error("ipc failure"));
    destroyMock.mockResolvedValueOnce(undefined);

    const { openProject, handleQuitRequested } = await import("../main.js");

    await openProject("/projects/a");
    await handleQuitRequested();
    expect(destroyMock).toHaveBeenCalledTimes(1);

    // The app is not wedged: a second quit attempt (e.g. the author tries
    // ⌘Q again) reaches destroy() again and this time succeeds.
    await expect(handleQuitRequested()).resolves.toBeUndefined();
    expect(destroyMock).toHaveBeenCalledTimes(2);
  });
});
