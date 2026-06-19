/**
 * Player document e2e (issue #120, spec §4, §5.4, §7.6, §7.8).
 *
 * Real-input flows over the player as an editor-area document: the fresh
 * load reproduces the Inky two-up (editor left, player right, editor
 * focused; the player tool window is gone and State View holds the right
 * strip's start slot), the session plays inside the document (stop →
 * placeholder → Start → continue → choose), story.openPlayer focuses the
 * existing tab, group maximize via the palette hides docks/siblings and
 * Escape restores them, and Mod-\ duplicates the player tab into a second
 * live view of the one session.
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

function group(page: Page, index: number): Locator {
  return page.locator(".shell-editor-group").nth(index);
}

function tabsIn(g: Locator): Locator {
  return g.locator(".brink-tab .brink-tab-label");
}

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

/**
 * Drive the session to the named choice and click it. Advance through the intro
 * by clicking Continue (waiting for the transcript to actually grow between
 * clicks, so we never read a half-rendered choice set) until the target choice
 * appears, then click it.
 *
 * The choice list now renders stably: it wins over the Continue button, and a
 * stray `story.continue` at a choice point is a no-op (#273). So once the target
 * is visible it stays visible — no retry/flicker handling needed.
 */
async function continueToChoice(pane: Locator, choice: string): Promise<void> {
  const target = pane.locator(".choices button", { hasText: choice });
  const continueBtn = pane.locator(".choices button", { hasText: "Continue" });

  for (let i = 0; i < 40; i++) {
    await expect(pane.locator(".choices button").first()).toBeVisible();
    if ((await target.count()) > 0) break;
    if ((await continueBtn.count()) === 0) break; // choices are up but not the target
    const before = (await pane.locator(".story-text").textContent()) ?? "";
    await continueBtn.first().click();
    await expect
      .poll(async () => (await pane.locator(".story-text").textContent()) ?? "", {
        timeout: 10000,
      })
      .not.toBe(before);
  }

  await expect(target).toBeVisible();
  await target.first().click();
}

test.describe("player document", () => {
  // The default (toppled-temple) project: its startup compile succeeds and
  // auto-starts the session (§7.6), so the player document has content.
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("fresh load is the Inky two-up: editor left, player right, editor focused", async ({
    page,
  }) => {
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink"]);
    await expect(tabsIn(group(page, 1))).toHaveText(["Player"]);
    await expect(page.locator(".player-pane")).toBeVisible();
    // Typing goes to the editor: the left group is focused.
    await expect(group(page, 0)).toHaveAttribute("data-focused", "true");

    // The player tool window is gone — no strip icon anywhere — and State
    // View takes the right strip's start slot (still closed by default).
    await expect(page.locator('.shell-strip-btn[aria-label="Player"]')).toHaveCount(0);
    await expect(
      page.locator(
        '.shell-strip-right .shell-strip-section-start .shell-strip-btn[aria-label="State View"]',
      ),
    ).toBeVisible();
    await expect(page.locator('[data-toolwindow="state"]')).toHaveCount(0);
  });

  test("the session plays inside the document: stop, start, continue, choose", async ({
    page,
  }) => {
    // Driving the story to a choice point + riding out the player's hasPending
    // render flicker legitimately takes longer than a simple UI test.
    test.slow();
    const pane = page.locator(".player-pane");
    // The startup compile auto-starts the session.
    await expect(pane.locator(".story-text")).toContainText("Toppled Temple", {
      timeout: 10000,
    });

    // Stop → session-bound placeholder with the start affordance (§7.6).
    await runPaletteCommand(page, "Story: Stop");
    await expect(pane).toContainText("No story session");

    // Start from the placeholder, then play to the first choice point.
    await pane.locator(".session-placeholder-start").click();
    await expect(pane.locator(".story-text")).toContainText("Toppled Temple");

    // Choose inside the document: drive to the merchant choice and pick it; the
    // choice echoes into the transcript.
    await continueToChoice(pane, "Browse his wares");
    await expect(pane.locator(".story-text")).toContainText("> Browse his wares");
  });

  test("story.openPlayer focuses the existing tab instead of duplicating", async ({
    page,
  }) => {
    // Focus the editor group first.
    await group(page, 0).locator(".cm-content").click();
    await expect(group(page, 0)).toHaveAttribute("data-focused", "true");

    await runPaletteCommand(page, "Story: Open Player");

    await expect(group(page, 1)).toHaveAttribute("data-focused", "true");
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    await expect(page.locator(".brink-tab-label", { hasText: "Player" })).toHaveCount(1);
  });

  test("group maximize hides docks and siblings; Escape restores them", async ({
    page,
  }) => {
    // Baseline: binder dock open, two groups.
    await expect(page.locator(".shell-dock-left")).toBeVisible();
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);

    // Focus the player group (click its tab), then maximize via the palette.
    await page.locator(".brink-tab", { hasText: "Player" }).click();
    await runPaletteCommand(page, "Editor: Toggle Maximized Group");

    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await expect(tabsIn(group(page, 0))).toHaveText(["Player"]);
    await expect(page.locator(".player-pane")).toBeVisible();
    await expect(page.locator(".shell-dock-left")).toHaveCount(0);

    // Escape restores the previous layout exactly: both groups, binder dock.
    await page.keyboard.press("Escape");
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink"]);
    await expect(page.locator(".shell-dock-left")).toBeVisible();
    await expect(page.locator('[data-toolwindow="binder"]')).toBeVisible();
  });

  test("the maximize button in the player header toggles its own group", async ({
    page,
  }) => {
    await page.locator(".player-pane .header button[title='Maximize']").click();
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await expect(page.locator(".player-pane")).toBeVisible();

    await page.locator(".player-pane .header button[title='Restore (Esc)']").click();
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
  });

  test("Mod-\\ duplicates the player tab: two live views of one session", async ({
    page,
  }) => {
    test.slow(); // drives the story to a choice + rides the hasPending flicker
    const pane = page.locator(".player-pane");
    await expect(pane.locator(".story-text")).toContainText("Toppled Temple", {
      timeout: 10000,
    });

    await page.locator(".brink-tab", { hasText: "Player" }).click();
    await page.keyboard.press("ControlOrMeta+\\");

    await expect(page.locator(".shell-editor-group")).toHaveCount(3);
    await expect(page.locator(".brink-tab-label", { hasText: "Player" })).toHaveCount(2);

    // Both views are plain subscribers over the one session: same content,
    // and driving the story from one updates the other.
    const panes = page.locator(".player-pane");
    await expect(panes).toHaveCount(2);
    await expect(panes.nth(0).locator(".story-text")).toContainText("Toppled Temple");
    await expect(panes.nth(1).locator(".story-text")).toContainText("Toppled Temple");

    await continueToChoice(panes.nth(1), "Push past him");
    await expect(panes.nth(0).locator(".story-text")).toContainText("> Push past him");
  });
});
