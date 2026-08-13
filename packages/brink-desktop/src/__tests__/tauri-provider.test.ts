import { describe, expect, it, vi } from "vitest";
import type { FileChange } from "@brink-lang/editor";
import { FileChangeHub } from "@brink-lang/editor";

const invoke = vi.fn();
const listen = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => listen(...args) }));

const { TauriFileProvider } = await import("../tauri-provider.js");

type WatcherCallback = (event: { payload: { path: string; content: string | null } }) => void;

/** Capture the `fs:external-change` handler `onExternalChange` registers,
 *  the same seam the shell's real fs watcher fires into. */
function captureWatcherCallback(): { get: () => WatcherCallback } {
  let captured: WatcherCallback | undefined;
  listen.mockImplementation((_event: string, cb: WatcherCallback) => {
    captured = cb;
    return Promise.resolve(() => {});
  });
  return {
    get: () => {
      if (captured === undefined) {
        throw new Error("listen() was not called — onExternalChange did not subscribe");
      }
      return captured;
    },
  };
}

describe("TauriFileProvider.requestSave serialization (#2403)", () => {
  it("queues an overlapping requestSave call instead of racing it against an in-flight write", async () => {
    const invokeCalls: Array<{ cmd: string; rel?: unknown }> = [];
    let releaseFirstWrite: (() => void) | undefined;
    let firstWriteStarted = false;
    invoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      invokeCalls.push({ cmd, rel: args?.["rel"] });
      if (cmd === "write_file" && !firstWriteStarted) {
        firstWriteStarted = true;
        return new Promise<void>((resolve) => {
          releaseFirstWrite = resolve;
        });
      }
      return Promise.resolve(undefined);
    });

    const provider = new TauriFileProvider("/proj");
    provider.onFileChanged("scene.brink", "hello");

    // The 2-minute autosave ticker fires requestSave(); its write to disk is
    // still in flight (held open by the mock) when a second overlapping
    // caller — the guarded-quit path's saveAll (PR #2382) — also calls
    // requestSave() before the first has settled.
    const autosave = provider.requestSave();
    await Promise.resolve();
    await Promise.resolve();
    expect(invokeCalls.filter((c) => c.cmd === "write_file")).toHaveLength(1);

    const quitSave = provider.requestSave();
    await Promise.resolve();
    await Promise.resolve();

    // SERIALIZED: the overlapping call must not have issued its own
    // write_file while the first is still in flight — it queues behind the
    // first instead of racing it. (Unserialized, this call's synchronous
    // prefix reaches its own `invoke("write_file", ...)` for the very same
    // staged entry immediately, before the first write even resolves.)
    expect(invokeCalls.filter((c) => c.cmd === "write_file")).toHaveLength(1);

    releaseFirstWrite?.();
    await autosave;
    await quitSave;

    // Nothing new was staged between the two calls, so once run in proper
    // order the queued second call finds nothing left to write — exactly
    // one write for the file happened, not a duplicate racing write.
    expect(invokeCalls.filter((c) => c.cmd === "write_file")).toHaveLength(1);
  });

  it("does not discard an edit staged while its own write was in flight (review)", async () => {
    // #2412 review: writeStaged() unconditionally deleted the staged entry
    // after its await, so an edit staged DURING the in-flight write (e.g.
    // onFileChanged racing the write's resolution) was discarded even though
    // it was never actually persisted — the file goes clean on disk with
    // stale content while the buffer that superseded it is silently lost.
    let releaseFirstWrite: (() => void) | undefined;
    let firstWriteStarted = false;
    const writes: unknown[] = [];
    invoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "write_file") {
        writes.push(args?.["content"]);
        if (!firstWriteStarted) {
          firstWriteStarted = true;
          return new Promise<void>((resolve) => {
            releaseFirstWrite = resolve;
          });
        }
      }
      return Promise.resolve(undefined);
    });

    const provider = new TauriFileProvider("/proj");
    provider.onFileChanged("scene.brink", "v1");
    const firstSave = provider.requestSave();
    await Promise.resolve();
    await Promise.resolve();

    // A new edit arrives while the v1 write is still in flight.
    provider.onFileChanged("scene.brink", "v2");

    releaseFirstWrite?.();
    await firstSave;

    // v2 must still be staged — a later requestSave() must write it.
    await provider.requestSave();
    expect(writes).toEqual(["v1", "v2"]);
  });
});

describe("TauriFileProvider watcher self-delete suppression (#2404)", () => {
  it("does not drop the pending 'deleted' egress record when its own delete echoes back", async () => {
    invoke.mockResolvedValue(undefined);
    const watcher = captureWatcherCallback();

    const provider = new TauriFileProvider("/proj");
    const files = new Map<string, string>([["scene.brink", "content"]]);
    const delivered: FileChange[][] = [];

    // FileChangeHub is the real pending-egress queue `ProjectSession` owns
    // (`packages/ink-editor/src/project-session.ts`) — `onFlush` here is
    // exactly the host mirror consumer the issue describes.
    const hub = new FileChangeHub({
      getContent: (path) => files.get(path) ?? null,
      onFlush: (changes) => delivered.push(changes),
      deliveryPersists: false, // the desktop overlay contract (D2)
    });
    hub.setBaseline("scene.brink", "content");

    // Mirrors `ProjectSession`'s `onExternalChange` handler (the no-conflict
    // branch, project-session.ts): apply the host's new truth and supersede
    // any pending studio-side change for the path.
    provider.onExternalChange((path, content) => {
      if (content === null) files.delete(path);
      else files.set(path, content);
      hub.applyExternal(path, content);
    });
    await Promise.resolve();
    await Promise.resolve();

    // The studio deletes the file (`ProjectSession.deleteFile`'s real call
    // sequence): provider write-through, then record the deletion for the
    // host egress queue.
    files.delete("scene.brink");
    await provider.deleteFile("scene.brink");
    hub.record("scene.brink", "deleted");

    // The shell's fs watcher observes our own `remove_file` call and — after
    // its debounce — echoes the SAME deletion back as an external-change
    // event, exactly like a real `notify` watcher would.
    watcher.get()({ payload: { path: "scene.brink", content: null } });

    // The host mirror must still learn about the studio-initiated deletion
    // once the debounced flush lands — the self-write-only suppression
    // (pre-#2404) would have let the echo through, which would have called
    // `applyExternal` and wiped the pending "deleted" record before flush
    // ever ran.
    const flushed = hub.flush();
    expect(flushed).toEqual([{ path: "scene.brink", type: "deleted" }]);
    expect(delivered).toEqual([[{ path: "scene.brink", type: "deleted" }]]);
  });

  it("still forwards a genuinely external deletion (no matching self-delete marker)", async () => {
    invoke.mockResolvedValue(undefined);
    const watcher = captureWatcherCallback();

    const provider = new TauriFileProvider("/proj");
    const seen: Array<{ path: string; content: string | null }> = [];
    provider.onExternalChange((path, content) => seen.push({ path, content }));
    await Promise.resolve();
    await Promise.resolve();

    // Nobody called provider.deleteFile("other.brink") — this is a real
    // out-of-band deletion (e.g. `rm` from a terminal), not our own echo.
    watcher.get()({ payload: { path: "other.brink", content: null } });

    expect(seen).toEqual([{ path: "other.brink", content: null }]);
  });

  it("the self-delete marker is consumed once — a later genuine external deletion of the same path still forwards", async () => {
    invoke.mockResolvedValue(undefined);
    const watcher = captureWatcherCallback();

    const provider = new TauriFileProvider("/proj");
    const seen: Array<{ path: string; content: string | null }> = [];
    provider.onExternalChange((path, content) => seen.push({ path, content }));
    await Promise.resolve();
    await Promise.resolve();

    await provider.deleteFile("scene.brink");
    watcher.get()({ payload: { path: "scene.brink", content: null } }); // suppressed (our own)
    // The file is recreated externally and then deleted again by someone
    // else entirely — the earlier marker must not still be "armed".
    watcher.get()({ payload: { path: "scene.brink", content: "back again" } });
    watcher.get()({ payload: { path: "scene.brink", content: null } });

    expect(seen).toEqual([
      { path: "scene.brink", content: "back again" },
      { path: "scene.brink", content: null },
    ]);
  });

  it("self-write suppression for edits is unaffected by the self-delete change", async () => {
    invoke.mockResolvedValue(undefined);
    const watcher = captureWatcherCallback();

    const provider = new TauriFileProvider("/proj");
    const seen: Array<{ path: string; content: string | null }> = [];
    provider.onExternalChange((path, content) => seen.push({ path, content }));
    await Promise.resolve();
    await Promise.resolve();

    provider.onFileChanged("scene.brink", "hello");
    await provider.requestSave();

    // Our own write echoing back with the exact content we just wrote is
    // still swallowed.
    watcher.get()({ payload: { path: "scene.brink", content: "hello" } });
    // A genuinely different external edit still forwards.
    watcher.get()({ payload: { path: "scene.brink", content: "someone else's edit" } });

    expect(seen).toEqual([{ path: "scene.brink", content: "someone else's edit" }]);
  });

  it("disarms the self-delete marker when delete_file rejects (review)", async () => {
    // #2412 review: deleteFile() armed selfDeletes BEFORE the await and
    // never disarmed it on rejection — a failed delete left a permanently
    // armed marker that would silently swallow the NEXT genuine external
    // deletion of that path.
    const watcher = captureWatcherCallback();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "delete_file") return Promise.reject(new Error("permission denied"));
      return Promise.resolve(undefined);
    });

    const provider = new TauriFileProvider("/proj");
    const seen: Array<{ path: string; content: string | null }> = [];
    provider.onExternalChange((path, content) => seen.push({ path, content }));
    await Promise.resolve();
    await Promise.resolve();

    await expect(provider.deleteFile("scene.brink")).rejects.toThrow("permission denied");

    // A later, genuine external deletion of the same path must still forward
    // — the marker must not have stayed armed after the failed delete.
    watcher.get()({ payload: { path: "scene.brink", content: null } });
    expect(seen).toEqual([{ path: "scene.brink", content: null }]);
  });

  it("clears a stale self-write marker on delete so a later re-creation with identical content is not suppressed (review)", async () => {
    // #2412 review: a save immediately followed by a delete coalesces
    // shell-side into one `deleted` event, so the write marker set by the
    // save is never consumed by its own echo — it lingers and wrongly
    // suppresses a later, genuinely external re-creation with identical
    // content.
    invoke.mockResolvedValue(undefined);
    const watcher = captureWatcherCallback();

    const provider = new TauriFileProvider("/proj");
    const seen: Array<{ path: string; content: string | null }> = [];
    provider.onExternalChange((path, content) => seen.push({ path, content }));
    await Promise.resolve();
    await Promise.resolve();

    provider.onFileChanged("scene.brink", "hello");
    await provider.requestSave();
    await provider.deleteFile("scene.brink");

    // The coalesced shell event for save+delete arrives as a single
    // deletion, consumed by the self-delete marker (not the stale
    // self-write marker).
    watcher.get()({ payload: { path: "scene.brink", content: null } });
    expect(seen).toEqual([]);

    // Someone else recreates the file with the exact same content the
    // provider last wrote — this must forward as genuinely external, not be
    // swallowed by a leftover self-write marker.
    watcher.get()({ payload: { path: "scene.brink", content: "hello" } });
    expect(seen).toEqual([{ path: "scene.brink", content: "hello" }]);
  });
});
