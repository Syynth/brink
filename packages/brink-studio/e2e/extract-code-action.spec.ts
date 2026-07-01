import { test, expect, type Page } from "@playwright/test";

/**
 * Extract selection → knot code action (#315 H) — the studio-enabled path
 * (#321's deferred code-actions menu). Select 2+ lines in a knot body, open the
 * code-actions menu (Ctrl-. / Cmd-.), choose "Extract to knot", type a name,
 * Enter → a new `=== name ===` knot with the moved content appears and the
 * original selection becomes the tunnel call `-> name ->`.
 *
 * Runs against the deterministic single-file `?fixture=screenplay` project,
 * whose `=== opening ===` knot has two body lines to lift.
 */

const IS_MAC = process.platform === "darwin";

/** The focused CM6 view exposed by DocumentSessions for e2e (`__brinkView`). */
async function selectBodyLines(page: Page): Promise<void> {
  await page.evaluate(() => {
    const view = (window as unknown as { __brinkView?: unknown }).__brinkView as
      | {
          state: { doc: { toString(): string } };
          dispatch(spec: unknown): void;
        }
      | undefined;
    if (!view) throw new Error("no focused editor view");
    const doc = view.state.doc.toString();
    // Select the two body lines of `=== opening ===`.
    const from = doc.indexOf("The lights dim.");
    const to = doc.indexOf("-> interrogation");
    view.dispatch({ selection: { anchor: from, head: to } });
  });
}

async function editorText(page: Page): Promise<string> {
  return page.evaluate(() => {
    const view = (window as unknown as { __brinkView?: { state: { doc: { toString(): string } } } })
      .__brinkView;
    return view ? view.state.doc.toString() : "";
  });
}

test.describe("extract to knot code action (#315 H)", () => {
  test("select 2 lines → Ctrl-. → Extract to knot → name → new knot + tunnel call", async ({
    page,
  }) => {
    await page.goto("/?fixture=screenplay");
    await page.waitForSelector(".brink-knot-header", { timeout: 8000 });

    // Focus the editor and select the two body lines of `=== opening ===`.
    await page.locator(".cm-content").click();
    await selectBodyLines(page);

    // Open the code-actions menu.
    await page.keyboard.press(IS_MAC ? "Meta+." : "Control+.");
    const menu = page.locator(".brink-code-actions-menu");
    await expect(menu).toBeVisible();

    // Choose "Extract to knot".
    const item = menu.locator(".brink-code-action-item", { hasText: "Extract to knot" });
    await expect(item).toBeVisible();
    await item.click();

    // The inline name prompt mounts; type a name and confirm.
    const input = page.locator(".brink-inline-rename-input");
    await expect(input).toBeVisible();
    await input.fill("prologue");
    await page.keyboard.press("Enter");

    // A safe extract applies immediately: the new knot header + tunnel call land.
    await expect
      .poll(async () => await editorText(page), { timeout: 8000 })
      .toContain("=== prologue ===");

    const text = await editorText(page);
    expect(text).toContain("-> prologue ->"); // selection replaced by the tunnel call
    expect(text).toContain("The lights dim."); // content preserved (now in the new knot)
    // The moved content lives under the new knot, not before its old call site.
    const knotIdx = text.indexOf("=== prologue ===");
    expect(text.indexOf("The lights dim.")).toBeGreaterThan(knotIdx);

    // The binder outline picks up the new knot.
    await expect(
      page.locator(".brink-binder-knot .brink-binder-label", { hasText: /^prologue$/ }),
    ).toHaveCount(1);

    // A toast confirms the apply (with Undo).
    await expect(page.locator(".shell-notification", { hasText: /Extract to knot/ })).toBeVisible();
  });
});
