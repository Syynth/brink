/**
 * Search tool window e2e (issue #94, spec §4; editable results buffer #322 D).
 *
 * The read-only match tree was replaced by the editor-owned *editable* results
 * buffer (design D): a synthetic CodeMirror document — one header line per file
 * (`path (N)`) followed by a match line per match (`  <line>: <source>`).
 * Assertions therefore read the buffer's document (via the `__brinkSearchBufferView`
 * hook, since CM6 only renders the viewport into the DOM) rather than tree DOM.
 *
 * Real-input flows: Mod-Shift-F opens the window and focuses the query input
 * (and never closes it), Mod-6 is the generated toggle, live search renders
 * results grouped by file with match counts, revealing a match navigates to the
 * exact span (asserted via __brinkView selection), case/regex toggles change
 * result counts, an invalid regex shows an inline error, editing a match row
 * rewrites the source (per-match replace), and replace-all runs behind an inline
 * confirmation and rewrites multiple files.
 */

import { test, expect, type Page } from "@playwright/test";

async function openSearch(page: Page): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+F");
  await expect(page.locator(".search-view")).toBeVisible();
}

/** The results buffer's full synthetic document text (header + match lines).
 *  CM6 only mounts the viewport into the DOM, so full-document assertions read
 *  the view's state through the e2e hook, like editorDoc does for __brinkView. */
function bufferDoc(page: Page): Promise<string> {
  return page.evaluate(() => {
    const view = (window as any).__brinkSearchBufferView;
    return view ? (view.state.doc.toString() as string) : "";
  });
}

/** Header lines in the buffer, one per matched file: `path (N)`. */
async function fileHeaders(page: Page): Promise<string[]> {
  const doc = await bufferDoc(page);
  return doc.split("\n").filter((line) => /^\S.*\(\d+\)$/.test(line));
}

/** Match lines in the buffer: `  <line>: <source>` (two-space indent + `N: `). */
async function matchLines(page: Page): Promise<string[]> {
  const doc = await bufferDoc(page);
  return doc.split("\n").filter((line) => /^ {2}\d+: /.test(line));
}

/** Type a query and wait for the debounced search to render the buffer. */
async function search(page: Page, query: string): Promise<void> {
  await page.locator(".search-input").fill(query);
  await expect(page.locator(".search-results-buffer .cm-content")).toBeVisible();
  await expect.poll(() => matchLines(page).then((m) => m.length)).toBeGreaterThan(0);
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

/**
 * The buffer `.cm-line` element whose text starts with `  <line>: `. Match rows
 * are plain CM6 lines (no per-row class); we locate them by their rendered
 * text. The line must be in the viewport — the fixtures are tiny, so every row
 * is rendered.
 */
function matchRowLocator(page: Page, index = 0) {
  return page
    .locator(".search-results-buffer .cm-line")
    .filter({ hasText: /^\s*\d+:\s/ })
    .nth(index);
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
    await page.keyboard.press("ControlOrMeta+Shift+F");
    await expect(page.locator(".search-view")).toBeVisible();
    await expect(page.locator(".search-input")).toBeFocused();
  });

  test("Mod-6 is the generated toggle (registered after the built-ins)", async ({
    page,
  }) => {
    await page.keyboard.press("ControlOrMeta+6");
    await expect(page.locator(".search-view")).toBeVisible();
    await page.keyboard.press("ControlOrMeta+6");
    await expect(page.locator(".search-view")).toHaveCount(0);
  });

  test("results are grouped by file with match counts", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    // Sorted file order in the buffer: main.ink header first (1 match), the
    // included story second (2 matches); three match lines total.
    await expect
      .poll(() => fileHeaders(page))
      .toEqual(["main.ink (1)", "toppled-temple.ink (2)"]);
    await expect.poll(() => matchLines(page).then((m) => m.length)).toBe(3);
  });

  test("revealing a result navigates to the exact span", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    // Double-click the first match row → editor.reveal (buffer's reveal gesture,
    // replacing the tree row's single-click).
    await matchRowLocator(page, 0).dblclick();

    // main.ink is the focused view; the selection sits on its one "intro".
    await expect.poll(() => editorDoc(page)).toContain("-> intro");
    const doc = await editorDoc(page);
    expect(await selectionHead(page)).toBe(doc.indexOf("intro"));
  });

  test("Enter from a focused match row reveals it (keyboard-reachable)", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "intro");

    // Focus the first match row and press Enter — the buffer's keyboard reveal
    // (replacing the tree's ArrowDown-select + Enter from the query input).
    await matchRowLocator(page, 0).click();
    await page.keyboard.press("Enter");

    await expect.poll(() => editorDoc(page)).toContain("-> intro");
    const doc = await editorDoc(page);
    expect(await selectionHead(page)).toBe(doc.indexOf("intro"));
  });

  test("case and regex toggles change result counts; invalid regex shows an inline error", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "the");
    const insensitive = (await matchLines(page)).length;

    // Case-sensitive narrows the count (capitalized "The" drops out).
    await page.locator('.search-option[data-option="caseSensitive"]').click();
    await expect
      .poll(() => matchLines(page).then((m) => m.length))
      .toBeLessThan(insensitive);
    const sensitive = (await matchLines(page)).length;
    expect(sensitive).toBeGreaterThan(0);
    await page.locator('.search-option[data-option="caseSensitive"]').click();

    // Literal "knot|gold" matches nothing; as a regex the alternation hits.
    await page.locator(".search-input").fill("knot|gold");
    await expect(page.locator(".search-empty")).toBeVisible();
    await page.locator('.search-option[data-option="regex"]').click();
    await expect(page.locator(".search-results-buffer .cm-content")).toBeVisible();
    await expect.poll(() => matchLines(page).then((m) => m.length)).toBeGreaterThan(0);

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
    // …and both rewritten files match the replacement text (two file headers).
    await search(page, "prologue");
    await expect.poll(() => fileHeaders(page).then((h) => h.length)).toBe(2);
  });
});

test.describe("search replace (screenplay fixture)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("editing a match row rewrites the source (per-match replace)", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "figure");

    // The buffer has no per-row replace button anymore: a match is replaced by
    // editing its row inline. The committed edit routes back through
    // applySearchRowEdit → ProjectSession.applyEdit — the same source-edit seam
    // the old per-row Replace button used. (Double-click is bound to reveal, so
    // we drive a caret edit instead of a word double-click.)
    const row = matchRowLocator(page, 0);
    await expect(row).toContainText("A figure steps into the light.");

    // Baseline: the "figure" source line, unedited.
    expect(await editorDoc(page)).toContain("A figure steps into the light.");

    // Single-click to focus the buffer + place the caret inside the match line's
    // editable source region (past the read-only `N: ` prefix), then type a
    // marker. The exact caret column within the line isn't important — the point
    // is that a keystroke on a match row rewrites *that source line* in place
    // (through applySearchRowEdit → ProjectSession.applyEdit) without touching
    // any other line.
    await row.click();
    await page.keyboard.type("[EDITED]");

    // Commit is debounced; clicking the query input blurs the buffer and flushes
    // the pending write, then the mounted CM6 view of main.ink refreshes from the
    // rewritten source. Assert via __brinkView (the full focused-editor document —
    // the visible DOM only renders the viewport, and there are now two
    // .cm-content: editor + buffer).
    await page.locator(".search-input").click();
    await expect.poll(() => editorDoc(page)).toContain("[EDITED]");

    // The marker landed *inside the single "figure" source line* (the buffer
    // rejects newline insertions, so exactly one source line changed): the edited
    // line still ends with "into the light." and every other line is untouched.
    const doc = await editorDoc(page);
    const edited = doc.split("\n").filter((l) => l.includes("[EDITED]"));
    expect(edited).toHaveLength(1);
    expect(edited[0]).toContain("into the light.");
    expect(doc).toContain("The lights dim."); // sibling line untouched
    expect(doc).toContain("-> interrogation.evidence"); // sibling line untouched
  });
});
