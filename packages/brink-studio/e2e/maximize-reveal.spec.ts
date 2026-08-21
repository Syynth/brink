/**
 * Maximize-reveal e2e (issue #2825, spec §5.4's general invariant).
 *
 * Pixel-level proof that a group hidden behind a maximized sibling actually
 * repaints in the DOM once a state change resolves focus there — the gap
 * #2787's and #2797's (PR #2817) tests never closed, and #2826's own
 * consumer-level addition (`editor-area-maximize-paint.test.tsx`) only
 * closed at the jsdom/store level, not in a real browser painting real CSS.
 *
 * Reproduces the exact repro chain #2825/#2826 named as "hole (1)":
 * `editor.focusNextGroup`'s `when` guard only checks group count, so it can
 * desync `focusedGroupId` onto a group `EditorArea` isn't rendering while a
 * sibling is maximized. That desync is NOT fixed here — whether the fix
 * should move-and-reveal (matching `openDocument`/`moveTabToGroup`) or
 * disable the command while maximized is an open maintainer UX call
 * (docs/studio-shell-spec.md §5.4, "Not yet covered: `editor.focusNextGroup`").
 * This spec instead pins what IS already fixed and reachable today.
 *
 * This spec's own setup deliberately RIDES hole (1)'s unfixed bug — the
 * `Editor: Focus Next Group` step below only produces the desynced,
 * nothing-painted state this spec needs because that `when` guard is not
 * yet fixed. Whichever way the pending ruling above goes (clear
 * `maximizedGroupId` on move, or make `when: false` while maximized), this
 * setup step stops producing that state and this spec will go red. Its
 * setup will need to be rewritten to reach the same "hidden, desynced
 * group" precondition some other way (or the precondition itself may no
 * longer be reachable, in which case this spec's premise should be
 * revisited).
 *
 * The trigger has to be Quick Open, not a Binder click: maximizing a group
 * collapses every open dock (§5.4 — `regions.tsx`'s `showLeftDock` etc. all
 * gate on `!groupMaximized`), so the Binder is not actually reachable while
 * a group is maximized. Quick Open (`QuickOpen.tsx`) is mounted as an
 * App-level overlay alongside the command palette, outside the dock/strip
 * tree, so it stays reachable — and its pick still runs through the exact
 * same `editor.reveal` → `openTarget` → `openDocument` path a Binder click
 * would use (`mount.tsx`'s `setDocumentOpener`/`revealSource`).
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

function group(page: Page, index: number): Locator {
  return page.locator(".shell-editor-group").nth(index);
}

function tabsIn(g: Locator): Locator {
  return g.locator(".brink-tab .brink-tab-label");
}

function fileRow(page: Page, name: string): Locator {
  return page.locator(".brink-binder-file-row", { hasText: name });
}

/** Run a palette command by title (real input: Mod-Shift-P, type, Enter). */
async function runPaletteCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(title);
  await page.keyboard.press("Enter");
}

/**
 * Quick Open (real input: Mod-E — the alternate binding; Mod-P is
 * browser-interceptable per QuickOpen.tsx's own registration comment) a
 * file by exact name and pick the top (only unambiguous) match.
 *
 * Quick Open's items come from the async compile outline
 * (`buildQuickOpenItems(useStudioStore((s) => s.outline))`), so a query
 * typed before that outline is ready — or that never matches anything —
 * would otherwise make the Enter press below a silent no-op: `pick` never
 * fires, and the only symptom is a timeout on a much later assertion. Wait
 * for the ranked match to actually appear (and be the one selected) before
 * committing to it.
 */
async function quickOpenFile(page: Page, fileName: string): Promise<void> {
  await page.keyboard.press("ControlOrMeta+E");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill(fileName);
  await expect(page.locator(".shell-palette-item.selected .title")).toHaveText(fileName);
  await page.keyboard.press("Enter");
}

test.describe("maximize reveal (#2825)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=nested");
    await page.waitForSelector(".brink-binder-file-row", { timeout: 10000 });

    // Same single-group baseline as groups.spec.ts: drop the default
    // right-split Player tab so group mechanics start from one group
    // (main.ink, the project's entry file, opened pinned at bootstrap).
    const playerTab = page.locator(".brink-tab", { hasText: "Player" });
    await playerTab.hover();
    await playerTab.locator(".brink-tab-close").click();
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink"]);
  });

  test("Quick Open on a hidden, focus-desynced group repaints it in the DOM", async ({
    page,
  }) => {
    // Two groups: main.ink stays in group 0, util.ink opens then moves into
    // a new group 1 (group 1 ends up focused). helper.ink is never touched —
    // it is the "not open anywhere" target for the new-tab branch below.
    await fileRow(page, "util.ink").locator(".brink-binder-label").dblclick();
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink", "util.ink"]);
    await page.locator(".brink-tab .brink-tab-label", { hasText: /^util\.ink$/ }).click();
    await runPaletteCommand(page, "Editor: Move Tab to Right Group");
    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    await expect(tabsIn(group(page, 0))).toHaveText(["main.ink"]);
    await expect(tabsIn(group(page, 1))).toHaveText(["util.ink"]);
    await expect(group(page, 1)).toHaveAttribute("data-focused", "true");

    // Maximize the focused group (util.ink): its sibling leaves the DOM
    // entirely (§5.4 — EditorArea renders only the maximized group).
    await runPaletteCommand(page, "Editor: Toggle Maximized Group");
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    const maximizedGroupId = await page
      .locator(".shell-editor-group")
      .getAttribute("data-editor-group");
    expect(maximizedGroupId).not.toBeNull();

    // `editor.focusNextGroup` (#2825 hole 1, deliberately left unfixed
    // pending a maintainer ruling): its `when` guard only checks group
    // count, so it moves focus onto the now-hidden main.ink group. Nothing
    // repaints yet — the maximized group is still the only thing rendered,
    // exactly the desync the invariant exists to catch.
    await runPaletteCommand(page, "Editor: Focus Next Group");
    await expect(page.locator(".shell-editor-group")).toHaveCount(1);
    await expect(page.locator(".shell-editor-group")).toHaveAttribute(
      "data-editor-group",
      maximizedGroupId!,
    );
    // The desync itself, DOM-observable: `EditorGroupView` recomputes
    // `focused={maximizedGroup.id === focusedGroupId}` as false once focus
    // has moved off the maximized group, so the still-rendered (maximized)
    // group drops its `data-focused` attribute. Without this assertion the
    // count/id checks above stay true even if `focusNextGroup` never ran.
    await expect(page.locator(".shell-editor-group")).not.toHaveAttribute(
      "data-focused",
      "true",
    );

    // Quick Open a file that has never been opened anywhere: the picked
    // item runs the same `editor.reveal` → `openTarget` → `openDocument`
    // path a Binder click would (mount.tsx), so `openDocument`'s "focused"
    // target finds no existing tab and falls through to the new-tab branch,
    // targeting the (hidden, desynced) focused group. Its hoisted
    // final-return clear (#2826) reveals it — the pixel-level proof this
    // invariant family never had before.
    await quickOpenFile(page, "helper.ink");

    await expect(page.locator(".shell-editor-group")).toHaveCount(2);
    const revealed = page.locator(
      `.shell-editor-group:not([data-editor-group="${maximizedGroupId}"])`,
    );
    await expect(revealed).toHaveAttribute("data-focused", "true");
    await expect(tabsIn(revealed)).toHaveText(["main.ink", "helper.ink"]);
    await expect(revealed.locator(".cm-content")).toContainText("Done.");
  });
});
