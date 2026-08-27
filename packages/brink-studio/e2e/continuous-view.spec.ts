/**
 * Continuous view (decision log 2026-08-26): every file stacked in binder
 * order as one manuscript.
 *
 * The properties worth guarding are the ones that distinguish this from "a
 * column of editors": one scroller rather than per-file scrollers, sections
 * that match the Binder exactly, and files sized to their own content.
 */

import { expect, test, type Page } from "@playwright/test";

async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.locator(".shell-palette-item", { hasText: title }).first().click();
}

const continuous = "[data-editor-view='continuous']";

test.describe("continuous view", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("stacks exactly the Binder's files, in the Binder's order", async ({ page }) => {
    // Wait for the tree: `.cm-content` says the editor is up, which is not
    // the same as the Binder having rendered its rows. Reading too early
    // gave an empty list and the comparison passed vacuously.
    await expect(page.locator(".brink-binder-file-row").first()).toBeVisible();
    const binderFiles = await page
      .locator(".brink-binder-file-row")
      .evaluateAll((rows) =>
        rows.map((r) => r.getAttribute("data-binder-row-key") ?? ""),
      );

    await runPaletteCommand(page, "View mode: Continuous");
    await expect(page.locator(continuous)).toBeVisible();

    const sections = await page
      .locator(".shell-continuous-section")
      .evaluateAll((els) => els.map((e) => e.getAttribute("data-continuous-file") ?? ""));

    // Same files, same order. A mounted std/ library file appearing here was
    // the first bug this view had: the outline carries them, the Binder tree
    // filters them, and the ordering helper has to filter them too.
    expect(sections.length).toBeGreaterThan(0);
    expect(sections.some((s) => s.startsWith("std/"))).toBe(false);
    expect(sections).toEqual(
      binderFiles.filter((f) => f !== "").slice(0, sections.length),
    );
  });

  test("is one scroller, with each file sized to its own content", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await expect(page.locator(continuous)).toBeVisible();

    const metrics = await page.evaluate(() => {
      const sc = document.querySelector(".shell-continuous-scroller");
      const editors = [...document.querySelectorAll(".shell-continuous-doc .cm-editor")];
      return {
        scrollHeight: sc?.scrollHeight ?? 0,
        clientHeight: sc?.clientHeight ?? 0,
        editorHeights: editors.map((e) => Math.round(e.getBoundingClientRect().height)),
      };
    });

    // The manuscript is taller than the window — that is the point.
    expect(metrics.scrollHeight).toBeGreaterThan(metrics.clientHeight);
    // Files differ in length, so equal heights would mean each editor had
    // been sized to the viewport instead of to its text — which is what an
    // internal scroller per file looks like.
    expect(new Set(metrics.editorHeights).size).toBeGreaterThan(1);
  });

  test("each file carries a heading", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    const headings = page.locator(".shell-continuous-title");
    await expect(headings.first()).toBeVisible();
    expect(await headings.count()).toBe(
      await page.locator(".shell-continuous-section").count(),
    );
  });

  test("navigating to a file scrolls the manuscript to it", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await expect(page.locator(continuous)).toBeVisible();
    await expect(page.locator(".brink-binder-file-row").first()).toBeVisible();

    const scroller = page.locator(".shell-continuous-scroller");
    // Start at the far end, so landing on the first file has to move.
    await scroller.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    const before = await scroller.evaluate((el) => el.scrollTop);
    expect(before).toBeGreaterThan(0);

    await page.locator(".brink-binder-file-row").first().click();

    // Navigation here IS scrolling: nothing opens, the manuscript moves.
    await expect
      .poll(async () => scroller.evaluate((el) => el.scrollTop))
      .toBeLessThan(before);
    await expect(page.locator(".shell-continuous-section[data-active]")).toHaveCount(1);
  });

  test("a structural reveal scrolls to the knot, not just the file", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await expect(page.locator(continuous)).toBeVisible();

    // Structure mode lists symbols; clicking one used to open a
    // `path::symbol` document, which this view does not render — so the
    // click did nothing at all. Symbol targets now resolve to a position
    // inside the file's section.
    await page.locator(".brink-binder-mode-toggle button").last().click();
    const symbol = page.locator(".brink-binder-row", { hasText: "warden_golem" }).first();
    await expect(symbol).toBeVisible();

    const scroller = page.locator(".shell-continuous-scroller");
    await scroller.evaluate((el) => {
      el.scrollTop = 0;
    });

    await symbol.click();

    await expect.poll(async () => scroller.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
    // The revealed line is the knot itself, and it is on screen.
    const line = page.locator(
      "[data-continuous-file='toppled-temple.ink'] .cm-activeLine",
    );
    await expect(line).toContainText("warden_golem");
    await expect(line).toBeInViewport();
  });

  test("the view survives a reload", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await page.reload();
    await page.waitForSelector(continuous, { timeout: 10000 });
    await expect(page.locator(".shell-continuous-section").first()).toBeVisible();
  });

  test("Settings offers it alongside the other views", async ({ page }) => {
    await runPaletteCommand(page, "Settings: Open");
    const radio = page.locator("[aria-label='Editor view'] input[value='continuous']");
    await expect(radio).toBeVisible();

    // Dispatched rather than `.check()`: choosing a view dismisses whatever
    // had taken the area over, so picking Continuous from inside Settings
    // closes Settings — by design (decision log 2026-08-26). Playwright's
    // actionability check sees the input detach mid-click and retries until
    // it times out, which is a fact about the check, not about the app.
    await radio.dispatchEvent("click");
    await expect(page.locator(continuous)).toBeVisible();
  });
});
