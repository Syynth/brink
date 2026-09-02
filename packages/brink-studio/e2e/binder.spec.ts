import { test, expect, type Page } from "@playwright/test";
import { enterStructureMode } from "./binder-mode";

// ── Helpers ────────────────────────────────────────────────────────

/** Wait for the binder, switch to STRUCTURE mode (the #3036 toggle —
 *  Files mode is the default and hides symbol rows), and wait for knots. */
async function waitForBinder(page: Page) {
  await enterStructureMode(page);
  await page.waitForSelector(".brink-binder-knot", { timeout: 5000 });
}

// The default layout is the Inky two-up (#120): the editor group on the
// left, the Player document in a right split. These specs are about the
// editor group, so the tab/editor helpers scope to the first group.
function editorGroup(page: Page) {
  return page.locator(".shell-editor-group").first();
}

/** Get the editor group's visible tab labels. */
async function getTabLabels(page: Page) {
  return editorGroup(page).locator(".brink-tab .brink-tab-label").allTextContents();
}

/** Get the editor group's active tab label. */
async function getActiveTabLabel(page: Page) {
  return editorGroup(page).locator(".brink-tab.active .brink-tab-label").textContent();
}

/** Check if the editor group's active tab is unpinned (italic). */
async function isActiveTabUnpinned(page: Page) {
  return editorGroup(page)
    .locator(".brink-tab.active.unpinned")
    .count()
    .then((n) => n > 0);
}

/** Get the text content of the CodeMirror editor. */
async function getEditorContent(page: Page) {
  return editorGroup(page).locator(".cm-content").textContent();
}

/** Get all binder knot labels. */
async function getKnotLabels(page: Page) {
  return page.locator(".brink-binder-knot .brink-binder-label").allTextContents();
}

/** Get all binder file labels. */
async function getFileLabels(page: Page) {
  return page.locator(".brink-binder-file-row .brink-binder-label").allTextContents();
}

/**
 * Close every tab in the editor group (`.brink-tab-close`, one at a time —
 * closing shifts indices, so re-querying each time is required).
 *
 * #3356 (RULED 2026-09-01): bootstrap always opens the entry file pinned
 * (mount.tsx), and a symbol NAVIGATION click reveals in place when its
 * file is already open rather than minting a fragment tab. A handful of
 * "tab pinning" specs below are about generic preview/pin behavior, not
 * about that reveal — they use this helper first so their knot clicks land
 * in the "file not open anywhere" case, which still mints a fresh unpinned
 * fragment tab exactly as before this PR.
 */
async function closeAllTabs(page: Page) {
  const closeButton = editorGroup(page).locator(".brink-tab-close").first();
  while (await closeButton.count()) {
    await closeButton.click();
  }
  await expect(editorGroup(page).locator(".brink-tab")).toHaveCount(0);
}

// ── Tests ──────────────────────────────────────────────────────────

test.describe("binder", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await waitForBinder(page);
  });

  test("renders file tree with knots", async ({ page }) => {
    const files = await getFileLabels(page);
    expect(files).toContain("main.ink");

    const knots = await getKnotLabels(page);
    expect(knots).toContain("opening");
    expect(knots).toContain("interrogation");
  });

  test("an expandable file's icon is the toggle (chevronless, 2026-08-23)", async ({ page }) => {
    // The icon carries the affordance and the state: filled droplet =
    // collapsed with content, outline = expanded (see icons.tsx).
    const icon = page.locator(".brink-binder-file-row .brink-binder-icon.toggle").first();
    await expect(icon).toBeVisible();
  });

  test("collapse hides knots", async ({ page }) => {
    const icon = page.locator(".brink-binder-file-row .brink-binder-icon.toggle").first();
    await icon.click();

    // Knots should be hidden
    const knots = await getKnotLabels(page);
    expect(knots).toHaveLength(0);
  });

  test("expand shows knots again", async ({ page }) => {
    const icon = page.locator(".brink-binder-file-row .brink-binder-icon.toggle").first();
    // Collapse
    await icon.click();
    expect(await getKnotLabels(page)).toHaveLength(0);

    // Expand
    await icon.click();
    const knots = await getKnotLabels(page);
    expect(knots.length).toBeGreaterThan(0);
  });
});

test.describe("binder → tab opening", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await waitForBinder(page);
  });

  // #3356 (RULED 2026-09-01): a single click is a NAVIGATION (pinned ===
  // false) open. The screenplay fixture is deliberately single-file (see
  // `SCREENPLAY_FIXTURE`'s comment in main.tsx) and bootstrap always opens
  // the entry file pinned (mount.tsx), so `main.ink` is already open before
  // any binder click here — the ruled behavior is to reveal the knot in
  // place inside that tab rather than mint a `opening (main.ink)` fragment
  // tab. A pinned (double-click) open is unaffected — see the "double-click
  // on knot opens pinned tab" test below — because it is excluded from this
  // reveal (`openSymbolTarget` in mount.tsx).
  test("single-click on knot reveals in place when its file is already open", async ({ page }) => {
    // Bootstrap: only the pinned entry-file tab, nothing else.
    expect(await getTabLabels(page)).toEqual(["main.ink"]);

    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).click();

    // Wait for the click timer (200ms) + the reveal to land.
    await expect(editorGroup(page).locator(".cm-activeLine")).toContainText("opening", {
      timeout: 2000,
    });

    // No fragment tab was minted — still just the one, still pinned.
    expect(await getTabLabels(page)).toEqual(["main.ink"]);
    expect(await getActiveTabLabel(page)).toBe("main.ink");
    expect(await isActiveTabUnpinned(page)).toBe(false);

    // The reveal scrolls/selects within the WHOLE file (no narrowed focused
    // view) — both knots remain visible.
    const content = await getEditorContent(page);
    expect(content).toContain("=== opening ===");
    expect(content).toContain("=== interrogation ===");
  });

  test("single-click on a different knot updates the reveal, still in the same tab", async ({ page }) => {
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).click();
    await expect(editorGroup(page).locator(".cm-activeLine")).toContainText("opening", {
      timeout: 2000,
    });

    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "interrogation" }).click();
    await expect(editorGroup(page).locator(".cm-activeLine")).toContainText("interrogation", {
      timeout: 2000,
    });

    // Still exactly one tab throughout — no fragment tab appeared for
    // either click.
    expect(await getTabLabels(page)).toEqual(["main.ink"]);
    const content = await getEditorContent(page);
    expect(content).toContain("=== opening ===");
    expect(content).toContain("=== interrogation ===");
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
    await page.locator(".brink-binder-file-row .brink-binder-label", { hasText: "main.ink" }).click();

    // Wait for timer + switch. The main.ink tab already exists and is pinned,
    // so clicking it should just switch to it.
    await page.waitForTimeout(300);

    const activeLabel = await getActiveTabLabel(page);
    expect(activeLabel).toBe("main.ink");
  });

  test("clicking the toggle icon does not open a tab", async ({ page }) => {
    const tabsBefore = await getTabLabels(page);

    // Click an expandable row's icon (the chevronless toggle).
    await page.locator(".brink-binder-icon.toggle").first().click();

    await page.waitForTimeout(300);

    // No new tab should be created
    const tabsAfter = await getTabLabels(page);
    expect(tabsAfter).toEqual(tabsBefore);
  });
});

test.describe("tab pinning", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await waitForBinder(page);
  });

  test("double-click on unpinned tab pins it", async ({ page }) => {
    // #3356: main.ink is already open (pinned) at bootstrap, so close it
    // first — a knot click while its file is already open reveals in place
    // rather than minting the fragment tab this test is about.
    await closeAllTabs(page);

    // Single-click to create unpinned
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).click();
    await expect(page.locator(".brink-tab.unpinned")).toBeVisible({ timeout: 2000 });

    // Double-click the tab itself to pin
    await page.locator(".brink-tab.unpinned .brink-tab-label").dblclick();

    // Should no longer be unpinned
    await expect(page.locator(".brink-tab.unpinned")).toHaveCount(0);
  });

  test("editing in unpinned tab auto-pins it", async ({ page }) => {
    // #3356: see the previous test's comment.
    await closeAllTabs(page);

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
    // #3356: close main.ink first — otherwise it being already open would
    // make the "interrogation" click below reveal in place instead of
    // opening the unpinned fragment tab this test checks survives.
    await closeAllTabs(page);

    // Pin start
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "opening" }).dblclick();
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Single-click story (unpinned)
    await page.locator(".brink-binder-knot .brink-binder-label", { hasText: "interrogation" }).click();
    await expect(page.locator(".brink-tab", { hasText: "interrogation (main.ink)" })).toBeVisible({ timeout: 2000 });

    // Start tab should still exist
    await expect(page.locator(".brink-tab", { hasText: "opening (main.ink)" })).toBeVisible();

    // Should have 2 tabs: opening (pinned) and interrogation (unpinned) —
    // main.ink itself was closed above, so it is not a third tab here.
    const labels = await getTabLabels(page);
    expect(labels).toHaveLength(2);
  });
});

test.describe("focused view content", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
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
    await page.keyboard.press("ControlOrMeta+End");
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
    // so "-> interrogation.evidence" should be immediately followed by "=== interrogation ==="
    // with no blank space.
    expect(fullContent).toMatch(/-> interrogation\.evidence=== interrogation ===/);
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
