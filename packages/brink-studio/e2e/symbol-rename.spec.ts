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

  test("F2 in the editor opens the safe rename prompt seeded at the cursor", async ({ page }) => {
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

    // The shared rename prompt opens (proof F2 resolved a renameable symbol and
    // routed through the store, not a native prompt()), seeded with the name.
    const input = page.locator("#brink-rename-input");
    await expect(input).toBeVisible();
    await expect(input).toHaveValue("barter");
    await input.fill("haggling");
    await page.keyboard.press("Enter");

    // Prompt closes; the rename applied (binder outline reflects it).
    await expect(input).toBeHidden();
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
