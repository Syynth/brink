/**
 * Settings document e2e (issue #93, spec §4).
 *
 * Real-input flows over the settings tab: open via the palette (and the
 * Mod-, default binding), the theme picker flips data-theme live and
 * reflects external palette switches, a keymap override applied through the
 * JSON textarea rebinds immediately (new chord works, replaced default goes
 * inert), invalid JSON shows an inline error and saves nothing, and the
 * diagnostics severity flag persists across a reload.
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

async function openSettings(page: Page): Promise<void> {
  await runPaletteCommand(page, "Settings: Open");
  await expect(page.locator(".settings-doc")).toBeVisible();
}

test.describe("settings document", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("opens via the palette; reopening focuses the existing tab", async ({
    page,
  }) => {
    await openSettings(page);
    await expect(
      page.locator(".brink-tab-label", { hasText: "Settings" }),
    ).toBeVisible();

    // Re-running the command never duplicates the singleton.
    await runPaletteCommand(page, "Settings: Open");
    await expect(
      page.locator(".brink-tab-label", { hasText: "Settings" }),
    ).toHaveCount(1);
  });

  test("opens via the Mod-, default binding", async ({ page }) => {
    await page.keyboard.press("Meta+,");
    await expect(page.locator(".settings-doc")).toBeVisible();
  });

  test("theme picker applies live and reflects external switches", async ({
    page,
  }) => {
    await openSettings(page);
    const root = page.locator(".brink-studio");
    await expect(root).toHaveAttribute("data-theme", "mocha");

    // Picker → service: data-theme flips without a reload.
    await page.locator(".settings-radio", { hasText: "Catppuccin Latte" }).click();
    await expect(root).toHaveAttribute("data-theme", "latte");

    // External change (palette command) → picker reflects it.
    await runPaletteCommand(page, "Theme: Catppuccin Mocha");
    await expect(root).toHaveAttribute("data-theme", "mocha");
    await expect(
      page
        .locator(".settings-radio", { hasText: "Catppuccin Mocha" })
        .locator("input"),
    ).toBeChecked();
  });

  test("a keymap override applies immediately: new chord works, default goes inert", async ({
    page,
  }) => {
    await openSettings(page);
    await page
      .locator(".settings-json")
      .fill('{\n  "palette.toggle": "Mod-J"\n}');
    await page.locator(".settings-apply").click();
    await expect(page.locator(".settings-error")).toHaveCount(0);

    // The hamburger menu's binding hint reflects the override — and opening
    // it forces React's effect flush, so the rebuilt key handler is attached
    // before the keypress below (Playwright can otherwise outrace the
    // commit by a frame; a human cannot).
    await page.locator(".shell-hamburger").click();
    await expect(
      page.locator(".shell-menu-item", { hasText: "Command Palette" }),
    ).toContainText("⌘J");
    await page.keyboard.press("Escape");

    // The new chord opens the palette right away — no reload.
    await page.keyboard.press("Meta+j");
    await expect(page.locator(".shell-palette-input")).toBeVisible();
    await page.keyboard.press("Escape");

    // The override replaced the whole default set, so Mod-Shift-P is inert.
    await page.keyboard.press("Meta+Shift+P");
    await expect(page.locator(".shell-palette-input")).toHaveCount(0);

    // And it was persisted under the versioned key.
    expect(
      await page.evaluate(() => localStorage.getItem("brink-studio.keymap.v1")),
    ).toBe('{"palette.toggle":"Mod-J"}');
  });

  test("invalid JSON shows an inline error and saves nothing", async ({
    page,
  }) => {
    await openSettings(page);
    await page.locator(".settings-json").fill("{not json");
    await page.locator(".settings-apply").click();

    await expect(page.locator(".settings-error")).toContainText("Not valid JSON");
    expect(
      await page.evaluate(() => localStorage.getItem("brink-studio.keymap.v1")),
    ).toBeNull();

    // The keymap is untouched — the palette still opens on its default.
    await page.keyboard.press("Meta+Shift+P");
    await expect(page.locator(".shell-palette-input")).toBeVisible();
  });

  test("the diagnostics severity flag persists across a reload", async ({
    page,
  }) => {
    await openSettings(page);
    const select = page.locator(".settings-select");
    await expect(select).toHaveValue("error");

    await select.selectOption("off");
    expect(
      await page.evaluate(() =>
        localStorage.getItem("brink-studio.diagnostics.v1"),
      ),
    ).toBe('{"externalCheck":"off"}');

    await page.reload();
    await page.waitForSelector(".cm-content", { timeout: 10000 });
    await openSettings(page);
    await expect(page.locator(".settings-select")).toHaveValue("off");
  });
});
