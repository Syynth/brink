import { expect, test, type Page } from "@playwright/test";

/**
 * #3497: the tooltip portal layer (#3349) is zero-width; CodeMirror's
 * fixed→absolute placement fallback then sized every tooltip against it and
 * cards collapsed to one word, wrapping mid-token. Pins that a diagnostic
 * hover card is a real card again.
 */
async function gotoFixable(page: Page): Promise<void> {
  await page.goto("/?fixture=fixable");
  await page.waitForSelector(".cm-content", { timeout: 15_000 });
  // Diagnostics land after the debounced compile.
  await page.waitForSelector(".cm-lintRange", { timeout: 30_000 });
}

test("a diagnostic hover tooltip is card-width, not one word wide", async ({ page }) => {
  await gotoFixable(page);
  const range = page.locator(".cm-lintRange").first();
  await range.scrollIntoViewIfNeeded();
  await range.hover();
  const tip = page.locator(".cm-tooltip").first();
  await expect(tip).toBeVisible({ timeout: 10_000 });
  // Structural pin: a shrink-to-fit box that collapsed against a 0px
  // containing block is far narrower than its own max-content width; a
  // healthy card equals it (bounded by --bs-tooltip-max-width). Compare the
  // live tooltip with a clone laid out at `width: max-content` under the
  // same root, so the assertion holds for any message length.
  const widths = await tip.evaluate((el) => {
    const root = el.closest(".brink-studio") ?? document.body;
    const clone = el.cloneNode(true) as HTMLElement;
    clone.style.position = "fixed";
    clone.style.left = "0px";
    clone.style.top = "0px";
    clone.style.width = "max-content";
    clone.style.visibility = "hidden";
    root.appendChild(clone);
    const maxContent = clone.getBoundingClientRect().width;
    clone.remove();
    return { actual: el.getBoundingClientRect().width, maxContent };
  });
  expect(widths.maxContent).toBeGreaterThan(0);
  expect(widths.actual).toBeGreaterThanOrEqual(widths.maxContent - 2);
});
