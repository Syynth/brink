/**
 * Settings e2e (issue #93; re-anchored to the modal by #3174).
 *
 * Real-input flows: open via the palette (and the Mod-, default binding),
 * the theme picker flips data-theme live and reflects external palette
 * switches, a keymap override applied through the JSON textarea rebinds
 * immediately (new chord works, replaced default goes inert), invalid JSON
 * shows an inline error and saves nothing, and the diagnostics severity flag
 * persists across a reload.
 *
 * Settings is a MODAL as of #3174, not an editor takeover — consult-and-
 * adjust should not cost you the file you were reading. Sections are also
 * one-at-a-time behind a rail now, so a flow that touches a section has to
 * navigate to it first rather than scrolling one long page.
 */

import { test, expect, type Page } from "@playwright/test";

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

async function openSettings(page: Page): Promise<void> {
  await runPaletteCommand(page, "Settings: Open");
  await expect(page.locator(".brink-settings-modal")).toBeVisible();
}

/** Switch scope and select a rail section by title. */
async function openSection(page: Page, scope: "Project" | "App", title: string) {
  await page.locator(".brink-settings-scope", { hasText: scope }).click();
  await page.locator(".brink-settings-nav-item", { hasText: title }).click();
  await expect(page.locator(".brink-settings-head h2")).toHaveText(title);
}

test.describe("settings document", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("opens via the palette; reopening keeps exactly one modal", async ({
    page,
  }) => {
    // A MODAL, not a takeover (#3174, ruled 2026-08-27): Settings is
    // consult-and-adjust, so it must not cost you the file you were reading.
    // The editor stays mounted behind it — that is the property the takeover
    // could not have.
    await openSettings(page);
    await expect(page.locator(".cm-content").first()).toBeAttached();

    await runPaletteCommand(page, "Settings: Open");
    await expect(page.locator(".brink-settings-modal")).toHaveCount(1);
  });

  test("opens via the Mod-, default binding", async ({ page }) => {
    await page.keyboard.press("ControlOrMeta+,");
    await expect(page.locator(".brink-settings-modal")).toBeVisible();
  });

  test("theme picker applies live and reflects external switches", async ({
    page,
  }) => {
    await openSettings(page);
    await openSection(page, "App", "Appearance");
    // The app root, not a tile's preview — every tile now carries
    // `.brink-studio` + `data-theme` too, since that pair IS the theme
    // cascade and is what makes a tile show the real theme (#3174).
    const root = page.locator("[data-brink-studio-root]").or(
      page.locator(".brink-studio").first(),
    );
    await expect(root).toHaveAttribute("data-theme", "mocha");

    // Picker → service: data-theme flips without a reload.
    await page
      .locator(".settings-theme-tile", { hasText: "Catppuccin Latte" })
      .click();
    await expect(root).toHaveAttribute("data-theme", "latte");

    // External change (palette command) → picker reflects it.
    await runPaletteCommand(page, "Theme: Catppuccin Mocha");
    await expect(root).toHaveAttribute("data-theme", "mocha");
    await expect(
      page
        .locator(".settings-theme-tile", { hasText: "Catppuccin Mocha" })
        .locator("input"),
    ).toBeChecked();
  });

  test("a keymap override applies immediately: new chord works, default goes inert", async ({
    page,
  }) => {
    await openSettings(page);
    await openSection(page, "App", "Keymap");
    // The JSON editor is the escape hatch below the keymap table (#3334) —
    // collapsed until opened.
    await page.locator(".settings-escape-hatch > summary").click();
    await page
      .locator(".settings-json")
      .fill('{\n  "palette.toggle": "Mod-J"\n}');
    await page.locator(".settings-apply").click();
    await expect(page.locator(".settings-error")).toHaveCount(0);

    // Close Settings before touching the shell chrome. As a MODAL (#3174) it
    // has a backdrop that intercepts pointer events — which is the point of a
    // modal, and is also the real flow: you apply a keybinding and leave.
    // Under the old takeover the rest of the window stayed clickable.
    await page.keyboard.press("Escape");
    await expect(page.locator(".brink-settings-modal")).toHaveCount(0);

    // The hamburger menu's binding hint reflects the override — and opening
    // it forces React's effect flush, so the rebuilt key handler is attached
    // before the keypress below (Playwright can otherwise outrace the
    // commit by a frame; a human cannot).
    await page.locator(".shell-hamburger").click();
    // The hint is formatted per-platform by the shell (⌘ on macOS, Ctrl+
    // elsewhere); match what this runner's OS renders. See formatChord.
    const modJ = process.platform === "darwin" ? "⌘J" : "Ctrl+J";
    await expect(
      page.locator(".shell-menu-item", { hasText: "Command Palette" }),
    ).toContainText(modJ);
    await page.keyboard.press("Escape");

    // The new chord opens the palette right away — no reload.
    await page.keyboard.press("ControlOrMeta+j");
    await expect(page.locator(".shell-palette-input")).toBeVisible();
    await page.keyboard.press("Escape");

    // The override replaced the whole default set, so Mod-Shift-P is inert.
    await page.keyboard.press("ControlOrMeta+Shift+P");
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
    await openSection(page, "App", "Keymap");
    await page.locator(".settings-escape-hatch > summary").click();
    await page.locator(".settings-json").fill("{not json");
    await page.locator(".settings-apply").click();

    await expect(page.locator(".settings-error")).toContainText("Not valid JSON");
    expect(
      await page.evaluate(() => localStorage.getItem("brink-studio.keymap.v1")),
    ).toBeNull();

    // The keymap is untouched — the palette still opens on its default.
    await page.keyboard.press("ControlOrMeta+Shift+P");
    await expect(page.locator(".shell-palette-input")).toBeVisible();
  });

  test("the diagnostics severity flag persists across a reload", async ({
    page,
  }) => {
    await openSettings(page);
    // Its own rail section now (#3174): the `[lints]` table is a PROJECT
    // setting and this flag is an APP one, which the scope switch states
    // rather than a hint inside a mixed section.
    // External-function checking lives inside the Player section now.
    await openSection(page, "App", "Player");
    const diagSelect = page.locator(".settings-select").first();
    await expect(diagSelect).toHaveValue("error");

    await diagSelect.selectOption("off");
    expect(
      await page.evaluate(() =>
        localStorage.getItem("brink-studio.diagnostics.v1"),
      ),
    ).toBe('{"externalCheck":"off"}');

    await page.reload();
    await page.waitForSelector(".cm-content", { timeout: 10000 });
    await openSettings(page);
    // External-function checking lives inside the Player section now.
    await openSection(page, "App", "Player");
    await expect(page.locator(".settings-select").first()).toHaveValue("off");
  });
});
