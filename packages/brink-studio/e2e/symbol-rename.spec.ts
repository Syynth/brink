import { test, expect, type Page } from "@playwright/test";

/**
 * Knot/stitch Rename (#305) — the shared symbol context menu's "Rename…" item
 * opens a safe-by-default prompt: a clean rename applies immediately; a rename
 * that would introduce diagnostics flips to a breakage report whose only
 * override is an explicit "Force rename".
 */

function binderKnot(page: Page, name: string) {
  return page.locator(".brink-binder-knot", {
    has: page.locator(".brink-binder-label", { hasText: new RegExp(`^${name}$`) }),
  });
}

async function openRename(page: Page, knot: string): Promise<void> {
  await binderKnot(page, knot).first().click({ button: "right" });
  const item = page.locator(".brink-context-menu-item", { hasText: "Rename" });
  await expect(item).toBeVisible();
  await item.click();
  await expect(page.locator("#brink-rename-input")).toBeVisible();
  // Wait on the prompt being *seeded*, not merely mounted (#2511). The field is
  // uncontrolled and `confirmName()` reads `input.value`, so filling it before
  // the seed lands lets the seed overwrite the typed name; the prompt then sees
  // `name === currentName`, closes without renaming, and the binder assertion
  // below fails against a rename that never happened. Holding the current name
  // is the app's own signal that the prompt is ready for input — the same wait
  // the inline (F2) rename test already makes on `.brink-inline-rename-input`.
  await expect(page.locator("#brink-rename-input")).toHaveValue(knot);
}

/**
 * Records how the rename prompt's field is seeded, and where the caret sits
 * immediately after each seeding write, in the REAL browser (#2595).
 *
 * `docs/studio-shell-spec.md` §7.7.1 warns that a caret assertion can pass
 * vacuously because the seed itself parks the caret at the end of the value.
 * That warning was measured in jsdom; the half of it that contrasted jsdom
 * with "a real browser, seeded through the `value` *attribute*" was inferred,
 * never observed. This hook observes it: it wraps the two seeding entry
 * points React can use on an `<input>` — the `value` and `defaultValue` IDL
 * properties — and logs the caret right after each write reaches
 * `#brink-rename-input`. Both wrappers delegate to the original setter, so
 * the app's behaviour is unchanged; only the reading is added.
 *
 * The finding is not carried by entries existing at all — `defaultValue` is
 * also an attribute-reflecting IDL property (it mirrors the `value` content
 * attribute, the same path the control's `viaSetAttribute` case exercises),
 * so a write there produces an entry too. It is carried by the assertion
 * below that `seeded.map((e) => e.prop)` contains `"value"`: that is the
 * specific property React's own seed write goes through, and it has no
 * attribute-only counterpart to be confused with.
 */
type SeedProbeEntry = { prop: "value" | "defaultValue"; text: string; start: number | null; end: number | null };

async function installSeedProbe(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const probe: SeedProbeEntry[] = [];
    (window as unknown as { __brinkSeedProbe: SeedProbeEntry[] }).__brinkSeedProbe = probe;
    for (const prop of ["value", "defaultValue"] as const) {
      const desc = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, prop);
      if (!desc?.get || !desc.set) continue;
      const { get, set } = desc;
      Object.defineProperty(HTMLInputElement.prototype, prop, {
        configurable: true,
        enumerable: desc.enumerable,
        get,
        set(this: HTMLInputElement, next: string) {
          set.call(this, next);
          if (this.id === "brink-rename-input") {
            probe.push({
              prop,
              text: get.call(this) as string,
              start: this.selectionStart,
              end: this.selectionEnd,
            });
          }
        },
      });
    }
  });
}

test.describe("knot/stitch rename (#305)", () => {
  /**
   * #2595 — settles, in a real browser, the factual claim §7.7.1 records about
   * where a seeded field's caret starts.
   *
   * The measured answer contradicts the recorded one: React seeds an
   * uncontrolled `defaultValue` field through the `.value` PROPERTY, and a
   * property write parks the caret at the end of the value in Chromium exactly
   * as it does in jsdom. The (0, 0) reading the spec attributed to "a real
   * browser" belongs to the `value` ATTRIBUTE path, which React does not take
   * — so the vacuity trap the seed-race suites guard against is a platform
   * behaviour, not a jsdom artifact. The control assertions below pin both
   * halves of that split so the finding cannot rot back into an assumption.
   */
  test("a defaultValue-seeded field parks the caret at the end in a real browser (#2595)", async ({
    page,
  }) => {
    await installSeedProbe(page);
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    // CONTROL — the raw platform split, measured in this browser rather than
    // assumed. Attribute-seeding leaves the caret at the start; a write to the
    // `.value` property moves it to the end. Both readings are what §7.7.1
    // now cites.
    const control = await page.evaluate(() => {
      const attrSeeded = document.createElement("div");
      attrSeeded.innerHTML = '<input value="barter">';
      document.body.appendChild(attrSeeded);
      const viaAttribute = attrSeeded.firstElementChild as HTMLInputElement;

      const viaSetAttribute = document.createElement("input");
      document.body.appendChild(viaSetAttribute);
      viaSetAttribute.setAttribute("value", "barter");

      const viaProperty = document.createElement("input");
      document.body.appendChild(viaProperty);
      viaProperty.value = "barter";

      const read = (el: HTMLInputElement) => [el.selectionStart, el.selectionEnd];
      const result = {
        viaAttribute: read(viaAttribute),
        viaSetAttribute: read(viaSetAttribute),
        viaProperty: read(viaProperty),
      };
      attrSeeded.remove();
      viaSetAttribute.remove();
      viaProperty.remove();
      return result;
    });
    expect(control.viaAttribute).toEqual([0, 0]);
    expect(control.viaSetAttribute).toEqual([0, 0]);
    expect(control.viaProperty).toEqual(["barter".length, "barter".length]);

    // PRODUCTION — open the real prompt by the real user path and read what
    // React's seed actually did to the caret.
    await openRename(page, "barter");
    const seeded = await page.evaluate(() =>
      (window as unknown as { __brinkSeedProbe: SeedProbeEntry[] }).__brinkSeedProbe.filter(
        (e) => e.text === "barter",
      ),
    );

    // React reaches the field through an IDL property, not the `value`
    // attribute — the premise the spec's (0, 0) claim rested on.
    expect(seeded.length).toBeGreaterThan(0);
    expect(seeded.map((e) => e.prop)).toContain("value");

    // And that seed leaves the caret at the END, exactly as jsdom does. Scoped
    // to the `value`-property entries only: `defaultValue` writes (if any land
    // here) read back whatever the field held at that moment, which is
    // React's write ORDER, not a caret claim this test can make on its own —
    // asserting them here would encode an implementation detail of React as
    // though it were platform behaviour.
    const valueWrites = seeded.filter((e) => e.prop === "value");
    expect(valueWrites.length).toBeGreaterThan(0);
    for (const entry of valueWrites) {
      expect(entry.start).toBe("barter".length);
      expect(entry.end).toBe("barter".length);
    }
  });

  test("a clean rename applies and the binder shows the new name", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });
    await expect(binderKnot(page, "barter")).toHaveCount(1);

    await openRename(page, "barter");
    await page.locator("#brink-rename-input").fill("haggle");
    await page.keyboard.press("Enter");

    // Prompt closes; the binder outline refreshes with the renamed knot.
    await expect(page.locator("#brink-rename-input")).toBeHidden();
    await expect(binderKnot(page, "haggle")).toHaveCount(1);
    await expect(binderKnot(page, "barter")).toHaveCount(0);
  });

  test("F2 in the editor opens the inline rename seeded at the cursor (#323)", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    // Open a knot in the editor, then click its name token in the header to
    // place the cursor squarely on the symbol (robust to CM line layout).
    await binderKnot(page, "barter").first().click();
    await page.waitForSelector(".cm-content");
    const nameToken = page.locator(".cm-content").getByText("barter", { exact: true }).first();
    await expect(nameToken).toBeVisible();
    await nameToken.click();
    await page.keyboard.press("F2");

    // The INLINE rename widget mounts in the editor (not the modal), seeded with
    // the current name — proof F2 resolved a renameable symbol and routed
    // through the in-editor surface (#323).
    const input = page.locator(".brink-inline-rename-input");
    await expect(input).toBeVisible();
    await expect(input).toHaveValue("barter");
    await expect(page.locator("#brink-rename-input")).toHaveCount(0); // no modal
    await input.fill("haggling");
    await page.keyboard.press("Enter");

    // Widget tears down; the safe rename applied (binder outline reflects it).
    await expect(input).toHaveCount(0);
    await expect(binderKnot(page, "haggling")).toHaveCount(1);
    await expect(binderKnot(page, "barter")).toHaveCount(0);

    // The open symbol-view tab survives its own rename: it re-keys (tab label
    // follows the new name) and the view re-resolves to the renamed knot rather
    // than degrading to the full file (#305 follow-up).
    await expect(page.locator(".brink-tab-label", { hasText: "haggling" })).toHaveCount(1);
    await expect(page.locator(".brink-tab-label", { hasText: /^barter\b/ })).toHaveCount(0);
    await expect(
      page.locator(".cm-line", { hasText: "=== haggling ===" }).first(),
    ).toBeVisible();
  });

  test("inline rename shows '⚠ breaks N' + inline report; Rename anyway overrides (#324)", async ({
    page,
  }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    // Open `threshold` in the editor and F2 on its name token.
    await binderKnot(page, "threshold").first().click();
    await page.waitForSelector(".cm-content");
    const nameToken = page.locator(".cm-content").getByText("threshold", { exact: true }).first();
    await expect(nameToken).toBeVisible();
    await nameToken.click();
    await page.keyboard.press("F2");

    const input = page.locator(".brink-inline-rename-input");
    await expect(input).toBeVisible();
    // Same seeded-before-typing discipline as `openRename` (#2511). The inline
    // widget seeds its value while building its DOM, so unlike the modal it has
    // no window in which the field is observable but empty — this is a guard
    // that it stays that way, not a fix for a live race.
    await expect(input).toHaveValue("threshold");
    // Rename `threshold` onto the existing `intro` knot → duplicate-knot break.
    await input.fill("intro");

    // The badge appears (debounced) with the breakage count, and clicking it
    // expands the INLINE report (not a modal).
    const badge = page.locator(".brink-inline-rename-badge");
    await expect(badge).toBeVisible();
    await expect(badge).toContainText(/breaks \d+/);
    await badge.click();
    const report = page.locator(".brink-inline-rename-report");
    await expect(report).toBeVisible();
    await expect(report.locator(".brink-inline-rename-report-item")).not.toHaveCount(0);
    await expect(page.locator("#brink-rename-input")).toHaveCount(0); // no modal

    // Still not applied — `threshold` is intact.
    await expect(binderKnot(page, "threshold")).toHaveCount(1);

    // "Rename anyway" overrides; the rename applies (now two `intro`).
    await page.locator(".brink-inline-rename-force").click();
    await expect(input).toHaveCount(0);
    await expect(binderKnot(page, "threshold")).toHaveCount(0);
    await expect(binderKnot(page, "intro")).toHaveCount(2);
  });

  test("a colliding rename shows the breakage report; Force overrides", async ({ page }) => {
    // #696 — this used to flake on `report.toBeVisible()`: the collision
    // analysis (`performSymbolRename` → `EditorSession.rename_symbol`, a
    // synchronous wasm call) ran inline in the Enter handler's own frame, so
    // under CI's parallel workers it could occasionally block paint (and
    // Playwright's CDP polling) past even a widened timeout — PR #714's
    // "raise the timeout" fix (`test.slow()` + 20s waits) reduced but never
    // eliminated the recurrence (PR #1500, PR #1888).
    //
    // #722 later fixed this exact defect for the sibling INLINE rename
    // widget (see "inline rename shows '⚠ breaks N'..." above in this file,
    // which needs no special timeout) by committing a `.brink-inline-rename-
    // badge--pending` state synchronously — so a paint lands before the
    // heavy call runs — then deferring the call itself off the paint path.
    // It never touched this MODAL prompt, which is the surface this test
    // actually drives; that gap is why the flake outlived #722.
    // `SymbolRenamePrompt` now takes the same discipline (see its OFF THE
    // PAINT PATH doc comment) via a `.brink-rename-pending` indicator, so the
    // analysis is no longer racing paint here either.
    //
    // #714's `test.slow()` + 20s waits are kept below deliberately, not
    // removed: nothing about this fix's mechanism shortens the wall clock
    // from Enter to the report becoming visible under real CI contention —
    // `performSymbolRename` still makes the same synchronous, unbounded wasm
    // call one idle tick later (`idle-schedule.ts`'s own doc comment: "not a
    // substitute for a worker: a call that itself blocks for seconds still
    // blocks once it starts"). The deferral only guarantees a paint of the
    // pending indicator lands first; it does not bound the analysis itself.
    // A clean `--repeat-each=25` run on an idle box is not evidence about the
    // contended case #714 was fixing, so the widened budget stays until CI
    // has run this spec green repeatedly on the fixed code and a follow-up
    // can retire it on its own merits.
    //
    // The pending indicator itself is real (and DOM-committed before the
    // deferred call ever starts — see the doc comment on `SymbolRenamePrompt`
    // and the deterministic, fake-timer-driven proof in
    // `symbol-rename-prompt-pending.test.tsx`), but it is deliberately NOT
    // asserted here: measured with `--repeat-each=15` against this real
    // browser, its visibility flashed too briefly for Playwright's CDP
    // polling to reliably observe on a mostly-idle page (10/15 runs missed
    // it, 0/15 missed the report) — an assertion on it would trade one flake
    // for another. A live browser's polling granularity is the wrong tool for
    // proving a sub-poll-interval ordering; the unit test's fake timers are.
    test.slow();
    await page.goto("/");
    await page.waitForSelector(".brink-binder-knot", { timeout: 8000 });

    // Rename `threshold` onto the existing `intro` knot → duplicate-knot breakage.
    await openRename(page, "threshold");
    await page.locator("#brink-rename-input").fill("intro");
    await page.keyboard.press("Enter");

    // Safe-by-default: the rename is blocked and the report is shown instead.
    const report = page.locator(".brink-rename-report");
    await expect(report).toBeVisible({ timeout: 20000 });
    await expect(report).toContainText(/would break/i);
    await expect(report.locator(".brink-rename-diag")).not.toHaveCount(0);
    // Still not applied — `threshold` is intact.
    await expect(binderKnot(page, "threshold")).toHaveCount(1);

    // Force overrides; the report closes and the rename applies (now two `intro`).
    await page.locator(".brink-rename-force").click();
    await expect(report).toBeHidden({ timeout: 20000 });
    await expect(binderKnot(page, "threshold")).toHaveCount(0);
    await expect(binderKnot(page, "intro")).toHaveCount(2);
  });
});
