/**
 * File rename/move e2e (#164 Stage 3). The default project is `main.ink`
 * (`INCLUDE toppled-temple.ink`) + `toppled-temple.ink`, so renaming the
 * included file exercises the real wasm `rename_file` op end-to-end: the
 * referrer's `INCLUDE` must rewrite to the new name and the project must still
 * compile (a broken include would raise an unresolved-include diagnostic).
 */

import { test, expect, type Page } from "@playwright/test";

async function gotoStudio(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector(".brink-binder-file-row", { timeout: 10000 });
}

function fileRow(page: Page, name: string) {
  return page.locator(".brink-binder-file-row", { hasText: name });
}

async function fileLabels(page: Page): Promise<string[]> {
  return page.locator(".brink-binder-file-row .brink-binder-label").allTextContents();
}

/** The Problems badge count, or 0 when absent (clean compile). */
async function problemCount(page: Page): Promise<number> {
  const badge = page
    .locator('.shell-strip-bottom .shell-strip-btn[aria-label="Problems"] .shell-strip-badge')
    .first();
  if ((await badge.count()) === 0) return 0;
  return Number((await badge.textContent()) ?? "0");
}

/** Open a file by its binder row and return the focused editor's text. */
async function openFileContent(page: Page, name: string): Promise<string> {
  await fileRow(page, name).locator(".brink-binder-label").click();
  await page.waitForTimeout(300); // single-click open timer
  return (await page.locator(".shell-editor-group .cm-content").first().textContent()) ?? "";
}

test.describe("file rename", () => {
  test.beforeEach(async ({ page }) => {
    await gotoStudio(page);
    // Sanity: the two-file INCLUDE project loaded and compiles clean.
    expect(await fileLabels(page)).toEqual(
      expect.arrayContaining(["main.ink", "toppled-temple.ink"]),
    );
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("context-menu Rename rewrites the referring INCLUDE and still compiles", async ({
    page,
  }) => {
    // Right-click the included file → Rename.
    await fileRow(page, "toppled-temple.ink").click({ button: "right" });
    await page
      .locator(".brink-context-menu-item")
      .filter({ hasText: /^Rename$/ })
      .click();

    // The in-row input appears; rename to temple.ink.
    const input = page.locator(".brink-binder-rename-input");
    await expect(input).toBeVisible();
    await input.fill("temple.ink");
    await input.press("Enter");

    // The binder reflects the new name; the old name is gone.
    await expect(fileRow(page, "temple.ink")).toBeVisible({ timeout: 5000 });
    await expect(fileRow(page, "toppled-temple.ink")).toHaveCount(0);

    // The referrer's INCLUDE rewrote to the new name…
    const main = await openFileContent(page, "main.ink");
    expect(main).toContain("INCLUDE temple.ink");
    expect(main).not.toContain("toppled-temple.ink");

    // …and the project still compiles (a broken include would raise one).
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("F2 renames the focused file row", async ({ page }) => {
    // Select the file (sets the binder's focused row), then F2 on the binder.
    await fileRow(page, "toppled-temple.ink").locator(".brink-binder-label").click();
    await page.waitForTimeout(300); // selection commit (single-click timer)
    await page.locator(".brink-binder").focus();
    await page.keyboard.press("F2");

    const input = page.locator(".brink-binder-rename-input");
    await expect(input).toBeVisible();
    await input.fill("ruins.ink");
    await input.press("Enter");

    await expect(fileRow(page, "ruins.ink")).toBeVisible({ timeout: 5000 });
    await expect(fileRow(page, "toppled-temple.ink")).toHaveCount(0);
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("renaming an open file re-keys its tab in place (not reopened)", async ({ page }) => {
    // Open the included file in a pinned editor tab.
    await fileRow(page, "toppled-temple.ink").locator(".brink-binder-label").dblclick();
    const group = page.locator(".shell-editor-group").first();
    await expect(
      group.locator(".brink-tab .brink-tab-label", { hasText: /^toppled-temple\.ink$/ }),
    ).toBeVisible();

    // Rename it.
    await fileRow(page, "toppled-temple.ink").click({ button: "right" });
    await page
      .locator(".brink-context-menu-item")
      .filter({ hasText: /^Rename$/ })
      .click();
    const input = page.locator(".brink-binder-rename-input");
    await input.fill("temple.ink");
    await input.press("Enter");

    // The same tab is re-labeled in place — exactly one temple.ink tab, no
    // leftover toppled-temple.ink tab, and the editor still shows its content.
    await expect(
      group.locator(".brink-tab .brink-tab-label", { hasText: /^temple\.ink$/ }),
    ).toHaveCount(1);
    await expect(
      group.locator(".brink-tab .brink-tab-label", { hasText: /^toppled-temple\.ink$/ }),
    ).toHaveCount(0);
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("Escape cancels an inline rename", async ({ page }) => {
    await fileRow(page, "toppled-temple.ink").click({ button: "right" });
    await page
      .locator(".brink-context-menu-item")
      .filter({ hasText: /^Rename$/ })
      .click();

    const input = page.locator(".brink-binder-rename-input");
    await expect(input).toBeVisible();
    await input.fill("nope.ink");
    await input.press("Escape");

    // Nothing changed.
    await expect(input).toHaveCount(0);
    await expect(fileRow(page, "toppled-temple.ink")).toBeVisible();
    await expect(fileRow(page, "nope.ink")).toHaveCount(0);
  });
});

// ── Move into a folder + folder rename (nested fixture) ─────────────

function folderRow(page: Page, name: string) {
  return page.locator(".brink-binder-folder-row", { hasText: name });
}

/** Drive an HTML5 drag of a file row onto a folder row via a shared
 *  DataTransfer (Chromium native DnD is unreliable through synthetic mouse).
 *  The dragover/drop carry the folder row's CENTER coordinates: since
 *  #3038, a folder's top/bottom 30% are sibling-REORDER zones and only the
 *  middle is move-into — a coordinate-less synthetic event lands at y=0,
 *  i.e. in the reorder zone. */
async function dragFileOntoFolder(page: Page, file: string, folder: string): Promise<void> {
  const src = fileRow(page, file);
  const dst = folderRow(page, folder);
  const dt = await page.evaluateHandle(() => new DataTransfer());
  const box = await dst.boundingBox();
  if (box === null) throw new Error("folder row has no box");
  const at = { clientX: box.x + box.width / 2, clientY: box.y + box.height / 2 };
  await src.dispatchEvent("dragstart", { dataTransfer: dt });
  await dst.dispatchEvent("dragover", { dataTransfer: dt, ...at });
  await dst.dispatchEvent("drop", { dataTransfer: dt, ...at });
  await src.dispatchEvent("dragend", { dataTransfer: dt });
}

test.describe("file move + folder rename (nested)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=nested");
    await page.waitForSelector(".brink-binder-folder-row", { timeout: 10000 });
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("drag a root file onto a folder moves it and rewrites the INCLUDE", async ({ page }) => {
    // helper.ink sits at root; scenes/ holds intro.ink. main.ink INCLUDEs both.
    await expect(folderRow(page, "scenes")).toBeVisible();
    await dragFileOntoFolder(page, "helper.ink", "scenes");

    // The referrer's INCLUDE now points into the folder, and it still compiles.
    const main = await openFileContent(page, "main.ink");
    expect(main).toContain("INCLUDE scenes/helper.ink");
    expect(main).not.toMatch(/INCLUDE helper\.ink\b/);
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("drag a nested file to the root zone moves it out of the folder", async ({ page }) => {
    const src = fileRow(page, "intro.ink"); // scenes/intro.ink
    const dt = await page.evaluateHandle(() => new DataTransfer());
    await src.dispatchEvent("dragstart", { dataTransfer: dt });

    // The "move to root" zone appears only while dragging a nested file.
    const zone = page.locator(".brink-binder-root-drop");
    await expect(zone).toBeVisible();
    await zone.dispatchEvent("dragover", { dataTransfer: dt });
    await zone.dispatchEvent("drop", { dataTransfer: dt });

    // The referrer INCLUDE rewrites to the root path and it still compiles.
    const main = await openFileContent(page, "main.ink");
    expect(main).toContain("INCLUDE intro.ink");
    expect(main).not.toContain("scenes/intro.ink");
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("multi-select then drag moves every selected file in one step", async ({ page }) => {
    // Ctrl/Cmd-click two root files to multi-select (no tab opens).
    await fileRow(page, "helper.ink")
      .locator(".brink-binder-label")
      .click({ modifiers: ["ControlOrMeta"] });
    await page.waitForTimeout(250); // single-click timer
    await fileRow(page, "util.ink")
      .locator(".brink-binder-label")
      .click({ modifiers: ["ControlOrMeta"] });
    await page.waitForTimeout(250);

    // Drag the selection onto the scenes folder.
    const dt = await page.evaluateHandle(() => new DataTransfer());
    await fileRow(page, "helper.ink").dispatchEvent("dragstart", { dataTransfer: dt });
    const dst = folderRow(page, "scenes");
    // Row-center coordinates: the middle is the move-into zone (#3038).
    const box = await dst.boundingBox();
    if (box === null) throw new Error("folder row has no box");
    const at = { clientX: box.x + box.width / 2, clientY: box.y + box.height / 2 };
    await dst.dispatchEvent("dragover", { dataTransfer: dt, ...at });
    await dst.dispatchEvent("drop", { dataTransfer: dt, ...at });

    // Both files moved (referrer INCLUDEs rewritten), one "Moved 2 files" toast.
    await expect(
      page.locator(".shell-notification", { hasText: "Moved 2 files" }),
    ).toBeVisible();
    const main = await openFileContent(page, "main.ink");
    expect(main).toContain("INCLUDE scenes/helper.ink");
    expect(main).toContain("INCLUDE scenes/util.ink");
    expect(main).not.toMatch(/INCLUDE helper\.ink/);
    expect(main).not.toMatch(/INCLUDE util\.ink/);
    await expect.poll(() => problemCount(page)).toBe(0);
  });

  test("renaming a folder re-keys its files and rewrites referrers", async ({ page }) => {
    await folderRow(page, "scenes").click({ button: "right" });
    await page
      .locator(".brink-context-menu-item")
      .filter({ hasText: /^Rename folder$/ })
      .click();

    const input = page.locator(".brink-binder-rename-input");
    await expect(input).toBeVisible();
    await input.fill("acts");
    await input.press("Enter");

    // The folder re-labels; the contained file's INCLUDE rewrites; compiles.
    await expect(folderRow(page, "acts")).toBeVisible({ timeout: 5000 });
    await expect(folderRow(page, "scenes")).toHaveCount(0);
    const main = await openFileContent(page, "main.ink");
    expect(main).toContain("INCLUDE acts/intro.ink");
    expect(main).not.toContain("scenes/intro.ink");
    await expect.poll(() => problemCount(page)).toBe(0);
  });
});
