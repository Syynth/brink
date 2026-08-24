import type { Page } from "@playwright/test";

/**
 * Switch the binder to STRUCTURE mode (#3036): Files mode is the ruled
 * default and hides knot/stitch rows, so any spec that drives symbol rows
 * must opt in first — exactly like a user would. Idempotent.
 */
export async function enterStructureMode(page: Page): Promise<void> {
  await page.waitForSelector(".brink-binder-toolbar", { timeout: 8000 });
  const btn = page.locator(".brink-binder-mode-toggle button[title='Structure']");
  const active = await btn
    .first()
    .evaluate((el) => el.classList.contains("active"))
    .catch(() => false);
  if (!active) await btn.click();
}
