/**
 * Editor groups e2e (issue #90, spec §7.8).
 *
 * Real-input flows over the split editor area: Mod-\ duplicates the active
 * editor into a new group, same-document views live-mirror, tabs move
 * between groups (palette command), closing a duplicate collapses its group
 * and keeps the survivor editable, and the open/reveal policy focuses an
 * existing tab instead of duplicating (binder click).
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

function group(page: Page, index: number): Locator {
  return page.locator(".shell-editor-group").nth(index);
}

function tabsIn(g: Locator): Locator {
  return g.locator(".brink-tab .brink-tab-label");
}

function editorIn(g: Locator): Locator {
  return g.locator(".cm-content");
}

async function editorText(g: Locator): Promise<string> {
  return (await editorIn(g).textContent()) ?? "";
}

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("Meta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

test.describe("editor groups", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await page.waitForSelector(".brink-knot-header", { timeout: 5000 });

    // The default layout opens the Player document in a right split (#120).
    // These specs exercise raw group mechanics from a single-group baseline;
    // the two-up itself is covered by player-document.spec.ts.
    const playerTab = page.locator(".brink-tab", { hasText: "Player" });
    await playerTab.hover();
    await playerTab.locator(".brink-tab-close").click();
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
  });

  test("Mod-\\ splits with the same file in both groups", async ({ page }) => {
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);

    await editorIn(group(page, 0)).click();
    await page.keyboard.press("Meta+\\");

    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink"]);
    await expect(tabsIn(group(page, 1))).toHaveText(["main.ink"]);

    // Same document, same content; the new (right) group is focused.
    expect(await editorText(group(page, 1))).toBe(await editorText(group(page, 0)));
    await expect(group(page, 1)).toHaveAttribute("data-focused", "true");
  });

  test("typing in the left view live-appears in the right", async ({ page }) => {
    await editorIn(group(page, 0)).click();
    await page.keyboard.press("Meta+\\");
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);

    // Focus the LEFT view and type at the end of the first line.
    const left = group(page, 0);
    const right = group(page, 1);
    await editorIn(left).click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.type(" XYZZY");

    await expect(editorIn(right)).toContainText("XYZZY");
    expect(await editorText(right)).toBe(await editorText(left));

    // And the other direction: type in the right view.
    await editorIn(right).click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.type(" PLUGH");
    await expect(editorIn(left)).toContainText("PLUGH");
    expect(await editorText(left)).toBe(await editorText(right));
  });

  test("moving a tab between groups via the palette command", async ({ page }) => {
    // Pin a second tab (the "opening" knot) next to main.ink in group 1.
    await page
      .locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" })
      .dblclick();
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink", "opening (main.ink)"]);

    // Activate main.ink, then move it to a (new) right group.
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await runPaletteCommand(page, "Editor: Move Tab to Right Group");

    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    await expect(tabsIn(group(page, 0))).toHaveText(["opening (main.ink)"]);
    await expect(tabsIn(group(page, 1))).toHaveText(["main.ink"]);
    await expect(group(page, 1)).toHaveAttribute("data-focused", "true");

    // And back: move it left into the first group.
    await runPaletteCommand(page, "Editor: Move Tab to Left Group");
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await expect(tabsIn(group(page, 0))).toHaveText([
      "opening (main.ink)",
      "main.ink",
    ]);
  });

  test("closing one duplicate keeps the other editable", async ({ page }) => {
    await editorIn(group(page, 0)).click();
    await page.keyboard.press("Meta+\\");
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);

    // Close the duplicate in the right group; the group collapses.
    const rightTab = group(page, 1).locator(".brink-tab");
    await rightTab.hover();
    await rightTab.locator(".brink-tab-close").click();
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);

    // The surviving view still edits (and still talks to the session).
    await editorIn(group(page, 0)).click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.type(" SURVIVOR");
    await expect(editorIn(group(page, 0))).toContainText("SURVIVOR");
    const fromView = await page.evaluate(() =>
      (window as any).__brinkView.state.doc.toString(),
    );
    expect(fromView).toContain("SURVIVOR");
  });

  test("binder click while the file is open in the other group focuses it (no duplicate)", async ({
    page,
  }) => {
    // Layout: group 1 = "opening" symbol tab, group 2 = main.ink.
    await page
      .locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" })
      .dblclick();
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await runPaletteCommand(page, "Editor: Move Tab to Right Group");
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);

    // Focus group 1 (click its symbol tab).
    await tabsIn(group(page, 0)).first().click();
    await expect(group(page, 0)).toHaveAttribute("data-focused", "true");

    // Click main.ink in the binder: it is open in group 2 → reveal, don't
    // duplicate into the focused group 1.
    await page
      .locator(".brink-binder-file-row .brink-binder-label", { hasText: "main.ink" })
      .click();

    await expect(group(page, 1)).toHaveAttribute("data-focused", "true");
    await expect(tabsIn(group(page, 0))).toHaveText(["opening (main.ink)"]);
    await expect(tabsIn(group(page, 1))).toHaveText(["main.ink"]);
    await expect(
      page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }),
    ).toHaveCount(1);
  });
});
