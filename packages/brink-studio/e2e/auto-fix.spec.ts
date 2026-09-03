/**
 * Auto-fix e2e (`docs/autofix-spec.md` §7), one test per surface.
 *
 * Every test drives `?fixture=fixable` — the deterministic project in
 * `src/main.tsx`, whose diagnostic set is CLOSED and pinned in Rust by
 * `fixable_fixture` (`crates/brink-web/src/editor/fix_batch.rs`). Seven
 * visible rows; five of them Safe-fixable; one (`E025`) Suggested; one
 * (`E033`) with no fixer at all; and `E110` — a Safe-fixable code with a
 * live diagnostic — turned `"allow"` in the fixture's own `brink.toml`.
 *
 * That last one is issue #3459: the batch used to read `ProjectDb`'s RAW
 * diagnostic list, so "Fix all safe" counted the invisible `E110` and read
 * **6** against a panel showing five Fix buttons. The header test below
 * asserts the count against the rows themselves, so the regression comes
 * back as a mismatch rather than as a number nobody checks.
 *
 * Every test asserts BOTH halves: the edit landed in the document, and the
 * diagnostic is gone from Problems afterwards — checked against the full
 * post-fix row list, since a fix that trades one diagnostic for another
 * would still clear its own.
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

/** The fixture's whole visible diagnostic set, by a distinctive substring. */
const ROWS = {
  e025: "unresolved cross-module reference",
  e014: "logic line has no effect",
  e095: "names the definition's own current name",
  e092: "restates the module default",
  e033: "unreachable code after divert",
  e031: "function call argument count mismatch",
  e176: "divert-with-args site",
} as const;

/** Fix titles, as `brink-ide`'s fixers word them (spec §10). */
const FIX_TITLE = {
  e014: "Remove effect-free `~` line",
  e092: "Remove the redundant visibility directive",
} as const;

/** How many rows the fixture shows, and how many of them are Safe-fixable. */
const TOTAL_ROWS = Object.keys(ROWS).length;
const SAFE_FIXABLE = 5;

async function gotoFixable(page: Page): Promise<void> {
  await page.goto("/?fixture=fixable");
  await page.waitForSelector(".cm-content", { timeout: 15_000 });
  await openProblems(page);
  // The startup compile is debounced; wait for the closed set to land
  // rather than for a fixed delay.
  await expect(page.locator(".problems-item")).toHaveCount(TOTAL_ROWS, { timeout: 20_000 });
}

async function openProblems(page: Page): Promise<void> {
  const panel = page.locator('[data-toolwindow="problems"]');
  if ((await panel.count()) === 0) {
    await page
      .locator('.shell-strip-bottom .shell-strip-btn[aria-label="Problems"]')
      .click();
  }
  await expect(panel).toBeVisible();
}

function row(page: Page, text: string): Locator {
  return page.locator(".problems-item", { hasText: text });
}

/** Every visible row's message — the whole list, for before/after diffs. */
async function messages(page: Page): Promise<string[]> {
  return page.locator(".problems-item .problems-message").allTextContents();
}

/** The project's current text for one file, through the studio's own seam. */
async function fileSource(page: Page, path: string): Promise<string> {
  return page.evaluate((p) => {
    type AnyStore = { getState(): Record<string, unknown> };
    const stores = (window as unknown as { __brinkStores?: AnyStore[] }).__brinkStores;
    const project = stores?.[0]?.getState()._project as
      | { getFiles?: () => Record<string, string> }
      | null
      | undefined;
    return project?.getFiles?.()[p] ?? "";
  }, path);
}

/** The "Fix all safe (N)" header button. Absent when N is 0. */
function fixAllButton(page: Page): Locator {
  return page.locator(".problems-fix-all");
}

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

async function openSettings(page: Page, scope: "Project" | "App", title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+,");
  await expect(page.locator(".brink-settings-modal")).toBeVisible();
  await page.locator(".brink-settings-scope", { hasText: scope }).click();
  await page.locator(".brink-settings-nav-item", { hasText: title }).click();
  await expect(page.locator(".brink-settings-head h2")).toHaveText(title);
}

async function closeSettings(page: Page): Promise<void> {
  await page.keyboard.press("Escape");
  await expect(page.locator(".brink-settings-modal")).toHaveCount(0);
}

test.describe("auto-fix surfaces", () => {
  test("the fixture shows the closed diagnostic set the counts rest on", async ({
    page,
  }) => {
    await gotoFixable(page);
    // Non-vacuity: every row the other tests name is actually here, and the
    // `[lints]`-allowed E110 is NOT — its Safe fixer has a live diagnostic
    // and is excluded purely by the severity intersection (#3459).
    for (const text of Object.values(ROWS)) {
      await expect(row(page, text)).toHaveCount(1);
    }
    const all = (await messages(page)).join("\n");
    expect(all).not.toContain("#@effects(…)` is deprecated");
  });

  test("a Problems row's Fix button applies the fix and clears the row", async ({
    page,
  }) => {
    await gotoFixable(page);
    const before = await messages(page);
    const target = row(page, ROWS.e014);
    const button = target.locator(".problems-fix");
    await expect(button).toHaveAttribute("data-tier", "safe");
    await button.click();

    // The edit landed in the document…
    await expect
      .poll(() => fileSource(page, "prologue.ink"), { timeout: 10_000 })
      .not.toContain("\n~\n~ gold");
    // …and the row is gone, with nothing taking its place.
    await expect(page.locator(".problems-item")).toHaveCount(TOTAL_ROWS - 1);
    expect(await messages(page)).toEqual(
      before.filter((m) => !m.includes(ROWS.e014)),
    );
  });

  test("'Fix all safe (N)' counts the visible Safe rows, then clears them", async ({
    page,
  }) => {
    await gotoFixable(page);
    // N is the number of Safe Fix buttons ON SCREEN — the assertion #3459 is
    // about. Before the fix this read 6 against 5 buttons, because the
    // `[lints]`-allowed E110 was counted off the raw diagnostic list.
    const safeButtons = page.locator('.problems-item .problems-fix[data-tier="safe"]');
    await expect(safeButtons).toHaveCount(SAFE_FIXABLE);
    await expect(fixAllButton(page)).toHaveText(`Fix all safe (${SAFE_FIXABLE})`);

    await fixAllButton(page).click();

    // Exactly the two unfixable rows remain: the Suggested import (never
    // batched at the Safe tier) and the warning with no fixer.
    await expect(page.locator(".problems-item")).toHaveCount(2, { timeout: 10_000 });
    const left = (await messages(page)).join("\n");
    expect(left).toContain(ROWS.e025);
    expect(left).toContain(ROWS.e033);
    // Nothing safe left to do, so the header button withdraws itself.
    await expect(fixAllButton(page)).toHaveCount(0);
    // …and the `allow`ed E110 line was NOT rewritten along the way.
    expect(await fileSource(page, "prologue.ink")).toContain(
      "#@effects(reads: gold, writes: gold)",
    );
  });

  test("the Problems row context menu applies the same fix", async ({ page }) => {
    await gotoFixable(page);
    const before = await messages(page);
    await row(page, ROWS.e014).locator(".problems-row").click({ button: "right" });

    const menu = page.locator(".brink-context-menu");
    await expect(menu).toBeVisible();
    await menu
      .locator(".brink-context-menu-item", { hasText: FIX_TITLE.e014 })
      .click();

    await expect
      .poll(() => fileSource(page, "prologue.ink"), { timeout: 10_000 })
      .not.toContain("\n~\n~ gold");
    await expect(page.locator(".problems-item")).toHaveCount(TOTAL_ROWS - 1);
    expect(await messages(page)).toEqual(
      before.filter((m) => !m.includes(ROWS.e014)),
    );
  });

  test("the editor context menu offers the fix at the diagnostic", async ({ page }) => {
    await gotoFixable(page);
    // main.ink is the entry and is open; its E092 sits on `gold` in
    // `VAR gold = 12`. Put the caret there and right-click at the caret's own
    // coordinates — the menu's fix group is computed for the click position,
    // so guessing a pixel inside the word is not good enough.
    const point = await page.evaluate(() => {
      type View = {
        state: { doc: { toString(): string } };
        dispatch(t: unknown): void;
        coordsAtPos(pos: number): { left: number; right: number; top: number; bottom: number } | null;
      };
      const view = (window as unknown as { __brinkView?: View }).__brinkView;
      if (!view) return null;
      const at = view.state.doc.toString().indexOf("VAR gold");
      if (at < 0) return null;
      const pos = at + "VAR ".length;
      view.dispatch({ selection: { anchor: pos } });
      const c = view.coordsAtPos(pos);
      return c ? { x: (c.left + c.right) / 2, y: (c.top + c.bottom) / 2 } : null;
    });
    expect(point, "the caret must land on `gold` in main.ink").not.toBeNull();
    if (point === null) return;
    await page.mouse.click(point.x, point.y, { button: "right" });

    const menu = page.locator(".brink-context-menu.brink-text-menu");
    await expect(menu).toBeVisible();
    await menu
      .locator(".brink-context-menu-item", { hasText: FIX_TITLE.e092 })
      .click();

    await expect
      .poll(() => fileSource(page, "main.ink"), { timeout: 10_000 })
      .not.toContain("#@public");
    await expect(row(page, ROWS.e092)).toHaveCount(0);
    await expect(page.locator(".problems-item")).toHaveCount(TOTAL_ROWS - 1);
  });

  test("the command palette runs the project-wide batch", async ({ page }) => {
    await gotoFixable(page);
    await runPaletteCommand(page, "Fix: Fix all safe in project");

    await expect(page.locator(".problems-item")).toHaveCount(2, { timeout: 10_000 });
    const left = (await messages(page)).join("\n");
    expect(left).toContain(ROWS.e025);
    expect(left).toContain(ROWS.e033);
    // Every Safe fix landed, across all three files it touched.
    expect(await fileSource(page, "main.ink")).not.toContain("#@public");
    expect(await fileSource(page, "prologue.ink")).not.toContain("#@was(prologue)");
    expect(await fileSource(page, "market.ink")).toContain('greet("stranger")');
  });

  test("fix on save: off does nothing, 'safe' applies on save, and the setting survives a reload", async ({
    page,
  }) => {
    await gotoFixable(page);

    // Default is off (§6.2) — a save must not rewrite the manuscript.
    await page.locator(".cm-content").first().click();
    await page.keyboard.press("ControlOrMeta+s");
    await expect(page.locator(".shell-notification")).toContainText("Saved main.ink");
    expect(await fileSource(page, "main.ink")).toContain("#@public");
    await expect(page.locator(".problems-item")).toHaveCount(TOTAL_ROWS);

    // Turn it on THROUGH the Settings document, not through the store.
    await openSettings(page, "App", "Editor");
    const select = page
      .locator(".settings-row", { hasText: "Fix on save" })
      .locator("select");
    await expect(select).toHaveValue("off");
    await select.selectOption("safe");
    await closeSettings(page);

    await page.locator(".cm-content").first().click();
    await page.keyboard.press("ControlOrMeta+s");
    await expect
      .poll(() => fileSource(page, "main.ink"), { timeout: 10_000 })
      .not.toContain("#@public");
    await expect(row(page, ROWS.e092)).toHaveCount(0);
    // Per-file (§7): only the saved file was rewritten.
    expect(await fileSource(page, "prologue.ink")).toContain("#@was(prologue)");

    // The setting is app scope and persists.
    await page.reload();
    await page.waitForSelector(".cm-content", { timeout: 15_000 });
    await openSettings(page, "App", "Editor");
    await expect(
      page.locator(".settings-row", { hasText: "Fix on save" }).locator("select"),
    ).toHaveValue("safe");
  });

  test("Settings ▸ Diagnostics: the Fix column withdraws a row's fix and restores it", async ({
    page,
  }) => {
    await gotoFixable(page);
    await expect(fixAllButton(page)).toHaveText(`Fix all safe (${SAFE_FIXABLE})`);

    await openSettings(page, "Project", "Diagnostics");
    await page.locator(".lint-search").fill("E014");
    const lintRow = page.locator(".lint-row", { has: page.locator(".lint-code", { hasText: "E014" }) });
    await expect(lintRow).toHaveCount(1);
    await lintRow.locator(".lint-fix .fix-level", { hasText: "off" }).click();
    await closeSettings(page);

    // `[fix] E014 = "off"` withdraws the code from every surface (§6.1).
    await expect(fixAllButton(page)).toHaveText(`Fix all safe (${SAFE_FIXABLE - 1})`, {
      timeout: 10_000,
    });
    await expect(row(page, ROWS.e014)).toHaveCount(1);
    await expect(row(page, ROWS.e014).locator(".problems-fix")).toHaveCount(0);

    // …and `auto` puts it back.
    await openSettings(page, "Project", "Diagnostics");
    await page.locator(".lint-search").fill("E014");
    const again = page.locator(".lint-row", { has: page.locator(".lint-code", { hasText: "E014" }) });
    await again.locator(".lint-fix .fix-level", { hasText: "auto" }).click();
    await closeSettings(page);

    await expect(fixAllButton(page)).toHaveText(`Fix all safe (${SAFE_FIXABLE})`, {
      timeout: 10_000,
    });
    await expect(row(page, ROWS.e014).locator(".problems-fix")).toHaveCount(1);
  });
});
