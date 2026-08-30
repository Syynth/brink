/**
 * Shared session bootstrap for e2e specs (W7/#3300 — RULED "no
 * auto-start": the startup compile leaves the Player idle, Run/Start
 * begins the session). Specs that need a RUNNING story call this after
 * load; the poll retry-clicks through the compile race (the
 * placeholder's Start does nothing until story bytes land).
 */
import { expect, type Page } from "@playwright/test";

export async function ensureStoryStarted(page: Page): Promise<void> {
  await expect
    .poll(
      async () => {
        const txt =
          (await page
            .locator(".story-text")
            .textContent()
            .catch(() => "")) ?? "";
        if (txt.trim().length > 0) return true;
        await page
          .locator(".session-placeholder-start")
          .click({ timeout: 500 })
          .catch(() => {});
        return false;
      },
      { timeout: 20_000 },
    )
    .toBe(true);
}
