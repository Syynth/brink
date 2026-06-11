/**
 * Editor tab drag e2e (issue #142, spec §7.8).
 *
 * Real pointer sequences over the per-group tab bars: drag a tab into the
 * other group's bar (cross-group move; emptied source collapses), drag the
 * Player tab the other way with a front insertion, reorder within a group,
 * ghost + insert-indicator visuals mid-drag, Escape cancel, and that a
 * plain click still activates after all of it. Mirrors drag-redock.spec.ts
 * (the strip-drag precedent).
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

function group(page: Page, index: number): Locator {
  return page.locator(".shell-editor-group").nth(index);
}

function tabsIn(g: Locator): Locator {
  return g.locator(".brink-tab .brink-tab-label");
}

/** A tab by its exact title (the tab div carries title={ref.title}). */
function tab(page: Page, title: string): Locator {
  return page.locator(`.brink-tab[title="${title}"]`);
}

/** Press on a tab and drag past the threshold (ghost appears). */
async function startDrag(page: Page, tabEl: Locator): Promise<void> {
  const box = await tabEl.boundingBox();
  if (!box) throw new Error("tab has no bounding box");
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx + 24, cy + 4, { steps: 4 });
  await expect(page.locator(".shell-tab-drag-ghost")).toBeVisible();
}

test.describe("editor tab drag", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await page.waitForSelector(".brink-knot-header", { timeout: 5000 });
    // Default layout (#120): group 1 = main.ink, group 2 = Player.
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
  });

  test("dragging a tab into the other group's bar moves it (source collapses)", async ({
    page,
  }) => {
    // Drag main.ink (group 1's only tab) past the Player tab into group 2's
    // tail. Mid-drag the ghost and a tail insert indicator must show.
    await startDrag(page, tab(page, "main.ink"));
    const playerTab = tab(page, "Player");
    const box = await playerTab.boundingBox();
    if (!box) throw new Error("Player tab has no bounding box");
    await page.mouse.move(box.x + box.width + 30, box.y + box.height / 2, { steps: 8 });
    await expect(page.locator(".brink-tab-drop-after")).toHaveCount(1);
    await page.screenshot({ path: "/tmp/tab-polish-shots/mid-drag-cross-group.png" });
    await page.mouse.up();

    // Ghost and indicator gone; group 1 emptied and collapsed; the moved
    // tab landed after Player and is active.
    await expect(page.locator(".shell-tab-drag-ghost")).toHaveCount(0);
    await expect(page.locator(".brink-tab-drop-after")).toHaveCount(0);
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await expect(tabsIn(group(page, 0))).toHaveText(["Player", "main.ink"]);
    await expect(tab(page, "main.ink")).toHaveAttribute("aria-selected", "true");
    // The document actually renders in the surviving group.
    await expect(group(page, 0).locator(".cm-content")).toBeVisible();
  });

  test("dragging the Player tab to group 1 inserts at the pointer's gap", async ({
    page,
  }) => {
    // Drop on the left half of main.ink → Player lands in front of it.
    await startDrag(page, tab(page, "Player"));
    const mainTab = tab(page, "main.ink");
    const box = await mainTab.boundingBox();
    if (!box) throw new Error("main.ink tab has no bounding box");
    await page.mouse.move(box.x + 8, box.y + box.height / 2, { steps: 8 });
    await expect(page.locator(".brink-tab-drop-before")).toHaveCount(1);
    await page.mouse.up();

    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await expect(tabsIn(group(page, 0))).toHaveText(["Player", "main.ink"]);
    await expect(tab(page, "Player")).toHaveAttribute("aria-selected", "true");
  });

  test("dragging within a group reorders", async ({ page }) => {
    // Single-group baseline with two tabs: close Player, open the "opening"
    // knot next to main.ink (the groups.spec pattern).
    const playerTab = tab(page, "Player");
    await playerTab.hover();
    await playerTab.locator(".brink-tab-close").click();
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await page
      .locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" })
      .dblclick();
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink", "opening (main.ink)"]);

    // Drag the second tab onto the left half of the first → order flips.
    await startDrag(page, tab(page, "opening (main.ink)"));
    const mainTab = tab(page, "main.ink");
    const box = await mainTab.boundingBox();
    if (!box) throw new Error("main.ink tab has no bounding box");
    await page.mouse.move(box.x + 8, box.y + box.height / 2, { steps: 8 });
    await expect(page.locator(".brink-tab-drop-before")).toHaveCount(1);
    await page.mouse.up();

    await expect(tabsIn(group(page, 0))).toHaveText(["opening (main.ink)", "main.ink"]);
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
  });

  test("Escape mid-drag cancels: ghost gone, nothing moved", async ({ page }) => {
    await startDrag(page, tab(page, "main.ink"));
    const playerTab = tab(page, "Player");
    const box = await playerTab.boundingBox();
    if (!box) throw new Error("Player tab has no bounding box");
    await page.mouse.move(box.x + box.width + 30, box.y + box.height / 2, { steps: 8 });
    await expect(page.locator(".brink-tab-drop-after")).toHaveCount(1);
    await page.keyboard.press("Escape");

    await expect(page.locator(".shell-tab-drag-ghost")).toHaveCount(0);
    await expect(page.locator(".brink-tab-drop-after")).toHaveCount(0);
    await page.mouse.up();
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink"]);
    await expect(tabsIn(group(page, 1))).toHaveText(["Player"]);
  });

  test("a plain click (no movement) still activates", async ({ page }) => {
    // Two tabs in one group; click flips activation without dragging.
    const playerTab = tab(page, "Player");
    await playerTab.hover();
    await playerTab.locator(".brink-tab-close").click();
    await page
      .locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" })
      .dblclick();
    await expect(tab(page, "opening (main.ink)")).toHaveAttribute("aria-selected", "true");

    await tab(page, "main.ink").click();
    await expect(tab(page, "main.ink")).toHaveAttribute("aria-selected", "true");
    await expect(tab(page, "opening (main.ink)")).toHaveAttribute(
      "aria-selected",
      "false",
    );
  });
});
