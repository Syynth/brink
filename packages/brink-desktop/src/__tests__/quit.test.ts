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
});
