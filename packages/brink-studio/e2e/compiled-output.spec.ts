/**
 * Compiled Output document e2e (issue #91, spec §4).
 *
 * Real-input flows over the read-only .inkt document: open via the palette
 * command, the dump text renders in a CM6 view, typing is inert, the CM6
 * search panel opens (Mod-F inside the view) and finds tokens, and the dump
 * live-updates after an edit + recompile in an ink view.
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("Meta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

function compiledOutputContent(page: Page): Locator {
  return page.locator(".brink-compiled-output .cm-content");
}

/**
 * The full document text via the e2e view hook — CM6 renders only the
 * viewport into the DOM, so textContent misses off-screen lines.
 */
function compiledOutputText(page: Page): Promise<string> {
  return page.evaluate(() => {
    const view = (window as unknown as Record<string, unknown>)
      .__brinkCompiledOutputView as { state: { doc: { toString(): string } } };
    return view.state.doc.toString();
  });
}

async function openCompiledOutput(page: Page): Promise<void> {
  await runPaletteCommand(page, "Program: Open Compiled Output");
  await expect(
    page.locator(".brink-tab-label", { hasText: "Compiled Output" }),
  ).toBeVisible();
  // The dump appears once the startup compile succeeds and the program loads.
  await expect(compiledOutputContent(page)).toContainText("(story", { timeout: 10000 });
}

test.describe("compiled output document", () => {
  // The default (toppled-temple) project: its startup compile succeeds, so
  // the program loads and programInkt is captured. (The screenplay fixture
  // used by other specs carries a deliberate unresolved-divert error and
  // never produces a compiled program.)
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("opens via the palette with the live compile's .inkt", async ({ page }) => {
    await openCompiledOutput(page);

    const text = await compiledOutputText(page);
    expect(text).toContain("(container");
    expect(text).toContain("(code");
    // Source text from the demo story lands in the lines tables.
    expect(text).toContain("the Toppled Temple");

    // Reopening focuses the existing tab — still exactly one.
    await runPaletteCommand(page, "Program: Open Compiled Output");
    await expect(
      page.locator(".brink-tab-label", { hasText: "Compiled Output" }),
    ).toHaveCount(1);
  });

  test("read-only: typing does nothing", async ({ page }) => {
    await openCompiledOutput(page);

    const content = compiledOutputContent(page);
    const before = await compiledOutputText(page);
    await content.click();
    await page.keyboard.type("INJECTED");
    await page.keyboard.press("Enter");
    await page.keyboard.press("Backspace");

    expect(await compiledOutputText(page)).toBe(before);
    await expect(content).toHaveAttribute("contenteditable", "false");
  });

  test("CM6 search panel opens with Mod-F and finds a token", async ({ page }) => {
    await openCompiledOutput(page);

    await compiledOutputContent(page).click();
    await page.keyboard.press("Meta+f");

    const panel = page.locator(".brink-compiled-output .cm-search");
    await expect(panel).toBeVisible();
    // Real keystrokes — the panel commits its query on keyup, not on
    // programmatic input. Find-next then scrolls the first match into the
    // viewport and selects it (CM6 only renders visible match decorations).
    await panel.locator("input[name=search]").pressSequentially("enter_container");
    await page.keyboard.press("Enter");
    await expect(
      page.locator(".brink-compiled-output .cm-searchMatch-selected").first(),
    ).toBeVisible();
  });

  test("dump live-updates after an edit + recompile in an ink view", async ({ page }) => {
    await openCompiledOutput(page);
    expect(await compiledOutputText(page)).not.toContain("XYZZY");

    // Put the dump in its own right group so it stays mounted while editing.
    await runPaletteCommand(page, "Editor: Move Tab to Right Group");
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);

    // Add a new knot to the ink source (debounced recompile).
    await page
      .locator(".shell-editor-group")
      .first()
      .locator(".cm-content")
      .click();
    await page.keyboard.press("Meta+End");
    await page.keyboard.press("Enter");
    await page.keyboard.type("=== xyzzy ===");
    await page.keyboard.press("Enter");
    await page.keyboard.type("The XYZZY rune glows.");
    await page.keyboard.press("Enter");
    await page.keyboard.type("-> END");

    // The mounted dump view follows the successful compile.
    await expect
      .poll(() => compiledOutputText(page), { timeout: 10000 })
      .toContain("XYZZY");
  });
});
