/**
 * W4 worker road, real wasm + real Worker (docs/editor-worker-spec.md §8):
 * with `?worker=1`, the project-level pulls — compile, outline, story
 * graph, closure — run in a Web Worker with its own wasm session.
 *
 * The discriminating probe: `__brinkPerf.wasmCounters()` reads the MAIN
 * thread's wasm perf registry — the worker's module instance has its own.
 * So in worker mode a completed compile must be OBSERVABLE (the story
 * runs — story bytes only exist through a landed CompileResult) while the
 * main-thread registry records ZERO `ide.compile`: the proof the compile
 * genuinely left the thread, not just the call stack. The control test
 * pins the same counter as nonzero without the flag, so the probe can
 * never rot into vacuity.
 */

import { test, expect, type Page } from "@playwright/test";

type Counters = Record<string, { count: number }>;

async function mainThreadCounters(page: Page): Promise<Counters> {
  return page.evaluate(
    () =>
      (
        window as unknown as {
          __brinkPerf: { wasmCounters(): Counters | null };
        }
      ).__brinkPerf.wasmCounters() ?? {},
  );
}

test("worker mode compiles off the main thread and still lands results", async ({ page }) => {
  await page.goto("/?fixture=screenplay&worker=1");
  await page.waitForSelector(".cm-content");

  // Type into the editor so at least one full debounce->compile->fan-out
  // cycle runs through the worker road.
  await page.click(".cm-content");
  await page.keyboard.press("End");
  await page.keyboard.type("x");

  // The story runs: story bytes exist only through a landed CompileResult,
  // so visible story text IS the compile round-trip completing.
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await expect
    .poll(async () => ((await page.locator(".story-text").textContent()) ?? "").length, {
      timeout: 20_000,
    })
    .toBeGreaterThan(0);

  const counters = await mainThreadCounters(page);
  // The analysis-free keystroke path ran on the main thread…
  expect(counters["ide.updateSource"]?.count ?? 0).toBeGreaterThan(0);
  // …but no compile (or outline pull) ever did: they ran in the worker's
  // own wasm instance.
  expect(counters["ide.compile"]).toBeUndefined();
  expect(counters["ide.projectOutline"]).toBeUndefined();
});

test("without the flag the compile still runs in-process (control)", async ({ page }) => {
  await page.goto("/?fixture=screenplay");
  await page.waitForSelector(".cm-content");
  await expect
    .poll(async () => (await mainThreadCounters(page))["ide.compile"]?.count ?? 0, {
      timeout: 20_000,
    })
    .toBeGreaterThan(0);
});
