import { test, expect, type Page } from "@playwright/test";

// ── Helpers ────────────────────────────────────────────────────────

/** Wait for the binder to render with knot entries. */
async function waitForBinder(page: Page) {
  await page.waitForSelector(".brink-binder-knot", { timeout: 5000 });
}

/** Get all visible tab labels. */
async function getTabLabels(page: Page) {
  return page.locator(".brink-tab .brink-tab-label").allTextContents();
}

/** Get the active tab's label. */
async function getActiveTabLabel(page: Page) {
  return page.locator(".brink-tab.active .brink-tab-label").textContent();
}

/** Check if the active tab is unpinned (italic). */
async function isActiveTabUnpinned(page: Page) {
  return page.locator(".brink-tab.active.unpinned").count().then((n) => n > 0);
}

/** Get the text content of the CodeMirror editor. */
async function getEditorContent(page: Page) {
  return page.locator(".cm-content").textContent();
}

/** Get all binder knot labels. */
async function getKnotLabels(page: Page) {
  return page.locator(".brink-binder-knot .brink-binder-label").allTextContents();
}

/** Get all binder file labels. */
async function getFileLabels(page: Page) {
  return page.locator(".brink-binder-file-label").allTextContents();
}

// ── Tests ──────────────────────────────────────────────────────────

test.describe("binder", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForBinder(page);
  });

  test("renders file tree with knots", async ({ page }) => {
    const files = await getFileLabels(page);
    expect(files).toContain("main.ink");

    const knots = await getKnotLabels(page);
    expect(knots).toContain("opening");
    expect(knots).toContain("interrogation");
  });

  test("file has expand/collapse arrow", async ({ page }) => {
    const arrow = page.locator(".brink-binder-arrow").first();
    await expect(arrow).toBeVisible();
    // Should show down arrow (expanded)
    const text = await arrow.textContent();
    expect(text).toBe("\u25bc");
  });

  test("collapse hides knots", async ({ page }) => {
    const arrow = page.locator(".brink-binder-arrow").first();
    await arrow.click();

    // Knots should be hidden
    const knots = await getKnotLabels(page);
    expect(knots).toHaveLength(0);

    // Arrow should be right (collapsed)
    const text = await arrow.textContent();
    expect(text).toBe("\u25b6");
  });

  test("expand shows knots again", async ({ page }) => {
    const arrow = page.locator(".brink-binder-arrow").first();
    // Collapse
    await arrow.click();
    expect(await getKnotLabels(page)).toHaveLength(0);

    // Expand
    await arrow.click();
    const knots = await getKnotLabels(page);
    expect(knots.length).toBeGreaterThan(0);
  });
});

test.describe("binder → tab opening", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForBinder(page);
  });

  test("single-click on knot opens unpinned symbol tab", async ({ page }) => {
    // Click the "start" knot
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).click();

    // Wait for the click timer (200ms) + tab switch
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Should be unpinned (italic)
    const unpinned = await isActiveTabUnpinned(page);
    expect(unpinned).toBe(true);

    // Editor should show knot content, not the full file
    const content = await getEditorContent(page);
    expect(content).toContain("=== opening ===");
    expect(content).toContain("The lights dim.");
    // Should NOT contain the story knot (focused view)
    expect(content).not.toContain("=== interrogation ===");
  });

  test("single-click on different knot replaces unpinned tab", async ({ page }) => {
    // Click "start"
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).click();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Click "story"
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "interrogation" }).click();
    await expect(page.locator(".brink-tab", { hasText: "interrogation (main.ink)" })).toBeVisible({ timeout: 2000 });

    // "start" tab should be gone (replaced)
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toHaveCount(0);

    // Editor shows story content
    const content = await getEditorContent(page);
    expect(content).toContain("=== interrogation ===");
    expect(content).not.toContain("=== opening ===");
  });

  test("double-click on knot opens pinned tab", async ({ page }) => {
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();

    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Should be pinned (no .unpinned class)
    const unpinned = await page.locator(".brink-tab.active.unpinned").count();
    expect(unpinned).toBe(0);
  });

  test("single-click on file opens unpinned file tab", async ({ page }) => {
    // First open a knot to have something different active
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Now single-click the file in binder
    await page.locator(".brink-binder-file-label", { hasText: "main.ink" }).click();

    // Wait for timer + switch. The main.ink tab already exists and is pinned,
    // so clicking it should just switch to it.
    await page.waitForTimeout(300);

    const activeLabel = await getActiveTabLabel(page);
    expect(activeLabel).toBe("main.ink");
  });

  test("clicking arrow does not open tab", async ({ page }) => {
    const tabsBefore = await getTabLabels(page);

    // Click the arrow
    await page.locator(".brink-binder-arrow").first().click();

    await page.waitForTimeout(300);

    // No new tab should be created
    const tabsAfter = await getTabLabels(page);
    expect(tabsAfter).toEqual(tabsBefore);
  });
});

test.describe("tab pinning", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForBinder(page);
  });

  test("double-click on unpinned tab pins it", async ({ page }) => {
    // Single-click to create unpinned
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).click();
    await expect(page.locator(".brink-tab.unpinned")).toBeVisible({ timeout: 2000 });

    // Double-click the tab itself to pin
    await page.locator(".brink-tab.unpinned .brink-tab-label").dblclick();

    // Should no longer be unpinned
    await expect(page.locator(".brink-tab.unpinned")).toHaveCount(0);
  });

  test("editing in unpinned tab auto-pins it", async ({ page }) => {
    // Single-click to create unpinned
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).click();
    await expect(page.locator(".brink-tab.unpinned")).toBeVisible({ timeout: 2000 });

    // Type in the editor
    await page.locator(".cm-content").click();
    await page.keyboard.type("x");

    // Should auto-pin
    await expect(page.locator(".brink-tab.unpinned")).toHaveCount(0, { timeout: 1000 });
  });

  test("pinned tab survives when another unpinned tab opens", async ({ page }) => {
    // Pin start
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Single-click story (unpinned)
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "interrogation" }).click();
    await expect(page.locator(".brink-tab", { hasText: "interrogation (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Start tab should still exist
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible();

    // Should have 3 tabs: main.ink, start, story
    const labels = await getTabLabels(page);
    expect(labels).toHaveLength(3);
  });
});

test.describe("focused view content", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForBinder(page);
  });

  test("knot tab shows only that knot's content", async ({ page }) => {
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    const content = await getEditorContent(page);
    expect(content).toContain("=== opening ===");
    expect(content).toContain("The lights dim.");
    expect(content).not.toContain("=== interrogation ===");
    expect(content).not.toContain("// A short screenplay-style demo.");
  });

  test("editing file then clicking knot uses updated offsets", async ({ page }) => {
    // Add newlines at the top of the file (shifts all byte offsets)
    await page.locator(".cm-content").click();
    await page.keyboard.press("Home");
    await page.keyboard.press("Enter");
    await page.keyboard.press("Enter");
    await page.keyboard.press("Enter");

    // Wait briefly, then click "start" in the binder (before debounced compile fires)
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // The focused view should show the start knot, NOT the file preamble
    const content = await getEditorContent(page);
    expect(content).toContain("=== opening ===");
    expect(content).not.toContain("// A short screenplay-style demo.");
  });

  test("edits in focused view splice back correctly into full file", async ({ page }) => {
    // Open the start knot via binder (pinned)
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // The focused view should contain "=== opening ===" and end with a blank line before "=== interrogation ==="
    const focusedContent = await getEditorContent(page);
    expect(focusedContent).toContain("=== opening ===");
    expect(focusedContent).not.toContain("=== interrogation ===");

    // Move cursor to the last line and delete it (Cmd+End to go to end, then Backspace)
    await page.locator(".cm-content").click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.press("Backspace");

    // Wait for the edit to flush
    await page.waitForTimeout(100);

    // Switch back to the full file tab
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await page.waitForTimeout(200);

    const fullContent = await getEditorContent(page);

    // The full file should still have both knots
    expect(fullContent).toContain("=== opening ===");
    expect(fullContent).toContain("=== interrogation ===");
    // The preamble should be intact
    expect(fullContent).toContain("// A short screenplay-style demo.");
    expect(fullContent).toContain("-> opening");
    // The blank line between the opening knot's last line and interrogation knot should be gone.
    // getEditorContent returns text without newlines (CM renders lines as separate elements),
    // so "-> evidence" should be immediately followed by "=== interrogation ===" with no blank space.
    expect(fullContent).toMatch(/-> evidence=== interrogation ===/);
  });

  test("switching back to file tab shows full file", async ({ page }) => {
    // Open knot
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Switch back to file tab — click the label directly to avoid matching "opening (main.ink)"
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await page.waitForTimeout(100);

    const content = await getEditorContent(page);
    expect(content).toContain("=== opening ===");
    expect(content).toContain("=== interrogation ===");
    expect(content).toContain("// A short screenplay-style demo.");
  });
});
