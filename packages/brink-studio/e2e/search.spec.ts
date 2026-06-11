/**
 * Search tool window e2e (issue #94, spec §4).
 *
 * Real-input flows: Mod-Shift-F opens the window and focuses the query
 * input (and never closes it), Mod-6 is the generated toggle, live search
 * renders results grouped by file with match-count badges, clicking a row
 * navigates to the exact span (asserted via __brinkView selection),
 * case/regex toggles change result counts, an invalid regex shows an
 * inline error, per-match replace updates the open editor view, and
 * replace-all runs behind an inline confirmation and rewrites multiple
 * files.
 */

import { test, expect, type Page } from "@playwright/test";

async function openSearch(page: Page): Promise<void> {
  await page.keyboard.press("Meta+Shift+F");
  await expect(page.locator(".search-view")).toBeVisible();
}

/** Type a query and wait for the debounced search to render results. */
async function search(page: Page, query: string): Promise<void> {
  await page.locator(".search-input").fill(query);
  await expect(page.locator(".search-file-header").first()).toBeVisible();
}

/** The focused editor view's selection head (CM6, UTF-16 offsets). */
function selectionHead(page: Page): Promise<number> {
  return page.evaluate(
    () => (window as any).__brinkView.state.selection.main.head,
  );
}

/** The focused editor view's full document text. */
function editorDoc(page: Page): Promise<string> {
  return page.evaluate(() => (window as any).__brinkView.state.doc.toString());
}

test.describe("search tool window", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("Mod-Shift-F opens the window and focuses the query input; repeat never closes", async ({
    page,
  }) => {
    await openSearch(page);
    await expect(page.locator(".search-input")).toBeFocused();

    // search.focus is not a toggle — a second press keeps it open.
    await page.keyboard.press("Meta+Shift+F");
    await expect(page.locator(".search-view")).toBeVisible();
    await expect(page.locator(".search-input")).toBeFocused();
  });

  test("Mod-6 is the generated toggle (registered after the built-ins)", async ({
    page,
  }) => {
    await page.keyboard.press("Meta+6");
    await expect(page.locator(".search-view")).toBeVisible();
    await page.keyboard.press("Meta+6");
    await expect(page.locator(".search-view")).toHaveCount(0);
  });

  test("results are grouped by file with match counts", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    // Sorted file order: main.ink first, the included story second.
    const headers = page.locator(".search-file-header");
    await expect(headers).toHaveCount(2);
    await expect(headers.nth(0).locator(".search-file-path")).toHaveText("main.ink");
    await expect(headers.nth(0).locator(".search-file-count")).toHaveText("1");
    await expect(headers.nth(1).locator(".search-file-path")).toHaveText(
      "toppled-temple.ink",
    );
    await expect(headers.nth(1).locator(".search-file-count")).toHaveText("2");
    await expect(page.locator(".search-result-row")).toHaveCount(3);

    // Collapsing a file header hides its rows, keeps the others.
    await headers.nth(1).click();
    await expect(page.locator(".search-result-row")).toHaveCount(1);
  });

  test("clicking a result navigates to the exact span", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    await page.locator(".search-result-line").first().click();

    // main.ink is the focused view; the selection sits on its one "intro".
    const doc = await editorDoc(page);
    expect(doc).toContain("-> intro");
    expect(await selectionHead(page)).toBe(doc.indexOf("intro"));
  });

  test("Enter from the query input reveals the selected row", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    await page.keyboard.press("ArrowDown");
    await expect(page.locator(".search-result-row.selected")).toHaveCount(1);
    await page.keyboard.press("Enter");

    const doc = await editorDoc(page);
    expect(await selectionHead(page)).toBe(doc.indexOf("intro"));
  });

  test("case and regex toggles change result counts; invalid regex shows an inline error", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "the");
    const insensitive = await page.locator(".search-result-row").count();

    // Case-sensitive narrows the count (capitalized "The" drops out).
    await page.locator('.search-option[data-option="caseSensitive"]').click();
    await expect
      .poll(() => page.locator(".search-result-row").count())
      .toBeLessThan(insensitive);
    const sensitive = await page.locator(".search-result-row").count();
    expect(sensitive).toBeGreaterThan(0);
    await page.locator('.search-option[data-option="caseSensitive"]').click();

    // Literal "knot|gold" matches nothing; as a regex the alternation hits.
    await page.locator(".search-input").fill("knot|gold");
    await expect(page.locator(".search-empty")).toBeVisible();
    await page.locator('.search-option[data-option="regex"]').click();
    await expect(page.locator(".search-file-header").first()).toBeVisible();

    // Invalid regex: inline error, like the Settings JSON validation.
    await page.locator(".search-input").fill("(");
    await expect(page.locator(".search-error")).toContainText("Invalid regex");
  });

  test("replace-all behind an inline confirmation rewrites multiple files", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "intro");

    await page.locator(".search-replace-toggle").click();
    await page.locator(".search-replace-input").fill("prologue");
    await page.locator(".search-replace-all").click();

    // Nothing happens until the confirmation is accepted.
    const confirm = page.locator(".search-confirm");
    await expect(confirm).toContainText("Replace 3 matches in 2 files?");
    expect(await editorDoc(page)).toContain("-> intro");

    await page.locator(".search-confirm-yes").click();

    // The open editor view reflects the edit (invalidateFile refresh)…
    await expect
      .poll(() => editorDoc(page))
      .toContain("-> prologue");
    // …the toast reports the counts…
    await expect(
      page
        .locator(".shell-notification-message")
        .filter({ hasText: "Replaced 3 matches in 2 files" }),
    ).toBeVisible();
    // …and both rewritten files match the replacement text.
    await search(page, "prologue");
    await expect(page.locator(".search-file-header")).toHaveCount(2);
  });
});

test.describe("search replace (screenplay fixture)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("per-match replace updates the open editor view", async ({ page }) => {
    await openSearch(page);
    await search(page, "figure");

    await page.locator(".search-replace-toggle").click();
    await page.locator(".search-replace-input").fill("shadow");

    const row = page.locator(".search-result-row").first();
    await row.hover();
    await row.locator(".search-row-replace").click();

    // The mounted CM6 view of main.ink refreshes from the rewritten source.
    await expect
      .poll(() => editorDoc(page))
      .toContain("A shadow steps into the light.");
    await expect(page.locator(".cm-content")).toContainText("A shadow steps");
    // Results refreshed: the old query no longer matches anything.
    await expect(page.locator(".search-empty")).toBeVisible();
  });
});
