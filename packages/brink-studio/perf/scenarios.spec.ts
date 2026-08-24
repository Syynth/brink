/**
 * Perf scenarios (measure-first ruling, docs/decision-log.md 2026-08-24).
 *
 * Each test drives one named, deterministic scenario against the
 * `?fixture=perf` playground and writes a run artifact directory under
 * `<repo>/perf-runs/<timestamp>-<scenario>/`:
 *
 *   - `probe.json`  — the `@brink-lang/editor` perf probe's full report
 *                     (spans, aggregates, worst events, marks)
 *   - `wasm-counters.json` — the wasm-internal phase counters
 *   - `trace.json`  — a CDP Performance trace (open in Chrome DevTools'
 *                     Performance panel or Perfetto for the flame chart)
 *   - `meta.json`   — commit, scenario, fixture, environment
 *
 * These are MEASUREMENTS, not assertions — the only expectations here
 * guard that the scenario actually ran (the probe saw events), never that
 * a duration is below a bound. Comparison across runs is
 * `scripts/perf-compare.mjs`'s job.
 *
 * Run: `pnpm test:perf` (all) or `pnpm test:perf -- -g typing-burst`.
 */

import { test, expect, type Page } from "@playwright/test";
import { execSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RUNS_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../../perf-runs");

/** Label runs by wall clock so successive attempts sort chronologically. */
function runDir(scenario: string): string {
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const dir = join(RUNS_ROOT, `${stamp}-${scenario}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function gitMeta(): { commit: string; dirty: boolean } {
  try {
    const commit = execSync("git rev-parse HEAD", { encoding: "utf8" }).trim();
    const dirty = execSync("git status --porcelain", { encoding: "utf8" }).trim().length > 0;
    return { commit, dirty };
  } catch {
    return { commit: "unknown", dirty: false };
  }
}

interface PerfHook {
  report: () => unknown;
  reset: () => void;
  wasmCounters: () => unknown;
  compileProbe: () => [number, number] | null;
}

declare global {
  interface Window {
    __brinkPerf?: PerfHook;
  }
}

async function openPerfFixture(page: Page): Promise<void> {
  await page.goto("/?fixture=perf");
  // Mounted: an editor view exists and the Binder lists the project. (NOT
  // `.brink-knot-header` — the perf fixture's main.ink is all INCLUDEs,
  // it contains no knot headers to decorate.)
  await page.waitForSelector(".cm-content", { timeout: 60_000 });
  await page.waitForSelector(".brink-binder-label", { timeout: 60_000 });
  await page.waitForFunction(() => window.__brinkPerf !== undefined, undefined, {
    timeout: 10_000,
  });
}

/** Open `large.ink` through the Binder (the user road, not a store poke). */
async function openLargeFile(page: Page): Promise<void> {
  const row = page.locator(".brink-binder-label", { hasText: "large.ink" }).first();
  await row.click();
  // The big file's first knot header proves the view switched.
  await page.waitForFunction(
    () => document.body.textContent?.includes("big_000") ?? false,
    undefined,
    { timeout: 30_000 },
  );
  await page.locator(".cm-content").first().click();
}

async function resetProbe(page: Page): Promise<void> {
  await page.evaluate(() => window.__brinkPerf?.reset());
}

async function harvest(
  page: Page,
  dir: string,
  scenario: string,
  extra: Record<string, unknown> = {},
): Promise<void> {
  const probe = await page.evaluate(() => window.__brinkPerf?.report() ?? null);
  const wasm = await page.evaluate(() => window.__brinkPerf?.wasmCounters() ?? null);
  writeFileSync(join(dir, "probe.json"), JSON.stringify(probe, null, 2));
  writeFileSync(join(dir, "wasm-counters.json"), JSON.stringify(wasm, null, 2));
  writeFileSync(
    join(dir, "meta.json"),
    JSON.stringify(
      {
        scenario,
        fixture: "perf",
        recordedAt: new Date().toISOString(),
        ...gitMeta(),
        userAgent: await page.evaluate(() => navigator.userAgent),
        ...extra,
      },
      null,
      2,
    ),
  );
  // The scenario must actually have produced probe data — an empty report
  // means the dev edge never enabled collection (a broken run, not a fast
  // one).
  expect(probe, "probe report missing — dev-edge collection not active").not.toBeNull();
  const aggregates = (probe as { aggregates?: unknown[] } | null)?.aggregates ?? [];
  expect(aggregates.length, "probe saw zero events — scenario did not exercise the editor").toBeGreaterThan(0);
}

test.describe("perf scenarios", () => {
  test("project-open", async ({ page, browser }) => {
    const dir = runDir("project-open");
    await browser.startTracing(page, { path: join(dir, "trace.json"), screenshots: false });
    await openPerfFixture(page);
    // Startup marks (studio.mountStart → studio.firstFrame) are already in
    // the probe; give the initial debounced compile time to land too.
    await page.waitForTimeout(2_000);
    await browser.stopTracing();
    await harvest(page, dir, "project-open");
  });

  test("typing-burst", async ({ page, browser }) => {
    const dir = runDir("typing-burst");
    await openPerfFixture(page);
    await openLargeFile(page);
    await resetProbe(page);
    await browser.startTracing(page, { path: join(dir, "trace.json"), screenshots: false });

    // A deterministic burst in the big file: prose, line breaks (the
    // reported symptom), pauses long enough for the 500 ms debounce to
    // fire twice mid-burst.
    const editor = page.locator(".cm-content").first();
    await editor.click();
    await page.keyboard.press("End");
    for (let round = 0; round < 3; round++) {
      await page.keyboard.type("The lantern gutters in the harbor wind.", { delay: 40 });
      await page.keyboard.press("Enter");
      await page.keyboard.type("Another line lands after the break.", { delay: 40 });
      await page.keyboard.press("Enter");
      await page.waitForTimeout(700); // let the debounced compile fire
    }

    await browser.stopTracing();
    await harvest(page, dir, "typing-burst");
  });

  test("fast-scroll", async ({ page, browser }) => {
    const dir = runDir("fast-scroll");
    await openPerfFixture(page);
    await openLargeFile(page);
    await resetProbe(page);
    await browser.startTracing(page, { path: join(dir, "trace.json"), screenshots: false });

    // Wheel top-to-bottom in bursts — the blank-viewport reproduction.
    const scroller = page.locator(".cm-scroller").first();
    const box = await scroller.boundingBox();
    expect(box).not.toBeNull();
    if (box) {
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      for (let i = 0; i < 60; i++) {
        await page.mouse.wheel(0, 1200);
        await page.waitForTimeout(30);
      }
      for (let i = 0; i < 30; i++) {
        await page.mouse.wheel(0, -2400);
        await page.waitForTimeout(30);
      }
    }
    await page.waitForTimeout(500);

    await browser.stopTracing();
    await harvest(page, dir, "fast-scroll");
  });

  test("compile-cycles", async ({ page, browser }) => {
    const dir = runDir("compile-cycles");
    await openPerfFixture(page);
    await openLargeFile(page);
    await resetProbe(page);
    await browser.startTracing(page, { path: join(dir, "trace.json"), screenshots: false });

    // Five isolated single-keystroke → debounced-compile cycles.
    const editor = page.locator(".cm-content").first();
    await editor.click();
    await page.keyboard.press("End");
    for (let i = 0; i < 5; i++) {
      await page.keyboard.type("x");
      await page.waitForTimeout(900); // 500 ms debounce + compile + fan-out
    }

    await browser.stopTracing();
    // The in-browser #2885 experiment rides along with this scenario.
    const compileProbe = await page.evaluate(() => window.__brinkPerf?.compileProbe() ?? null);
    await harvest(page, dir, "compile-cycles", { compileProbe });
  });
});
