import { test, expect, type Page } from "@playwright/test";

/**
 * Knot/stitch Rename (#305) — the shared symbol context menu's "Rename…" item
 * opens a safe-by-default prompt: a clean rename applies immediately; a rename
 * that would introduce diagnostics flips to a breakage report whose only
 * override is an explicit "Force rename".
 */

function binderKnot(page: Page, name: string) {
  return page.locator(".brink-binder-knot", {
    has: page.locator(".brink-binder-label", { hasText: new RegExp(`^${name}$`) }),
  });
}

async function openRename(page: Page, knot: string): Promise<void> {
  await binderKnot(page, knot).first().click({ button: "right" });
  const item = page.locator(".brink-context-menu-item", { hasText: "Rename" });
  await expect(item).toBeVisible();
  await item.click();
  await expect(page.locator("#brink-rename-input")).toBeVisible();
}

test.describe("knot/stitch rename (#305)", () => {
  test("a clean rename applies and the binder shows the new name", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });
    await expect(binderKnot(page, "barter")).toHaveCount(1);

    await openRename(page, "barter");
    await page.locator("#brink-rename-input").fill("haggle");
    await page.keyboard.press("Enter");

    // Prompt closes; the binder outline refreshes with the renamed knot.
    await expect(page.locator("#brink-rename-input")).toBeHidden();
    await expect(binderKnot(page, "haggle")).toHaveCount(1);
    await expect(binderKnot(page, "barter")).toHaveCount(0);
  });

  test("F2 in the editor opens the inline rename seeded at the cursor (#323)", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    // Open a knot in the editor, then click its name token in the header to
    // place the cursor squarely on the symbol (robust to CM line layout).
    await binderKnot(page, "barter").first().click();
    await page.waitForSelector(".cm-content");
    const nameToken = page.locator(".cm-content").getByText("barter", { exact: true }).first();
    await expect(nameToken).toBeVisible();
    await nameToken.click();
    await page.keyboard.press("F2");

    // The INLINE rename widget mounts in the editor (not the modal), seeded with
    // the current name — proof F2 resolved a renameable symbol and routed
    // through the in-editor surface (#323).
    const input = page.locator(".brink-inline-rename-input");
    await expect(input).toBeVisible();
    await expect(input).toHaveValue("barter");
    await expect(page.locator("#brink-rename-input")).toHaveCount(0); // no modal
    await input.fill("haggling");
    await page.keyboard.press("Enter");

    // Widget tears down; the safe rename applied (binder outline reflects it).
    await expect(input).toHaveCount(0);
    await expect(binderKnot(page, "haggling")).toHaveCount(1);
    await expect(binderKnot(page, "barter")).toHaveCount(0);

    // The open symbol-view tab survives its own rename: it re-keys (tab label
    // follows the new name) and the view re-resolves to the renamed knot rather
    // than degrading to the full file (#305 follow-up).
    await expect(page.locator(".brink-tab-label", { hasText: "haggling" })).toHaveCount(1);
    await expect(page.locator(".brink-tab-label", { hasText: /^barter\b/ })).toHaveCount(0);
    await expect(
      page.locator(".cm-line", { hasText: "=== haggling ===" }).first(),
    ).toBeVisible();
  });

  test("inline rename shows '⚠ breaks N' + inline report; Rename anyway overrides (#324)", async ({
    page,
  }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    // Open `threshold` in the editor and F2 on its name token.
    await binderKnot(page, "threshold").first().click();
    await page.waitForSelector(".cm-content");
    const nameToken = page.locator(".cm-content").getByText("threshold", { exact: true }).first();
    await expect(nameToken).toBeVisible();
    await nameToken.click();
    await page.keyboard.press("F2");

    const input = page.locator(".brink-inline-rename-input");
    await expect(input).toBeVisible();
    // Rename `threshold` onto the existing `intro` knot → duplicate-knot break.
    await input.fill("intro");

    // The badge appears (debounced) with the breakage count, and clicking it
    // expands the INLINE report (not a modal).
    const badge = page.locator(".brink-inline-rename-badge");
    await expect(badge).toBeVisible();
    await expect(badge).toContainText(/breaks \d+/);
    await badge.click();
    const report = page.locator(".brink-inline-rename-report");
    await expect(report).toBeVisible();
    await expect(report.locator(".brink-inline-rename-report-item")).not.toHaveCount(0);
    await expect(page.locator("#brink-rename-input")).toHaveCount(0); // no modal

    // Still not applied — `threshold` is intact.
    await expect(binderKnot(page, "threshold")).toHaveCount(1);

    // "Rename anyway" overrides; the rename applies (now two `intro`).
    await page.locator(".brink-inline-rename-force").click();
    await expect(input).toHaveCount(0);
    await expect(binderKnot(page, "threshold")).toHaveCount(0);
    await expect(binderKnot(page, "intro")).toHaveCount(2);
  });

  test("a colliding rename shows the breakage report; Force overrides", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    // Rename `threshold` onto the existing `intro` knot → duplicate-knot breakage.
    await openRename(page, "threshold");
    await page.locator("#brink-rename-input").fill("intro");
    await page.keyboard.press("Enter");

    // Safe-by-default: the rename is blocked and the report is shown instead.
    const report = page.locator(".brink-rename-report");
    await expect(report).toBeVisible();
    await expect(report).toContainText(/would break/i);
    await expect(report.locator(".brink-rename-diag")).not.toHaveCount(0);
    // Still not applied — `threshold` is intact.
    await expect(binderKnot(page, "threshold")).toHaveCount(1);

    // Force overrides; the report closes and the rename applies (now two `intro`).
    await page.locator(".brink-rename-force").click();
    await expect(report).toBeHidden();
    await expect(binderKnot(page, "threshold")).toHaveCount(0);
    await expect(binderKnot(page, "intro")).toHaveCount(2);
  });
});
