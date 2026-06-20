import { test, expect, type Page } from "@playwright/test";

/**
 * "Play from here" (#186) — start a fresh play session entered at a knot/stitch
 * from the binder context menu and the editor gutter ▶. Opening a session adds
 * an entry to the status-bar session picker labeled with the ink path.
 */

async function sessionOptionCount(page: Page): Promise<number> {
  return page.locator(".brink-session-select option").count();
}

async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

test.describe("play from here (#186)", () => {
  test("binder context menu opens a session at the knot", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    const knot = page.locator(".brink-binder-knot").first();
    const knotName = ((await knot.locator(".brink-binder-label").textContent()) ?? "").trim();
    expect(knotName).not.toBe("");

    await knot.click({ button: "right" });
    const item = page.locator(".brink-context-menu-item", { hasText: "Play from here" });
    await expect(item).toBeVisible();
    await item.click();

    // A session entered at the knot is registered, labeled with its ink path.
    await expect(
      page.locator(".brink-session-select option", { hasText: knotName }),
    ).toHaveCount(1);
  });

  test("editor gutter ▶ on a knot header opens a session", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content");

    const before = await sessionOptionCount(page);

    // Hover a knot header line to reveal its gutter run-icon, then click it.
    // (A hidden measurement spacer also carries the class, so target `:visible`.)
    const header = page.locator(".cm-line").filter({ hasText: /^===/ }).first();
    await header.hover();
    const icon = page.locator(".brink-play-gutter-icon:visible").first();
    await expect(icon).toBeVisible();
    await icon.click();

    await expect
      .poll(async () => sessionOptionCount(page))
      .toBeGreaterThan(before);
  });

  test("story graph node right-click opens a session at the knot", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });

    await runPaletteCommand(page, "Story: Open Story Graph");
    await expect(page.locator(".brink-story-graph")).toBeVisible({ timeout: 10000 });
    const node = page.locator('[data-graph-node="intro"]');
    await expect(node).toBeAttached();

    await node.click({ button: "right" });
    const item = page.locator(".brink-context-menu-item", { hasText: "Play from here" });
    await expect(item).toBeVisible();
    await item.click();

    await expect(
      page.locator(".brink-session-select option", { hasText: "intro" }),
    ).toHaveCount(1);
  });
});
