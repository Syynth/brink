/**
 * Value-list picker typeahead (#211). The demo's `teleport(5, …)` types its
 * `map` arg as the `map_id` value-list (Harbor #1 / Old Temple #5 / Catacombs
 * #9), so the filled slot renders a clickable `⟨Old Temple⟩` chip. Clicking it
 * opens the searchable popover; the search filters on label, value, AND detail,
 * and picking inserts the id.
 */

import { test, expect, type Page } from "@playwright/test";

async function gotoStudio(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector(".cm-content", { timeout: 10000 });
}

test("value-list picker filters by label / value / detail and inserts the id", async ({
  page,
}) => {
  await gotoStudio(page);

  // map 5 → "Old Temple" chip.
  const chip = page.locator(".brink-value-chip", { hasText: "Old Temple" });
  await expect(chip).toBeVisible({ timeout: 10000 });
  await chip.click();

  const picker = page.locator(".brink-value-picker");
  await expect(picker).toBeVisible();
  const items = picker.locator(".brink-value-item");
  await expect(items).toHaveCount(3); // Harbor, Old Temple, Catacombs

  const filter = picker.locator(".brink-value-filter");

  // Detail-only term: "#9" appears only in Catacombs' detail ("Map #9") — not
  // in any label or value — so it must match via detail (#211).
  await filter.fill("#9");
  await expect(items).toHaveCount(1);
  await expect(items.first()).toContainText("Catacombs");

  // Label still filters.
  await filter.fill("harbor");
  await expect(items).toHaveCount(1);
  await expect(items.first()).toContainText("Harbor");

  // Picking inserts the id; the chip re-renders to the new selection's label.
  await filter.fill("catacombs");
  await items.first().click();
  await expect(page.locator(".brink-value-chip", { hasText: "Catacombs" })).toBeVisible({
    timeout: 5000,
  });
});
