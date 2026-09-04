/**
 * Scroll position is sticky across a tab switch (#3559).
 *
 * The slot machinery snapshotted `scrollDOM.scrollTop` and re-applied the
 * number on remount, which is not a stable address into the document:
 * CodeMirror ESTIMATES the height of unmeasured lines, so the same pixel
 * offset lands elsewhere once a fresh view measures. Measured on the 8k-line
 * fixture before the fix: leave at 4,124 px, come back at 5,177 px.
 * `view.scrollSnapshot()` records a position instead.
 *
 * jsdom cannot carry this test — it has no layout, so `scrollTop` never
 * takes a value there. It has to be a real browser.
 */
import { expect, test, type Page } from "@playwright/test";

/** The snapshot restores a document POSITION, so the pixel offset settles
 *  to that line's top on the first return — measured: 3,005 → 2,960 px,
 *  once. It does NOT accumulate: eight switch cycles then hold 2,960 px
 *  exactly. The tolerance covers that one settle, and is far below the
 *  ~1,000 px drift the pixel-offset restore produced. */
const TOLERANCE_PX = 120;

const fileRow = (page: Page, name: string) =>
  page.locator(".brink-binder-row", { hasText: name }).first();

async function openFile(page: Page, name: string): Promise<void> {
  await fileRow(page, name).locator(".brink-binder-label").first().click();
  await page.waitForTimeout(400);
}

const scrollTop = (page: Page): Promise<number> =>
  page.evaluate(() => document.querySelector(".cm-scroller")?.scrollTop ?? -1);

test.describe("scroll memory", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=perf");
    await page.waitForSelector(".cm-content", { timeout: 60_000 });
    await page.waitForSelector(".brink-binder-label", { timeout: 60_000 });
  });

  test("a tab keeps its scroll position across a switch away and back", async ({ page }) => {
    await openFile(page, "large.ink");
    await page.locator(".cm-content").first().click();
    await page.evaluate(() => {
      const s = document.querySelector(".cm-scroller");
      if (s) s.scrollTop = 4000;
    });
    await page.waitForTimeout(500);
    const before = await scrollTop(page);
    expect(before).toBeGreaterThan(1000);

    await openFile(page, "main.ink");
    await openFile(page, "large.ink");
    await page.waitForTimeout(800);

    expect(Math.abs((await scrollTop(page)) - before)).toBeLessThan(TOLERANCE_PX);
  });

  test("it holds across repeated switches without accumulating drift", async ({ page }) => {
    await openFile(page, "large.ink");
    await page.locator(".cm-content").first().click();
    await page.evaluate(() => {
      const s = document.querySelector(".cm-scroller");
      if (s) s.scrollTop = 2500;
    });
    await page.waitForTimeout(500);
    const before = await scrollTop(page);

    await openFile(page, "main.ink");
    await openFile(page, "large.ink");
    await page.waitForTimeout(600);
    const afterFirst = await scrollTop(page);
    expect(Math.abs(afterFirst - before)).toBeLessThan(TOLERANCE_PX);

    // Every later cycle must be EXACT — the one-time settle happens once.
    for (let i = 0; i < 3; i++) {
      await openFile(page, "main.ink");
      await openFile(page, "large.ink");
      await page.waitForTimeout(500);
      expect(await scrollTop(page)).toBe(afterFirst);
    }
  });
});
