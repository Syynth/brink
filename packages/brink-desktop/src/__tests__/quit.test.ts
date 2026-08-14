import { describe, expect, it, vi } from "vitest";
import { awaitSaveAllBeforeQuit, type QuitSaveApi } from "../quit.js";

/** A `QuitSaveApi` stub whose `getDirtyFiles()` walks through a fixed
 * sequence of results, one call per invocation, holding the last entry once
 * the sequence runs out — models the dirty set converging to `[]` (or
 * never converging, for the timeout case). */
function stubApi(dirtySequence: string[][]): QuitSaveApi {
  let call = 0;
  const getDirtyFiles = vi.fn((): string[] => {
    const files = dirtySequence[Math.min(call, dirtySequence.length - 1)] as string[];
    call += 1;
    return files;
  });
  return { dispatch: vi.fn(() => true), getDirtyFiles };
}

describe("awaitSaveAllBeforeQuit", () => {
  it("dispatches file.saveAll even when getDirtyFiles reports empty", async () => {
    // getDirtyFiles() only reflects the 500ms debounce-recorded dirty set;
    // a keystroke made just before quit may not show up there yet, so
    // file.saveAll (which flushes pending edits synchronously) must be
    // dispatched regardless of what getDirtyFiles() currently reports.
    const api = stubApi([[]]);
    await awaitSaveAllBeforeQuit(api, 3000, 5);
    expect(api.dispatch).toHaveBeenCalledTimes(1);
    expect(api.dispatch).toHaveBeenCalledWith("file.saveAll");
  });

  it("dispatches file.saveAll and waits for dirty files to clear", async () => {
    // Simulates the host-save promise chain settling after a couple of polls.
    const api = stubApi([["a.ink"], ["a.ink"], ["a.ink"], []]);
    await awaitSaveAllBeforeQuit(api, 3000, 5);
    expect(api.dispatch).toHaveBeenCalledTimes(1);
    expect(api.dispatch).toHaveBeenCalledWith("file.saveAll");
    expect(api.getDirtyFiles()).toEqual([]);
  });

  it("caps the wait and returns even if a file never clears", async () => {
    const api = stubApi([["a.ink"]]);
    const start = Date.now();
    await awaitSaveAllBeforeQuit(api, 100, 10);
    const elapsed = Date.now() - start;
    // Must return promptly at the cap, not hang indefinitely.
    expect(elapsed).toBeGreaterThanOrEqual(90);
    expect(elapsed).toBeLessThan(1000);
    expect(api.dispatch).toHaveBeenCalledTimes(1);
  });

  // ── #2434: a mid-write edit (#2426/#2431) that correctly stays dirty
  // after the first `file.saveAll` settles must not just sit there until
  // the cap burns out — a redispatch has to actually pick it up ──

  it("re-dispatches file.saveAll when the dirty set persists past the redispatch interval", async () => {
    // Models a path staying dirty for a while after the first `file.saveAll`
    // settles (the #2426 stale-mid-write-edit case), then clearing once a
    // second write (the redispatch) picks up the current content.
    const dirtySequence: string[][] = [];
    for (let i = 0; i < 10; i += 1) dirtySequence.push(["a.ink"]);
    dirtySequence.push([]);
    const api = stubApi(dirtySequence);
    await awaitSaveAllBeforeQuit(api, 3000, 5, 20);
    // First dispatch (unconditional) plus at least one redispatch once the
    // dirty set outlived the redispatch interval.
    // `vi.mocked` re-narrows to the Mock type. `stubApi`'s `: QuitSaveApi`
    // return annotation widens the `vi.fn()`s back to the plain interface, so
    // `api.dispatch.mock` exists at runtime but not to `tsc` (TS2339) —
    // vitest strips types rather than checking them, so only `tsc --noEmit`
    // sees it.
    expect(vi.mocked(api.dispatch).mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(api.dispatch).toHaveBeenCalledWith("file.saveAll");
  });

  it("holds the timeout cap even while redispatching for a file that never clears", async () => {
    // The central safety claim under test: a genuinely hung write must not
    // make quit hang, even once redispatching is in play. With
    // timeoutMs=100, pollIntervalMs=5, redispatchIntervalMs=20 and a dirty
    // set that never clears, redispatches land at t≈20/40/60/80 (5 total
    // dispatches including the initial one) — a deadline-unguarded
    // redispatch would additionally fire at t≈100, right as the cap
    // expires, so this also proves the deadline guard (no redispatch once
    // the deadline is inside one redispatch interval).
    const api = stubApi([["a.ink"]]);
    const start = Date.now();
    await awaitSaveAllBeforeQuit(api, 100, 5, 20);
    const elapsed = Date.now() - start;
    expect(elapsed).toBeGreaterThanOrEqual(90);
    expect(elapsed).toBeLessThan(200);
    const dispatchCalls = vi.mocked(api.dispatch).mock.calls.length;
    expect(dispatchCalls).toBeGreaterThanOrEqual(2);
    expect(dispatchCalls).toBeLessThanOrEqual(5);
  });

  it("a mid-write edit reaches disk via a re-dispatched file.saveAll", async () => {
    // A fuller double modeling the file-commands.ts save discipline
    // (#2426/#2431): dispatching "file.saveAll" snapshots the current
    // content and, after `writeDelayMs`, writes that snapshot to `disk` —
    // clearing dirty only if nothing edited the content in the meantime.
    // This lets the assertion check the actual "disk" the edit reaches,
    // not an internal dirty flag (house rule: assert what the consumer
    // receives).
    const path = "story.ink";
    const disk = new Map<string, string>();
    let content = "v1";
    let dirty = true;
    // A generous margin between the mid-write edit and the write settling:
    // a tight margin (e.g. a 10ms edit delay against a 40ms write) risks a
    // loaded test runner overshooting the edit's `sleep` past the write's
    // settle time, making the edit land too late and the test fail for a
    // timing reason unrelated to the fix under test.
    const writeDelayMs = 150;
    const dispatch = vi.fn((commandId: string): boolean => {
      if (commandId !== "file.saveAll") return false;
      const before = content;
      dirty = true;
      setTimeout(() => {
        disk.set(path, before);
        if (content === before) dirty = false;
      }, writeDelayMs);
      return true;
    });
    const api: QuitSaveApi = { dispatch, getDirtyFiles: () => (dirty ? [path] : []) };

    const quitDone = awaitSaveAllBeforeQuit(api, 1000, 5, 20);
    // The mid-write edit: lands well before the first write (150ms) settles.
    await new Promise((resolve) => setTimeout(resolve, 30));
    content = "v2";

    await quitDone;

    // The edit must have actually reached disk, not just have flipped a
    // flag back to clean.
    expect(disk.get(path)).toBe("v2");
    expect(dirty).toBe(false);
  });
});
