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
import { ensureStoryStarted } from "./session-helpers.js";

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
 * Drive the session to the named choice and click it. Two phases, each robust
 * to slow CI runners (the source of the prior flake):
 *
 * 1. **Advance the intro** by clicking Continue, waiting for the transcript to
 *    actually grow between clicks (never reads a half-rendered choice set),
 *    until the target choice first appears.
 * 2. **Select the target** by retry-clicking *only* the target button. The pane
 *    renders `hasPending ? [Continue] : [choices]` and `hasPending` can briefly
 *    flicker at a choice point, so the target button comes and goes — but a
 *    locator scoped to its text can never match Continue, so retrying can only
 *    ever land the real choice (never advance past it), it just waits out the
 *    flicker. We deliberately stop clicking Continue here: dispatching continue
 *    at a choice point is what aggravates the wobble.
 */
async function continueToChoice(pane: Locator, choice: string): Promise<void> {
  const target = pane.locator(".choices button", { hasText: choice });
  const continueBtn = pane.locator(".choices button", { hasText: "Continue" });

  // Phase 1 — advance until the choice point is reached.
  for (let i = 0; i < 40; i++) {
    await expect(pane.locator(".choices button").first()).toBeVisible();
    if ((await target.count()) > 0) break;
    if ((await continueBtn.count()) === 0) {
      await pane.page().waitForTimeout(50); // transient empty render — settle
      continue;
    }
    const before = (await pane.locator(".story-text").textContent()) ?? "";
    await continueBtn.first().click();
    // Wait for PROGRESS, which is not the same as changed text (#3011). Now
    // that a reveal advances one line, that line can carry no visible text at
    // all — `appendLines` skips empty strings, and a `done` boundary has none —
    // so a legitimate advance can leave the transcript byte-identical. Under
    // the old run-to-pause reveal a batch almost always contained something
    // visible, which is why polling on text alone used to hold. Treat the
    // choice list appearing as progress too, or a story that steps onto a
    // choice via a textless line hangs here for the full timeout.
    await expect
      .poll(
        async () => {
          const text = (await pane.locator(".story-text").textContent()) ?? "";
          const atChoice = (await target.count()) > 0;
          return text !== before || atChoice;
        },
        { timeout: 10000 },
      )
      .toBe(true);
  }

  // Phase 2 — pick the target, riding out any hasPending flicker (the button
  // comes and goes as the player re-renders choices↔Continue). Retry-clicking a
  // text-scoped locator can only ever land the real choice, never advance past
  // it. If the wobble is bad enough that no window opens here, the test-level
  // retry re-runs from a fresh `page.goto` (a clean session that isn't stuck).
  await expect(async () => {
    await target.first().click({ timeout: 1000 });
  }).toPass({ timeout: 20000 });
}

test.describe("player document", () => {
  // The player's `hasPending` briefly oscillates at a choice point (a known
  // render wobble — see the follow-up ticket), so the choice-driving tests can
  // rarely fail to land a click within a run. Retry from a fresh page in that
  // case; a retry gets a clean (un-stuck) session. The non-choice tests here
  // are unaffected (they pass first try).
  test.describe.configure({ retries: 2 });

  // The default (toppled-temple) project. Since W7/#3300 (RULED: no
  // auto-start) the startup compile leaves the Player idle — start the
  // story explicitly, retry-clicking through the compile race (the
  // placeholder's Start does nothing until story bytes land).
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
    await ensureStoryStarted(page);
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
