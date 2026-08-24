/**
 * Search tool window e2e (issue #94, spec §4; result cards
 * docs/search-results-cards-spec.md, PR C).
 *
 * The editable results buffer (#322 design D) was replaced by the per-match
 * card list: one card per match — a header row (`file:line`, containing
 * knot, edited badge, reveal ↗) over the match's own small editable CM6
 * buffer (match line + context window). Card headers and static bodies are
 * plain DOM, so assertions read the DOM; a card's editable buffer is
 * reached through the `__brinkSearchCardViews` hook (keyed by the stable
 * match id `path#ordinal`), since CM6 only renders the viewport.
 *
 * Real-input flows: Mod-Shift-F opens the window and focuses the query
 * input (and never closes it), Mod-6 is the generated toggle, live search
 * renders cards grouped in sorted-file order with a count summary,
 * revealing a card navigates to the exact span, case/regex toggles change
 * result counts, an invalid regex shows an inline error, editing inside a
 * card rewrites the source (write-through) while the frozen snapshot keeps
 * every card, replace-all runs behind an inline confirmation, collapse
 * works per card and via the summary's all-buttons, and cmd-clicking a
 * definition routes Find References into the panel (ruled 2026-08-24).
 */

import { test, expect, type Page } from "@playwright/test";

async function openSearch(page: Page): Promise<void> {
  await page.keyboard.press("ControlOrMeta+Shift+F");
  await expect(page.locator(".search-view")).toBeVisible();
}

/** Card headers' `file:line` labels, in list order. */
function cardLocs(page: Page): Promise<string[]> {
  return page.locator(".search-card .search-card-loc").allTextContents();
}

function cardCount(page: Page): Promise<number> {
  return page.locator(".search-card").count();
}

/** Type a query and wait for the debounced search to render cards. */
async function search(page: Page, query: string): Promise<void> {
  await page.locator(".search-input").fill(query);
  await expect(page.locator(".search-card").first()).toBeVisible();
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

/** A visible card's own CM6 document, via the e2e hook (`path#ordinal`). */
function cardDoc(page: Page, id: string): Promise<string> {
  return page.evaluate(
    (cardId) =>
      (window as any).__brinkSearchCardViews?.[cardId]?.state.doc.toString() ?? "",
    id,
  );
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

  test("cards render in sorted-file order with the count summary", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    // One card per match, sorted file order: main.ink's single "intro"
    // first, then the included story's two.
    await expect.poll(() => cardLocs(page).then((l) => l.length)).toBe(3);
    const locs = await cardLocs(page);
    expect(locs[0]).toMatch(/^main\.ink:\d+$/);
    expect(locs[1]).toMatch(/^toppled-temple\.ink:\d+$/);
    expect(locs[2]).toMatch(/^toppled-temple\.ink:\d+$/);
    await expect(page.locator(".search-summary-count")).toHaveText(
      "3 results · 2 files",
    );
  });

  test("revealing a card navigates to the exact span", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    await page.locator(".search-card-reveal").first().click();

    // main.ink is the focused view; the selection sits on its one "intro".
    await expect.poll(() => editorDoc(page)).toContain("-> intro");
    const doc = await editorDoc(page);
    expect(await selectionHead(page)).toBe(doc.indexOf("intro"));
  });

  test("reveal is keyboard-reachable (focused ↗ + Enter)", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    await page.locator(".search-card-reveal").first().focus();
    await page.keyboard.press("Enter");

    await expect.poll(() => editorDoc(page)).toContain("-> intro");
    const doc = await editorDoc(page);
    expect(await selectionHead(page)).toBe(doc.indexOf("intro"));
  });

  test("cards collapse to a header preview; the summary buttons hit all of them", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "intro");
    await expect.poll(() => cardCount(page)).toBe(3);

    // Per-card collapse: buffer gone, truncated preview in the header.
    const first = page.locator(".search-card").first();
    await first.locator(".search-card-chevron").click();
    await expect(first.locator(".search-card-editor")).toHaveCount(0);
    await expect(first.locator(".search-card-preview")).toBeVisible();

    // Collapse all (binder-style toolbar buttons), then expand all.
    await page.locator(".search-collapse-all").click();
    await expect(page.locator(".search-card-editor")).toHaveCount(0);
    await page.locator(".search-expand-all").click();
    await expect
      .poll(() => page.locator(".search-card-editor").count())
      .toBeGreaterThan(0);
  });

  test("the context knob retunes every card's window", async ({ page }) => {
    await openSearch(page);
    await search(page, "intro");

    // Default 1↑ 2↓ → the first card shows more than the match line.
    await expect
      .poll(() => cardDoc(page, "main.ink#0").then((d) => d.split("\n").length))
      .toBeGreaterThan(1);

    await page.locator(".search-context-toggle").click();
    await page.getByLabel("Context lines before").fill("0");
    await page.getByLabel("Context lines after").fill("0");

    await expect
      .poll(() => cardDoc(page, "main.ink#0").then((d) => d.split("\n").length))
      .toBe(1);
    await expect.poll(() => cardDoc(page, "main.ink#0")).toContain("intro");
  });

  test("case and regex toggles change result counts; invalid regex shows an inline error", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "the");
    const insensitive = await cardCount(page);

    // Case-sensitive narrows the count (capitalized "The" drops out).
    await page.locator('.search-option[data-option="caseSensitive"]').click();
    await expect.poll(() => cardCount(page)).toBeLessThan(insensitive);
    expect(await cardCount(page)).toBeGreaterThan(0);
    await page.locator('.search-option[data-option="caseSensitive"]').click();

    // Literal "knot|gold" matches nothing; as a regex the alternation hits.
    await page.locator(".search-input").fill("knot|gold");
    await expect(page.locator(".search-empty")).toBeVisible();
    await page.locator('.search-option[data-option="regex"]').click();
    await expect.poll(() => cardCount(page)).toBeGreaterThan(0);

    // Invalid regex: inline error, like the Settings JSON validation.
    await page.locator(".search-input").fill("(");
    await expect(page.locator(".search-error")).toContainText("Invalid regex");
  });

  test("Accept all applies every pending replacement — the previews are the confirmation", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "intro");

    await page.locator(".search-replace-toggle").click();
    await page.locator(".search-replace-input").fill("prologue");

    // Typing replace text turns cards into old→new previews and arms the
    // summary's Accept all with the pending count. No arm/confirm step.
    await expect(page.locator(".search-accept-all")).toHaveText("Accept all (3)");
    await expect(page.locator(".search-card-del").first()).toHaveText("intro");
    await expect(page.locator(".search-card-ins").first()).toHaveText("prologue");
    expect(await editorDoc(page)).toContain("-> intro");

    await page.locator(".search-accept-all").click();

    // The open editor view reflects the edit (invalidateFile refresh)…
    await expect.poll(() => editorDoc(page)).toContain("-> prologue");
    // …the toast reports the counts…
    await expect(
      page
        .locator(".shell-notification-message")
        .filter({ hasText: "Replaced 3 matches in 2 files" }),
    ).toBeVisible();
    // …and the frozen snapshot keeps every card, receipted.
    await expect(page.locator(".search-card-replaced-badge")).toHaveCount(3);
    await expect.poll(() => cardCount(page)).toBe(3);
  });

  test("per-card Accept applies one; skip excludes a card from Accept all", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "intro");
    await page.locator(".search-replace-toggle").click();
    await page.locator(".search-replace-input").fill("prologue");
    await expect(page.locator(".search-accept-all")).toHaveText("Accept all (3)");

    // Skip the first card: badged, excluded from the pending count.
    await page.locator(".search-card").first().locator(".search-card-skip").click();
    await expect(page.locator(".search-card-badge.skipped-badge")).toHaveCount(1);
    await expect(page.locator(".search-accept-all")).toHaveText("Accept all (2)");

    // Accept one pending card: it gets its receipt; the rest stay pending.
    await page.locator(".search-card-accept").first().click();
    await expect(page.locator(".search-card-replaced-badge")).toHaveCount(1);
    await expect(page.locator(".search-accept-all")).toHaveText("Accept all (1)");

    // Undo the skip: the card returns to pending.
    await page.locator(".search-card-undo-skip").click();
    await expect(page.locator(".search-accept-all")).toHaveText("Accept all (2)");
  });

  test("cmd-clicking a definition routes Find References into the panel", async ({
    page,
  }) => {
    // `EXTERNAL set_tint(color)` — the definition itself. Cmd-click there
    // must show references, not self-navigate (ruled 2026-08-24).
    await page
      .locator('.cm-content span:text-is("set_tint")')
      .first()
      .click({ modifiers: ["ControlOrMeta"] });

    // The scope chip renders inside the query box (Direction C, ruled
    // 2026-08-24), and the shared summary strip counts references.
    await expect(page.locator(".search-refs-chip")).toBeVisible();
    await expect(page.locator(".search-refs-chip-symbol")).toHaveText("set_tint");
    await expect(page.locator(".search-summary-count")).toContainText("references");
    await expect.poll(() => cardCount(page)).toBeGreaterThan(1);

    // References dressing (PR E): the declaration card pins first with the
    // accent border + decl badge; call sites carry their kind badges.
    await expect(page.locator(".search-card").first()).toHaveClass(/decl/);
    await expect(
      page.locator(".search-card").first().locator(".search-card-kind"),
    ).toHaveText("decl");
    await expect(page.locator(".search-card-kind", { hasText: "call" }).first()).toBeVisible();

    // The chip's ✕ clears references mode.
    await page.locator(".search-refs-chip-clear").click();
    await expect(page.locator(".search-refs-chip")).toHaveCount(0);
  });
});

test.describe("search cards (screenplay fixture)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/?fixture=screenplay");
    await page.waitForSelector(".cm-content", { timeout: 10000 });
  });

  test("editing inside a card writes through; the frozen snapshot keeps every card", async ({
    page,
  }) => {
    await openSearch(page);
    await search(page, "figure");
    const before = await cardCount(page);
    expect(before).toBeGreaterThan(0);

    // Baseline: the "figure" source line, unedited.
    expect(await editorDoc(page)).toContain("A figure steps into the light.");

    // Click into the first card's editable buffer at the match line and type
    // a marker. The whole-window edit commits after the idle pause (or on
    // blur) through applySearchRowEdit → ProjectSession.applyEdit.
    const firstCard = page.locator(".search-card").first();
    // Click the hit mark itself so the caret lands inside the match line
    // (a bare content click could land on a context line).
    await firstCard.locator(".search-card-editor .brink-search-hit").click();
    await page.keyboard.type("[EDITED]");

    // Blur the card (focus the query input) to flush the pending commit.
    await page.locator(".search-input").click();
    await expect.poll(() => editorDoc(page)).toContain("[EDITED]");

    // Exactly one region changed; sibling lines are untouched.
    const doc = await editorDoc(page);
    expect(doc.split("\n").filter((l) => l.includes("[EDITED]"))).toHaveLength(1);
    expect(doc).toContain("The lights dim.");
    expect(doc).toContain("-> interrogation.evidence");

    // Frozen snapshot: no card vanished because of the edit.
    await expect.poll(() => cardCount(page)).toBe(before);
  });
});
