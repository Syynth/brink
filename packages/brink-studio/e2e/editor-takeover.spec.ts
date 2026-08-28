/**
 * Editor takeover (decision log 2026-08-26, "The editor root area has one
 * occupant"), as it stands after #3174.
 *
 * The **Story Graph** is a whole-window activity, not a file, so it occupies
 * the editor root area instead of opening as a tab. The bug that motivated
 * the mechanism: a tab is only reachable from a view that HAS tabs, so in
 * Continuous view — which renders the project's files — such a document
 * opened behind the view and never appeared at all.
 *
 * **Settings is no longer one of these.** #3174 moved it to a modal (ruled
 * 2026-08-27): it is consult-and-adjust, so taking over the editor cost you
 * the file you were reading for something you leave in seconds. The takeover
 * was right while Settings was small; the `brink.toml` interface made it a
 * surface with its own navigation, which is what a modal is for. Its flows
 * live in `settings.spec.ts` — the two "is it reachable from Continuous
 * view" and "does it survive a reload" cases below are kept here against the
 * Graph, because they test the MECHANISM rather than the occupant.
 */

import { expect, test, type Page } from "@playwright/test";

async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.locator(".shell-palette-item", { hasText: title }).first().click();
}

/** The Story Graph's palette command, whatever it is titled. */
const OPEN_GRAPH = "Story: Open Story Graph";

test.describe("editor takeover", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("the Story Graph is reachable from Continuous view — the case tabs could not serve", async ({
    page,
  }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await expect(page.locator("[data-editor-view='continuous']")).toBeVisible();

    await runPaletteCommand(page, OPEN_GRAPH);

    await expect(page.locator("[data-takeover]")).toBeVisible();
    // It OCCUPIES the area rather than sitting inside the view.
    await expect(page.locator("[data-editor-view='continuous']")).toHaveCount(0);
  });

  test("the close button gives the view back", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Continuous");
    await runPaletteCommand(page, OPEN_GRAPH);
    await expect(page.locator("[data-takeover]")).toBeVisible();

    await page.locator(".shell-takeover-close").click();

    await expect(page.locator("[data-takeover]")).toHaveCount(0);
    // Back to the view that was underneath, not to some default.
    await expect(page.locator("[data-editor-view='continuous']")).toBeVisible();
  });

  test("takes over Code view too, rather than opening a tab there", async ({ page }) => {
    await runPaletteCommand(page, "View mode: Code");
    const tabsBefore = await page.locator("[role='tab']").count();

    await runPaletteCommand(page, OPEN_GRAPH);
    await expect(page.locator("[data-takeover]")).toBeVisible();
    // While a takeover is up there are no tabs on screen AT ALL — it occupies
    // the whole area, tab strip included — so the count has to be taken after
    // dismissing it. Asserting it during the takeover measured the takeover,
    // not the absence of a tab.
    await page.locator(".shell-takeover-close").click();

    expect(await page.locator("[role='tab']").count()).toBe(tabsBefore);
  });

  test("choosing a view dismisses it", async ({ page }) => {
    await runPaletteCommand(page, OPEN_GRAPH);
    await expect(page.locator("[data-takeover]")).toBeVisible();

    await runPaletteCommand(page, "View mode: Single File");

    await expect(page.locator("[data-takeover]")).toHaveCount(0);
    await expect(page.locator("[data-editor-view='single']")).toBeVisible();
  });

  test("does not survive a reload — it is an interruption, not a place", async ({
    page,
  }) => {
    await runPaletteCommand(page, OPEN_GRAPH);
    await expect(page.locator("[data-takeover]")).toBeVisible();

    await page.reload();
    await page.waitForSelector(".cm-content", { timeout: 10000 });

    await expect(page.locator("[data-takeover]")).toHaveCount(0);
  });

  test("Settings is a modal, not a takeover — the editor stays behind it", async ({
    page,
  }) => {
    // The regression guard for #3174's ruling. If Settings ever goes back to
    // occupying the area, this fails rather than the change passing silently
    // because the settings spec only ever asserts what it can see.
    await runPaletteCommand(page, "Settings: Open");

    await expect(page.locator(".brink-settings-modal")).toBeVisible();
    await expect(page.locator("[data-takeover]")).toHaveCount(0);
    await expect(page.locator(".cm-content").first()).toBeAttached();
  });
});
