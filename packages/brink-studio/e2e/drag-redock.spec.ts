/**
 * Strip-icon drag-to-re-dock e2e (shell issue 3.1 / #87, spec §5.1).
 *
 * Drives real pointer sequences through the browser: drag the Binder icon to
 * the bottom dock and back (the issue's acceptance scenario), ghost +
 * drop-zone visuals mid-drag, Escape cancel, the same-section no-op, and
 * that a plain click still toggles after all of it.
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

function stripButton(page: Page, dock: "left" | "right" | "bottom", label: string): Locator {
  return page.locator(`.shell-strip-${dock} .shell-strip-btn[aria-label="${label}"]`);
}

function dropZone(page: Page, zone: string): Locator {
  return page.locator(`.shell-strip-dropzone[data-zone="${zone}"]`);
}

/** Press on a strip button and drag past the threshold (ghost appears). */
async function startDrag(page: Page, button: Locator): Promise<void> {
  const box = await button.boundingBox();
  if (!box) throw new Error("strip button has no bounding box");
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx + 24, cy + 8, { steps: 4 });
  await expect(page.locator(".shell-drag-ghost")).toBeVisible();
}

/** Continue the active drag onto a drop zone (it must highlight). */
async function dragOver(page: Page, zone: Locator): Promise<void> {
  const box = await zone.boundingBox();
  if (!box) throw new Error("drop zone has no bounding box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 8 });
  await expect(zone).toHaveClass(/active/);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  // Shell is up once the strips render (Binder defaults to left/start, open).
  await expect(stripButton(page, "left", "Binder")).toBeVisible();
  await expect(page.locator('[data-toolwindow="binder"]')).toBeVisible();
});

test("drag Binder to the bottom dock and back (re-docks, stays open, click toggles there)", async ({
  page,
}) => {
  // ── Left → bottom.end ──
  await startDrag(page, stripButton(page, "left", "Binder"));
  // All six sections are visible drop targets while dragging.
  for (const zone of [
    "left.start",
    "left.end",
    "right.start",
    "right.end",
    "bottom.start",
    "bottom.end",
  ]) {
    await expect(dropZone(page, zone)).toBeVisible();
  }
  await dragOver(page, dropZone(page, "bottom.end"));
  await page.screenshot({ path: "/tmp/shell87-mid-drag.png" });
  await page.mouse.up();

  // Ghost gone; icon moved to the bottom strip; window re-opened down there.
  await expect(page.locator(".shell-drag-ghost")).toHaveCount(0);
  await expect(page.locator(".shell-strip-dropzone")).toHaveCount(0);
  await expect(stripButton(page, "left", "Binder")).toHaveCount(0);
  const bottomBinderBtn = stripButton(page, "bottom", "Binder");
  await expect(bottomBinderBtn).toBeVisible();
  await expect(
    page.locator('.shell-dock-bottom [data-toolwindow="binder"]'),
  ).toBeVisible();
  await page.screenshot({ path: "/tmp/shell87-post-drop.png" });

  // Placement survives toggling: close via a plain click, reopen — it comes
  // back in the bottom dock (acceptance criterion).
  await bottomBinderBtn.click();
  await expect(page.locator('[data-toolwindow="binder"]')).toHaveCount(0);
  await bottomBinderBtn.click();
  await expect(
    page.locator('.shell-dock-bottom [data-toolwindow="binder"]'),
  ).toBeVisible();

  // ── Bottom → back to left.start ──
  await startDrag(page, stripButton(page, "bottom", "Binder"));
  await dragOver(page, dropZone(page, "left.start"));
  await page.mouse.up();
  await expect(stripButton(page, "left", "Binder")).toBeVisible();
  await expect(stripButton(page, "bottom", "Binder")).toHaveCount(0);
  await expect(page.locator('.shell-dock-left [data-toolwindow="binder"]')).toBeVisible();
});

test("Escape mid-drag cancels: ghost gone, nothing moved", async ({ page }) => {
  await startDrag(page, stripButton(page, "left", "Binder"));
  await dragOver(page, dropZone(page, "bottom.end"));
  await page.keyboard.press("Escape");

  await expect(page.locator(".shell-drag-ghost")).toHaveCount(0);
  await expect(page.locator(".shell-strip-dropzone")).toHaveCount(0);
  await page.mouse.up();
  await expect(stripButton(page, "left", "Binder")).toBeVisible();
  await expect(page.locator('.shell-dock-left [data-toolwindow="binder"]')).toBeVisible();

  // The click fired by that pointerup is suppressed — Binder stays open.
  await expect(page.locator('[data-toolwindow="binder"]')).toBeVisible();
});

test("dropping on the section a window already occupies is a no-op", async ({ page }) => {
  await startDrag(page, stripButton(page, "left", "Binder"));
  await dragOver(page, dropZone(page, "left.start"));
  await page.mouse.up();

  await expect(stripButton(page, "left", "Binder")).toBeVisible();
  await expect(page.locator('.shell-dock-left [data-toolwindow="binder"]')).toBeVisible();
});

test("a plain click (no movement) still toggles", async ({ page }) => {
  const binderBtn = stripButton(page, "left", "Binder");
  await binderBtn.click();
  await expect(page.locator('[data-toolwindow="binder"]')).toHaveCount(0);
  await binderBtn.click();
  await expect(page.locator('[data-toolwindow="binder"]')).toBeVisible();
});
