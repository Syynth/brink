/**
 * Story Graph document e2e (issue #97, spec §4.1).
 *
 * Real-input flows over the custom-rendered graph document: open via the
 * palette (knot nodes render, stitches collapsed, END/DONE pseudo-nodes
 * present, legend visible), click a node to reveal its source in the editor
 * (§6.1), expand a knot to show its stitches, the live session overlay
 * (current-location highlight + visit badges; stopping the story strips the
 * overlay, starting brings it back — §7.6), and a recompile refreshing the
 * graph with a new knot.
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

function graphNode(page: Page, id: string): Locator {
  return page.locator(`[data-graph-node="${id}"]`);
}

async function openStoryGraph(page: Page): Promise<void> {
  await runPaletteCommand(page, "Story: Open Story Graph");
  // The graph TAKES OVER the editor root area rather than opening as a tab
  // (decision log 2026-08-26) — so its name is on the takeover header, and
  // there is no tab label to look for.
  await expect(
    page.locator(".shell-takeover-title", { hasText: "Story Graph" }),
  ).toBeVisible();
  // The canvas renders once the startup compile delivers a graph.
  await expect(page.locator(".brink-story-graph")).toBeVisible({ timeout: 10000 });
  await expect(graphNode(page, "intro")).toBeAttached();
}

test.describe("story graph document", () => {
  // The default (toppled-temple) project: its startup compile succeeds, so
  // the graph data lands, and the session auto-starts (§7.6), so the live
  // overlay has data from the first paint.
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("opens via the palette: collapsed knots, pseudo-nodes, legend, read-only", async ({
    page,
  }) => {
    await openStoryGraph(page);

    // Knot nodes from the demo story.
    for (const id of ["intro", "barter", "threshold", "warden_golem"]) {
      await expect(graphNode(page, id)).toBeAttached();
      await expect(graphNode(page, id)).toHaveAttribute("data-kind", "knot");
    }

    // Knots are collapsed by default — warden_golem's stitches are absent.
    await expect(graphNode(page, "warden_golem")).toHaveAttribute(
      "data-expanded",
      "false",
    );
    await expect(graphNode(page, "warden_golem.specials")).toHaveCount(0);

    // END/DONE pseudo-nodes are present and distinct.
    await expect(graphNode(page, "END")).toHaveAttribute("data-kind", "end");

    // Corner legend names the edge kinds.
    await expect(page.locator(".brink-graph-legend")).toBeVisible();
    await expect(page.locator(".brink-graph-legend")).toContainText("divert");
    await expect(page.locator(".brink-graph-legend")).toContainText("choice");

    // Reopening leaves the area with its one occupant, still the graph.
    await runPaletteCommand(page, "Story: Open Story Graph");
    await expect(
      page.locator(".shell-takeover-title", { hasText: "Story Graph" }),
    ).toHaveCount(1);
  });

  test("clicking a node reveals its source in the editor", async ({ page }) => {
    await openStoryGraph(page);

    await graphNode(page, "barter").click();

    // editor.reveal opens the declaring file (the knots live in the include)
    // and scrolls the knot header into the viewport. It also puts the graph
    // away: revealing source means "take me to the code", so the takeover
    // steps aside rather than covering what it just revealed.
    await expect(page.locator("[data-takeover]")).toHaveCount(0);
    await expect(
      page.locator(".brink-tab-label", { hasText: "toppled-temple.ink" }),
    ).toBeVisible();
    await expect(
      page.locator(".cm-content").filter({ hasText: "=== barter ===" }),
    ).toBeVisible();
  });

  test("expanding a knot reveals its stitches", async ({ page }) => {
    await openStoryGraph(page);

    await graphNode(page, "warden_golem")
      .locator(".brink-graph-node-toggle")
      .click();

    await expect(graphNode(page, "warden_golem")).toHaveAttribute(
      "data-expanded",
      "true",
    );
    await expect(graphNode(page, "warden_golem.specials")).toBeAttached();
    await expect(graphNode(page, "warden_golem.warden_turn")).toBeAttached();
    await expect(graphNode(page, "warden_golem.specials")).toHaveAttribute(
      "data-kind",
      "stitch",
    );

    // Collapse again — stitches fold back into the knot.
    await graphNode(page, "warden_golem")
      .locator(".brink-graph-node-toggle")
      .click();
    await expect(graphNode(page, "warden_golem.specials")).toHaveCount(0);
  });

  test("live overlay: highlight + visit badge while running, plain graph when stopped", async ({
    page,
  }) => {
    await openStoryGraph(page);

    // The startup compile auto-started the session at the intro choice point:
    // the current-location highlight is on `intro`. (No visit badges here —
    // the runtime only tracks counts for containers whose flags request
    // them, and the demo story never reads knot counts; badge mapping is
    // unit-tested.)
    await expect(graphNode(page, "intro")).toHaveAttribute("data-current", "true");

    // Stop the story: no session → plain graph, no overlay artifacts.
    await runPaletteCommand(page, "Story: Stop");
    await expect(page.locator("[data-current=\"true\"]")).toHaveCount(0);
    await expect(page.locator("[data-visits]")).toHaveCount(0);
    // The graph itself stays (compile-bound, not session-bound).
    await expect(graphNode(page, "intro")).toBeAttached();

    // Start again: the overlay returns.
    await runPaletteCommand(page, "Story: Start");
    await expect(graphNode(page, "intro")).toHaveAttribute("data-current", "true");
  });

  test("the graph reflects an edit once it recompiles", async ({ page }) => {
    // Typing, a debounced recompile, and then opening the graph do not fit
    // in the suite's 15s default — the old version overlapped the compile
    // with an already-mounted graph, which this one cannot.
    test.setTimeout(45_000);
    // This used to park the graph in a right-hand split and type in the other
    // group, so a MOUNTED graph could be watched following the recompile.
    // The takeover ruling (decision log 2026-08-26) makes that impossible on
    // purpose: the editor root area has one occupant, so the graph and the
    // editor are never on screen together. Editing first and opening the
    // graph after is the shape the design now allows — see the note in the
    // Wave 3 PR about what that costs.
    await page.locator(".cm-content").first().click();
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.press("Enter");
    await page.keyboard.type("=== xyzzy ===");
    await page.keyboard.press("Enter");
    await page.keyboard.type("A hollow voice says, plugh.");
    await page.keyboard.press("Enter");
    await page.keyboard.type("-> END");

    // Prove the edit landed before blaming the graph for not showing it.
    await expect(page.locator(".cm-content").first()).toContainText("xyzzy");

    // This settle is NOT ordinary flakiness padding — it waits for a
    // RECOMPILE (#3137). The graph renders the last compiled program, and the
    // compile is debounced 500ms in `diagnostics.ts`; `destroy()` cancels a
    // pending one, and opening the graph unmounts the editor (one occupant).
    // So opening it inside that window shows the pre-edit program.
    //
    // Note what this is NOT: the SOURCE is not lost. It reaches the session
    // synchronously on the transaction, so a save writes the right text —
    // measured, after an earlier version of this comment claimed otherwise.
    await page.waitForTimeout(2500);
    await openStoryGraph(page);
    await expect(graphNode(page, "xyzzy")).toBeAttached({ timeout: 15000 });
  });
});
