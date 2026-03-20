import { test, expect, type Page } from "@playwright/test";

/** Wait for the binder to render with knot entries. */
async function waitForBinder(page: Page) {
  await page.waitForSelector(".brink-binder-knot", { timeout: 5000 });
}

/** Get all visible tab labels. */
async function getTabLabels(page: Page) {
  return page.locator(".brink-tab .brink-tab-label").allTextContents();
}

/** Get the text content of the CodeMirror editor. */
async function getEditorContent(page: Page) {
  return page.locator(".cm-content").textContent();
}

/** Get all binder knot labels. */
async function getKnotLabels(page: Page) {
  return page.locator(".brink-binder-knot .brink-binder-label").allTextContents();
}

/** Get all binder stitch labels. */
async function getStitchLabels(page: Page) {
  return page.locator(".brink-binder-stitch .brink-binder-label").allTextContents();
}

const INK_WITH_STITCHES = `// Test file

-> start

=== start ===
Hello, world!

= stitch_1
content one

= stitch_2
content two

=== story ===
Once upon a time.
-> END
`;

/** Type the ink content into the editor, replacing everything. */
async function setEditorContent(page: Page, content: string) {
  // Use the exposed EditorView to dispatch a content replacement.
  // keyboard.type eats consecutive newlines in CM's input handling.
  await page.evaluate((text) => {
    const view = (window as any).__brinkView;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    });
  }, content);
  // Wait for debounced compile to fire and binder to refresh
  await page.waitForTimeout(700);
}

test.describe("stitches in binder", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForBinder(page);
  });

  test("binder shows stitches under their parent knot", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);

    const knots = await getKnotLabels(page);
    expect(knots).toContain("start");
    expect(knots).toContain("story");

    const stitches = await getStitchLabels(page);
    expect(stitches).toContain("stitch_1");
    expect(stitches).toContain("stitch_2");
  });

  test("all knots remain visible when stitches are present", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);

    // Both knots must be in the binder — story should NOT disappear
    const knots = await getKnotLabels(page);
    expect(knots).toContain("start");
    expect(knots).toContain("story");
  });

  test("clicking a stitch opens focused view of just that stitch", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);

    // Click stitch_1
    await page.locator(".brink-binder-stitch .brink-binder-label", { hasText: "stitch_1" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "stitch_1" })).toBeVisible({ timeout: 2000 });

    const content = await getEditorContent(page);
    expect(content).toContain("= stitch_1");
    expect(content).toContain("content one");
    // Should NOT contain the other stitch or other knots
    expect(content).not.toContain("= stitch_2");
    expect(content).not.toContain("=== story ===");
    expect(content).not.toContain("=== start ===");
  });

  test("clicking stitch_2 opens focused view of just that stitch", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);
    await page.waitForSelector(".brink-binder-stitch", { timeout: 3000 });

    await page.locator(".brink-binder-stitch .brink-binder-label", { hasText: "stitch_2" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "stitch_2" })).toBeVisible({ timeout: 2000 });

    const content = await getEditorContent(page);
    expect(content).toContain("= stitch_2");
    expect(content).toContain("content two");
    expect(content).not.toContain("= stitch_1");
    expect(content).not.toContain("=== story ===");
  });

  test("switching between stitch and file tab preserves content", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);

    // Open stitch_1 focused view
    await page.locator(".brink-binder-stitch .brink-binder-label", { hasText: "stitch_1" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "stitch_1" })).toBeVisible({ timeout: 2000 });

    // Switch back to full file
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await page.waitForTimeout(200);

    const fullContent = await getEditorContent(page);
    // Full file should have everything intact
    expect(fullContent).toContain("=== start ===");
    expect(fullContent).toContain("= stitch_1");
    expect(fullContent).toContain("= stitch_2");
    expect(fullContent).toContain("=== story ===");
    expect(fullContent).toContain("Once upon a time.");
  });

  test("editing in stitch focused view splices back correctly", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);

    // Open stitch_1
    await page.locator(".brink-binder-stitch .brink-binder-label", { hasText: "stitch_1" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "stitch_1" })).toBeVisible({ timeout: 2000 });

    // Edit: go to end and add text
    await page.locator(".cm-content").click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.type("added text");

    await page.waitForTimeout(100);

    // Switch back to file tab
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await page.waitForTimeout(200);

    const fullContent = await getEditorContent(page);
    expect(fullContent).toContain("added text");
    expect(fullContent).toContain("= stitch_2");
    expect(fullContent).toContain("=== story ===");
    expect(fullContent).toContain("// Test file");
  });

  test("typing at end of last stitch does not merge with next knot", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);
    await page.waitForSelector(".brink-binder-stitch", { timeout: 3000 });

    // Open stitch_2 (the LAST stitch before === story ===)
    await page.locator(".brink-binder-stitch .brink-binder-label", { hasText: "stitch_2" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "stitch_2" })).toBeVisible({ timeout: 2000 });

    // Go to end and type -> DONE
    await page.locator(".cm-content").click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.press("Enter");
    await page.keyboard.type("-> DONE");

    await page.waitForTimeout(100);

    // Switch back to file tab
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await page.waitForTimeout(200);

    // "-> DONE" and "=== story ===" must be on separate lines, not merged
    // (textContent() strips newlines, so we check line-by-line)
    const doneLineText = await page.locator(".cm-line", { hasText: "-> DONE" }).textContent();
    expect(doneLineText?.trim()).toBe("-> DONE");
    // Story knot must exist on its own line
    await expect(page.locator(".cm-line", { hasText: "=== story ===" })).toBeVisible();
    // Both sections should exist
    const fullContent = await getEditorContent(page);
    expect(fullContent).toContain("-> DONE");
    expect(fullContent).toContain("=== story ===");
    expect(fullContent).toContain("Once upon a time.");
  });

  test("typing at end of first stitch does not merge with second stitch", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);

    // Open stitch_1
    await page.locator(".brink-binder-stitch .brink-binder-label", { hasText: "stitch_1" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "stitch_1" })).toBeVisible({ timeout: 2000 });

    // Go to end and type -> DONE
    await page.locator(".cm-content").click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.press("Enter");
    await page.keyboard.type("-> DONE");

    await page.waitForTimeout(100);

    // Switch back to file tab
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await page.waitForTimeout(200);

    const fullContent = await getEditorContent(page);

    // "-> DONE" and "= stitch_2" must be on separate lines
    const doneLineText = await page.locator(".cm-line", { hasText: "-> DONE" }).textContent();
    expect(doneLineText?.trim()).toBe("-> DONE");
    await expect(page.locator(".cm-line", { hasText: "= stitch_2" })).toBeVisible();
    expect(fullContent).toContain("-> DONE");
    expect(fullContent).toContain("= stitch_2");
  });

  test("opening stitch then knot then file all work without corruption", async ({ page }) => {
    await setEditorContent(page, INK_WITH_STITCHES);

    // Open stitch_1
    await page.locator(".brink-binder-stitch .brink-binder-label", { hasText: "stitch_1" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "stitch_1" })).toBeVisible({ timeout: 2000 });
    let content = await getEditorContent(page);
    expect(content).toContain("= stitch_1");

    // Open start knot
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "start" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "start (main.ink)" })).toBeVisible({ timeout: 2000 });
    content = await getEditorContent(page);
    expect(content).toContain("=== start ===");
    expect(content).toContain("= stitch_1");
    expect(content).toContain("= stitch_2");
    expect(content).not.toContain("=== story ===");

    // Open file tab
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^main\.ink$/ }).click();
    await page.waitForTimeout(200);
    content = await getEditorContent(page);
    expect(content).toContain("=== start ===");
    expect(content).toContain("=== story ===");
    expect(content).toContain("= stitch_1");
    expect(content).toContain("= stitch_2");
    expect(content).toContain("// Test file");
  });
});
