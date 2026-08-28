/**
 * Single File view (decision log 2026-08-26, "The three editor views are
 * named Code, Single File, and Continuous").
 *
 * The editor root area holds one occupant. These cover what makes this view
 * a view rather than a mode: no tab strip, navigation that replaces instead
 * of accumulating, a companion split that belongs to the view, and an active
 * file that survives switching back to Code view.
 */

import { expect, test, type Page } from "@playwright/test";

async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.locator(".shell-palette-item", { hasText: title }).first().click();
}

const singleFile = "[data-editor-view='single']";

test.describe("single file view", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("replaces the editor area: one file, a player, and no tabs", async ({ page }) => {
    // Code view is the default, and it has tabs.
    await expect(page.locator("[role='tab']").first()).toBeVisible();

    await runPaletteCommand(page, "View mode: Single File");

    await expect(page.locator(singleFile)).toBeVisible();
    // The defining property: no tab strip at all, not a strip with one entry.
    await expect(page.locator("[role='tab']")).toHaveCount(0);
    // The file is named, the editor is there, and so is the companion.
    await expect(page.locator(".shell-single-file-name")).not.toBeEmpty();
    await expect(page.locator(".cm-content")).toBeVisible();
    await expect(page.locator(".player-pane")).toBeVisible();
  });

  test("the companion collapses and comes back, and never closes into nothing", async ({
    page,
  }) => {
    await runPaletteCommand(page, "View mode: Single File");
    const toggle = page.locator(".shell-single-file-companion-toggle");

    await toggle.click();
    await expect(page.locator(".player-pane")).toHaveCount(0);
    // Still offered — the split is part of the view, so there is always a way
    // back. A close button that left an empty pane would be the other design.
    await expect(toggle).toBeVisible();

    await toggle.click();
    await expect(page.locator(".player-pane")).toBeVisible();
  });

  test("opening a file replaces the one on screen rather than adding to it", async ({
    page,
  }) => {
    await runPaletteCommand(page, "View mode: Single File");
    const name = page.locator(".shell-single-file-name");
    const first = await name.textContent();

    // Reach a different file the way an author would. The Binder is already
    // open in the default layout — clicking its strip button would TOGGLE it
    // shut, which is what the first draft of this test did.
    await page
      .locator(".brink-binder-file-row")
      .filter({ hasNotText: first ?? "" })
      .first()
      .click();

    await expect(name).not.toHaveText(first ?? "");
    await expect(page.locator("[role='tab']")).toHaveCount(0);
  });

  test("the active file and the chosen view both survive a reload", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Single File");
    const before = await page.locator(".shell-single-file-name").textContent();

    await page.reload();
    await page.waitForSelector(singleFile, { timeout: 10000 });

    await expect(page.locator(".shell-single-file-name")).toHaveText(before ?? "");
  });

  test("Settings offers the view picker; the view changes under the modal", async ({
    page,
  }) => {
    await runPaletteCommand(page, "Settings: Open");
    await page.locator(".brink-settings-scope", { hasText: "App" }).click();
    await page.locator(".brink-settings-nav-item", { hasText: "Editor" }).click();

    const group = page.locator("[aria-label='Editor view']");
    await expect(group).toBeVisible();
    await group.locator("input[value='single']").check();

    // Under the old takeover, choosing a view had to put Settings away —
    // Settings WAS the area you were choosing what to fill, so you could only
    // pick once per visit. As a modal (#3174) it stays up over the view you
    // just chose, and you can pick again to compare.
    await expect(page.locator(singleFile)).toBeVisible();
    await expect(page.locator(".brink-settings-modal")).toBeVisible();
    await expect(page.locator("[data-takeover]")).toHaveCount(0);
  });

  test("switching back to Code view keeps the file you were on", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Single File");
    const inSingle = await page.locator(".shell-single-file-name").textContent();

    await runPaletteCommand(page, "View mode: Code");

    await expect(page.locator(singleFile)).toHaveCount(0);
    // The active file is the one thing the two views share, so it is still
    // the document on screen — as the selected tab, this time. Scoped to the
    // first group: the default layout puts the player in a second group,
    // whose own tab is selected too.
    await expect(
      page
        .locator(".shell-editor-group")
        .first()
        .locator("[role='tab'][aria-selected='true']"),
    ).toContainText(inSingle ?? "");
  });
});
