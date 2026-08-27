/**
 * Quick open (⌘P, "Go to File or Symbol") has to land you in the right place
 * in EVERY view mode (decision log 2026-08-26).
 *
 * It is the one navigation surface that works without the Binder, so it is
 * how you move around a view that has no tab strip. It dispatches the same
 * file + span reveal every other navigation surface does, which is exactly
 * the shape each view knows how to honour — but "should follow" and "does
 * follow" are different claims, and this makes it the second one.
 */

import { expect, test, type Page } from "@playwright/test";

async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.locator(".shell-palette-item", { hasText: title }).first().click();
}

/** Open quick-open and pick the first entry matching `query`. */
async function quickOpen(page: Page, query: string): Promise<void> {
  // Through the palette rather than the ⌘P chord: the browser claims that
  // one for print in some contexts, and this test is about where you LAND,
  // not about the binding.
  await runPaletteCommand(page, "Go to File or Symbol");
  // QuickPick reuses the palette's own chrome classes rather than having
  // its own, so these are the same selectors the command palette uses.
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(query);
  await page.locator(".shell-palette-item", { hasText: query }).first().click();
}

/** The revealed line, wherever it ended up. */
const revealedLine = "[data-continuous-file='toppled-temple.ink'] .cm-activeLine";

test.describe("quick open lands correctly in every view", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("Code view", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Code");
    await quickOpen(page, "warden_golem");

    await expect(
      page.locator(".shell-editor-group").first().locator("[role='tab'][aria-selected='true']"),
    ).toContainText("toppled-temple.ink");
    await expect(page.locator(".cm-activeLine").first()).toContainText("warden_golem");
  });

  test("Single File view", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Single File");
    await quickOpen(page, "warden_golem");

    await expect(page.locator(".shell-single-file-name")).toHaveText("toppled-temple.ink");
    await expect(page.locator(".cm-activeLine").first()).toContainText("warden_golem");
  });

  test("Continuous view", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    const scroller = page.locator(".shell-continuous-scroller");
    await scroller.evaluate((el) => {
      el.scrollTop = 0;
    });

    await quickOpen(page, "warden_golem");

    // Here landing means scrolling: the file was already on screen further
    // down, so nothing "opens" — the manuscript moves.
    await expect.poll(async () => scroller.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
    await expect(page.locator(revealedLine)).toContainText("warden_golem");
    await expect(page.locator(revealedLine)).toBeInViewport();
  });
});
