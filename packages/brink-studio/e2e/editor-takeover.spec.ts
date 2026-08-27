/**
 * Editor takeover (decision log 2026-08-26, "The editor root area has one
 * occupant").
 *
 * Settings and the Story Graph are whole-window activities, not files, so
 * they occupy the editor root area instead of opening as tabs. The bug this
 * fixes: a tab is only reachable from a view that HAS tabs, so in Continuous
 * view — which renders the project's files — Settings opened behind the view
 * and never appeared at all.
 */

import { expect, test, type Page } from "@playwright/test";

async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.locator(".shell-palette-item", { hasText: title }).first().click();
}

test.describe("editor takeover", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("Settings is reachable from Continuous view — the case tabs could not serve", async ({
    page,
  }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await expect(page.locator("[data-editor-view='continuous']")).toBeVisible();

    await runPaletteCommand(page, "Settings: Open");

    await expect(page.locator("[data-takeover='settings']")).toBeVisible();
    await expect(page.locator(".settings-doc")).toBeVisible();
    // It OCCUPIES the area rather than sitting inside the view.
    await expect(page.locator("[data-editor-view='continuous']")).toHaveCount(0);
  });

  test("the close button gives the view back", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await runPaletteCommand(page, "Settings: Open");
    await expect(page.locator("[data-takeover='settings']")).toBeVisible();

    await page.locator(".shell-takeover-close").click();

    await expect(page.locator("[data-takeover]")).toHaveCount(0);
    // Back to the view that was underneath, not to some default.
    await expect(page.locator("[data-editor-view='continuous']")).toBeVisible();
  });

  test("takes over Code view too, rather than opening a tab there", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Code");
    const tabsBefore = await page.locator("[role='tab']").count();

    await runPaletteCommand(page, "Settings: Open");
    await expect(page.locator("[data-takeover='settings']")).toBeVisible();
    // While a takeover is up there are no tabs on screen AT ALL — it occupies
    // the whole area, tab strip included — so the count has to be taken after
    // dismissing it. Asserting it during the takeover measured the takeover,
    // not the absence of a Settings tab.
    await page.locator(".shell-takeover-close").click();

    // One mechanism in every view: no Settings tab was minted anywhere.
    expect(await page.locator("[role='tab']").count()).toBe(tabsBefore);
    await expect(page.locator("[role='tab']", { hasText: "Settings" })).toHaveCount(0);
  });

  test("choosing a view dismisses it", async ({ page }) => {
    await runPaletteCommand(page, "Settings: Open");
    await expect(page.locator("[data-takeover='settings']")).toBeVisible();

    await runPaletteCommand(page, "View mode: Single File");

    await expect(page.locator("[data-takeover]")).toHaveCount(0);
    await expect(page.locator("[data-editor-view='single']")).toBeVisible();
  });

  test("does not survive a reload — it is an interruption, not a place", async ({
    page,
  }) => {
    await runPaletteCommand(page, "Settings: Open");
    await expect(page.locator("[data-takeover='settings']")).toBeVisible();

    await page.reload();
    await page.waitForSelector(".cm-content", { timeout: 10000 });

    await expect(page.locator("[data-takeover]")).toHaveCount(0);
  });
});
