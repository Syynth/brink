import { test, expect, type Page } from "@playwright/test";
import { ensureStoryStarted } from "./session-helpers.js";
import { enterStructureMode } from "./binder-mode";

/**
 * "Play from here" (#186) — start a fresh play session entered at a knot/stitch
 * from the binder context menu and the editor gutter ▶. Opening a session adds
 * a row to the Debugger panel's Flows list labeled with the ink path
 * (W8/#3301 — the status-bar SessionPicker retired).
 */

/** Open the Debugger panel (idempotent enough for these flows: the strip
 *  button toggles, so only click when the panel isn't already shown). */
async function openDebuggerPanel(page: Page): Promise<void> {
  if ((await page.locator(".debugger-panel").count()) === 0) {
    await page.locator('.shell-strip-btn[aria-label="Debugger"]').click();
  }
  await expect(page.locator(".debugger-panel")).toBeVisible();
}

async function sessionOptionCount(page: Page): Promise<number> {
  await openDebuggerPanel(page);
  return page.locator(".dp-flow-row").count();
}

/**
 * Wait until the project has actually COMPILED, not merely rendered.
 *
 * `.cm-content` says the editor mounted. It does not say a compile has
 * landed — and "Play from here" needs a compiled program to enter, so a
 * click inside that window no-ops and the session never appears (issue
 * #3163; the same "the editor is up, therefore the app is ready" mistake
 * as #3158).
 *
 * The entry badge is the signal because it is written by the compile
 * fan-out itself (`landCompileResult` -> `setEntryFile` -> the Binder's
 * entry ICON, the brink mark that replaced the text badge), and every
 * project has exactly one entry — so this is not
 * coupled to what the demo fixture happens to contain, the way waiting on
 * a warning count would be.
 */
async function waitForFirstCompile(page: Page): Promise<void> {
  await expect(page.locator(".brink-file-icon-entry").first()).toBeVisible({
    timeout: 15000,
  });
}

async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

test.describe("play from here (#186)", () => {
  test("binder context menu opens a session at the knot", async ({ page }) => {
    await page.goto("/");
    await enterStructureMode(page);
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    const knot = page.locator(".brink-binder-knot").first();
    const knotName = ((await knot.locator(".brink-binder-label").textContent()) ?? "").trim();
    expect(knotName).not.toBe("");

    await knot.click({ button: "right" });
    const item = page.locator(".brink-context-menu-item", { hasText: "Play from here" });
    await expect(item).toBeVisible();
    await item.click();

    // A session entered at the knot is registered, labeled with its ink path.
    await openDebuggerPanel(page);
    await expect(
      page.locator(".dp-flow-select", { hasText: knotName }),
    ).toHaveCount(1);
  });

  test("editor gutter ▶ on a knot header opens a session", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content");
    await waitForFirstCompile(page);

    const before = await sessionOptionCount(page);

    // Hover a knot header line to reveal its gutter run-icon, then click it.
    // (A hidden measurement spacer also carries the class, so target `:visible`.)
    // Re-hover under a poll: a single synthetic hover can land before the
    // header classification / hover machinery is ready (flaked on CI), and
    // nothing re-fires it — the poll is the honest "user wiggles the mouse".
    const header = page.locator(".cm-line").filter({ hasText: /^===/ }).first();
    const icon = page.locator(".brink-play-gutter-icon:visible").first();
    await expect
      .poll(
        async () => {
          await header.hover();
          return icon.isVisible();
        },
        { timeout: 15_000 },
      )
      .toBe(true);
    await icon.click();

    await expect
      .poll(async () => sessionOptionCount(page))
      .toBeGreaterThan(before);
  });

  test("story graph node right-click opens a session at the knot", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
    // Since W7 (no auto-start): the status-bar picker only appears with
    // MORE than one session, so the main session must be running before
    // play-from-here opens the second.
    await ensureStoryStarted(page);

    await runPaletteCommand(page, "Story: Open Story Graph");
    await expect(page.locator(".brink-story-graph")).toBeVisible({ timeout: 10000 });
    const node = page.locator('[data-graph-node="intro"]');
    await expect(node).toBeAttached();

    await node.click({ button: "right" });
    const item = page.locator(".brink-context-menu-item", { hasText: "Play from here" });
    await expect(item).toBeVisible();
    await item.click();

    // W8/#3301: the status-bar SessionPicker retired — the open-flows
    // list lives in the Debugger panel now.
    await page.locator('.shell-strip-btn[aria-label="Debugger"]').click();
    await expect(page.locator(".dp-flow-row")).toHaveCount(2);
    await expect(page.locator(".dp-flow-select", { hasText: "intro" })).toHaveCount(1);
  });

  test("editor right-click on a knot shows the shared refactor menu", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content");

    // Right-click a knot header → the shared symbol menu (play + refactors).
    const header = page.locator(".cm-line").filter({ hasText: /^===/ }).first();
    await header.click({ button: "right" });

    await expect(
      page.locator(".brink-context-menu-item", { hasText: "Play from here" }),
    ).toBeVisible();
    // The structural refactors are present too (a knot offers Move Up/Down).
    await expect(
      page.locator(".brink-context-menu-item", { hasText: /Move/ }).first(),
    ).toBeVisible();
  });
});
