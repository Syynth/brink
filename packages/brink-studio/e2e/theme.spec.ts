/**
 * Theme switching e2e (issue #92, spec §7.4).
 *
 * Real-input flow: switch themes via the palette command and verify the
 * whole UI flips — the data-theme attribute changes, a computed background
 * actually changes color, and the choice persists across a reload through
 * localStorage (brink-studio.theme.v1).
 */

import { test, expect, type Page } from "@playwright/test";

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("Meta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

function statusBarBackground(page: Page): Promise<string> {
  return page
    .locator(".shell-statusbar")
    .evaluate((el) => getComputedStyle(el).backgroundColor);
}

test.describe("theme switching", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("palette command flips data-theme and the rendered colors", async ({
    page,
  }) => {
    const root = page.locator(".brink-studio");
    await expect(root).toHaveAttribute("data-theme", "mocha");
    const mochaBg = await statusBarBackground(page);

    await runPaletteCommand(page, "Theme: Catppuccin Latte");
    await expect(root).toHaveAttribute("data-theme", "latte");
    const latteBg = await statusBarBackground(page);
    expect(latteBg).not.toBe(mochaBg);

    // And back — runtime switching is symmetric, no reload involved.
    await runPaletteCommand(page, "Theme: Catppuccin Mocha");
    await expect(root).toHaveAttribute("data-theme", "mocha");
    expect(await statusBarBackground(page)).toBe(mochaBg);
  });

  test("the choice persists across a reload", async ({ page }) => {
    await runPaletteCommand(page, "Theme: Catppuccin Latte");
    await expect(page.locator(".brink-studio")).toHaveAttribute(
      "data-theme",
      "latte",
    );

    await page.reload();
    await page.waitForSelector(".cm-content", { timeout: 10000 });
    await expect(page.locator(".brink-studio")).toHaveAttribute(
      "data-theme",
      "latte",
    );
    expect(
      await page.evaluate(() => localStorage.getItem("brink-studio.theme.v1")),
    ).toBe("latte");
  });
});
