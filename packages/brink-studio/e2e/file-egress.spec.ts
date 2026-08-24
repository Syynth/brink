/**
 * File-content egress e2e (issues #154/#137): the playground mounts with a
 * recording onFilesChanged hook (`?egress=test`, see main.tsx), and every
 * mutation path delivers host change batches — debounced for editor typing,
 * immediate on Mod-S (file.save), and covering the bulk paths that used to
 * skip the seam: binder drag-reorder and search replace-all.
 */

import { test, expect, type Page } from "@playwright/test";
import { enterStructureMode } from "./binder-mode";
import type { FileChange } from "../src/index.js";

/** The recorded onFilesChanged batches (window hook from `?egress=test`). */
function batches(page: Page): Promise<FileChange[][]> {
  return page.evaluate(
    () => (window as unknown as { __brinkFileChanges?: FileChange[][] }).__brinkFileChanges ?? [],
  );
}

async function flatChanges(page: Page): Promise<FileChange[]> {
  return (await batches(page)).flat();
}

async function gotoStudio(page: Page, query: string): Promise<void> {
  await page.goto(query);
  await page.waitForSelector(".cm-content", { timeout: 10000 });
}

test.describe("file egress — editor typing", () => {
  test("an edit reaches the host as a debounced modified batch with content", async ({
    page,
  }) => {
    await gotoStudio(page, "/?egress=test&fixture=screenplay");

    // Mounting + the initial compile must not produce egress noise: the
    // mount files are the clean baseline.
    expect(await batches(page)).toEqual([]);

    await page.locator(".cm-content").first().click();
    await page.keyboard.type("// egress marker");

    // Editor flush debounce + egress debounce — delivered well within the
    // poll window, with the file named, typed as modified, content carried.
    await expect
      .poll(async () =>
        (await flatChanges(page)).filter(
          (c) => c.path === "main.ink" && c.type === "modified",
        ),
      )
      .toContainEqual(
        expect.objectContaining({ content: expect.stringContaining("// egress marker") }),
      );
  });
});

test.describe("file egress — file.save (Mod-S)", () => {
  test("saves immediately, bypassing the debounces, and raises a notification", async ({
    page,
  }) => {
    await gotoStudio(page, "/?egress=test&fixture=screenplay");

    await page.locator(".cm-content").first().click();
    await page.keyboard.type("// saved marker");
    await page.keyboard.press("ControlOrMeta+s");

    // The notification is raised synchronously by the command…
    await expect(
      page.locator(".shell-notification-message").filter({ hasText: "Saved main.ink" }),
    ).toBeVisible();

    // …so by now the host batch must already exist (no debounce wait):
    // file.save flushed the focused editor straight to the session and
    // delivered pending changes immediately.
    const changes = await flatChanges(page);
    const saved = changes.find((c) => c.path === "main.ink" && c.type === "modified");
    expect(saved?.content).toContain("// saved marker");
  });

  test("file.save works without a host hook (standalone playground)", async ({ page }) => {
    // No ?egress=test: the playground mounts hookless — the command must
    // still flush internally and notify, never error.
    await gotoStudio(page, "/?fixture=screenplay");

    await page.locator(".cm-content").first().click();
    await page.keyboard.type("x");
    await page.keyboard.press("ControlOrMeta+s");

    await expect(
      page.locator(".shell-notification-message").filter({ hasText: "Saved main.ink" }),
    ).toBeVisible();
  });
});

test.describe("file egress — binder structural ops (#137)", () => {
  test("drag-reordering knots delivers the rewritten file to the host", async ({ page }) => {
    await gotoStudio(page, "/?egress=test&fixture=screenplay");
    await enterStructureMode(page);
    await page.waitForSelector(".brink-binder-knot");

    // Drag "interrogation" onto the top 30% of "opening" → reorder before.
    const rows = page.locator(".brink-binder-knot");
    const opening = rows.filter({ hasText: "opening" }).first();
    const interrogation = rows.filter({ hasText: "interrogation" }).first();
    await interrogation.dragTo(opening, { targetPosition: { x: 40, y: 2 } });

    // The structural op rewrites main.ink in one shot — the host batch
    // carries the reordered content.
    await expect
      .poll(async () =>
        (await flatChanges(page)).filter(
          (c) => c.path === "main.ink" && c.type === "modified",
        ),
      )
      .not.toHaveLength(0);
    const changes = await flatChanges(page);
    const moved = changes.find((c) => c.path === "main.ink")!;
    expect(moved.content!.indexOf("=== interrogation ===")).toBeLessThan(
      moved.content!.indexOf("=== opening ==="),
    );
  });
});

test.describe("file egress — search replace-all (#137)", () => {
  test("replace-all delivers one batch naming both rewritten files", async ({ page }) => {
    // Default fixture: main.ink + toppled-temple.ink, "intro" appears in both.
    await gotoStudio(page, "/?egress=test");

    await page.keyboard.press("ControlOrMeta+Shift+F");
    await page.locator(".search-input").fill("intro");
    // Results rendered as cards (docs/search-results-cards-spec.md): a card
    // is the signal that the debounced search produced matches to replace.
    await expect(page.locator(".search-card").first()).toBeVisible();

    await page.locator(".search-replace-toggle").click();
    await page.locator(".search-replace-input").fill("prologue");
    await page.locator(".search-replace-all").click();
    await page.locator(".search-confirm-yes").click();

    // Both edits land inside one debounce window → one batch, both files.
    await expect.poll(() => batches(page)).not.toHaveLength(0);
    const all = await batches(page);
    const batch = all.find((b) => b.some((c) => c.path === "main.ink"))!;
    expect(batch.map((c) => c.path).sort()).toEqual(["main.ink", "toppled-temple.ink"]);
    for (const change of batch) {
      expect(change.type).toBe("modified");
      expect(change.content).toContain("prologue");
    }
  });
});
