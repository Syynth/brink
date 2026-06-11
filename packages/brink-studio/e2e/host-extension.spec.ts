/**
 * Embedder extension API e2e (shell issue 5.4 / #95, reworked by #146, spec §8).
 *
 * The playground mounts the example host extension (host.example.functions)
 * through the StudioExtensions mount config and registers its pretend
 * host-capability manifest at mount — these specs verify the panel is an
 * equal citizen: it appears in the strip, palette, and hamburger;
 * opens/docks; renders the manifest's metadata (signatures + doc comments);
 * click-to-insert lands ONLY a call site (`~ fn(args)` — never an EXTERNAL
 * declaration; those already live in the story) at the editor cursor
 * (StudioApi.insertText) and raises a notification; the host command
 * navigates via dispatch("editor.reveal"); drag re-docks it and the
 * placement survives a reload; and loading without the extension
 * (`?ext=none`) drops the persisted host ids cleanly.
 */

import { test, expect, type Locator, type Page } from "@playwright/test";

const PANEL_ID = "host.example.functions";
const PANEL_TITLE = "Host Functions";

function stripButton(page: Page, dock: "left" | "right" | "bottom", label: string): Locator {
  return page.locator(`.shell-strip-${dock} .shell-strip-btn[aria-label="${label}"]`);
}

function panel(page: Page): Locator {
  return page.locator(`[data-toolwindow="${PANEL_ID}"]`);
}

async function gotoStudio(page: Page, query = ""): Promise<void> {
  await page.goto(`/${query}`);
  await page.waitForSelector(".cm-content", { timeout: 10000 });
}

/** Press on a strip button and drag past the threshold (ghost appears). */
async function startDrag(page: Page, button: Locator): Promise<void> {
  const box = await button.boundingBox();
  if (!box) throw new Error("strip button has no bounding box");
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx + 24, cy + 8, { steps: 4 });
  await expect(page.locator(".shell-drag-ghost")).toBeVisible();
}

/** Continue the active drag onto a drop zone (it must highlight). */
async function dragOver(page: Page, zone: Locator): Promise<void> {
  const box = await zone.boundingBox();
  if (!box) throw new Error("drop zone has no bounding box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 8 });
  await expect(zone).toHaveClass(/active/);
}

test("host panel appears in the strip, palette, and hamburger; opens like a built-in", async ({
  page,
}) => {
  await gotoStudio(page);

  // Strip icon in its default dock (right/end), closed by default.
  const button = stripButton(page, "right", PANEL_TITLE);
  await expect(button).toBeVisible();
  await expect(panel(page)).toHaveCount(0);

  // Palette lists the generated view-toggle command for the host window.
  await page.keyboard.press("Meta+Shift+P");
  const input = page.locator(".shell-palette-input");
  await expect(input).toBeVisible();
  await input.fill("Host Functions");
  await expect(
    page.locator(".shell-palette-item", { hasText: `View: Toggle ${PANEL_TITLE}` }),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  // Hamburger menu carries it too (generated from the command registry).
  await page.locator(".shell-hamburger").click();
  await expect(
    page.locator(".shell-menu-item", { hasText: `View: Toggle ${PANEL_TITLE}` }),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  // Strip click opens the panel in the right dock with its content: rows
  // render the manifest's metadata — the typed signature plus the doc
  // comment as secondary text.
  await button.click();
  await expect(page.locator(`.shell-dock-right [data-toolwindow="${PANEL_ID}"]`)).toBeVisible();
  await expect(page.locator(".host-example-fn").first()).toBeVisible();
  const hasItemRow = page.locator(".host-example-fn", { hasText: "has_item" });
  await expect(hasItemRow).toContainText("has_item(item: item_id) -> bool");
  await expect(hasItemRow.locator(".host-example-fn-doc")).toHaveText(
    "True if the party carries the item.",
  );
});

test("click inserts ONLY the call snippet at the editor cursor and notifies", async ({
  page,
}) => {
  await gotoStudio(page);
  await stripButton(page, "right", PANEL_TITLE).click();

  // Place the cursor at a known offset in the entry file.
  await page.evaluate(() => {
    const view = (window as any).__brinkView;
    view.dispatch({ selection: { anchor: 0 } });
    view.focus();
  });

  await page.locator(".host-example-fn", { hasText: "has_item" }).click();

  // The cursor sits after the insertion, so [0, head) is exactly the
  // inserted text: a call site only — no EXTERNAL declaration (the story
  // already declares the host functions; the panel never inserts them).
  const inserted = await page.evaluate(() => {
    const view = (window as any).__brinkView;
    return view.state.doc.sliceString(0, view.state.selection.main.head);
  });
  expect(inserted).toBe("~ has_item(item)\n");
  expect(inserted).toContain("~ has_item(");
  expect(inserted).not.toContain("EXTERNAL");

  // The notify() toast appeared.
  await expect(
    page.locator(".shell-notification", { hasText: "Inserted has_item(item)" }),
  ).toBeVisible();
});

test("the registered manifest drives diagnostics; the external-check flag suppresses them", async ({
  page,
}) => {
  await gotoStudio(page);

  const problemsBadge = page
    .locator('.shell-strip-bottom .shell-strip-btn[aria-label="Problems"]')
    .locator(".shell-strip-badge");

  // The default project (manifest registered, EXTERNALs declared) compiles
  // clean: no Problems badge.
  await expect(problemsBadge).toHaveCount(0);

  // A literal type mismatch against the manifest — gain_gold takes an int
  // (E041). Without the manifest this line would compile fine.
  await page.evaluate(() => {
    const view = (window as any).__brinkView;
    view.dispatch({ changes: { from: 0, insert: '~ gain_gold("lots")\n' } });
  });
  await expect(problemsBadge).toHaveText("1", { timeout: 10000 });

  // Settings → external check "off" recompiles immediately: the
  // manifest-driven diagnostic is suppressed, badge gone.
  await page.keyboard.press("Meta+,");
  await expect(page.locator(".settings-doc")).toBeVisible();
  await page.locator(".settings-select").selectOption("off");
  await expect(problemsBadge).toHaveCount(0, { timeout: 10000 });
});

test("the host command navigates via dispatch('editor.reveal', …)", async ({ page }) => {
  await gotoStudio(page);
  await stripButton(page, "right", PANEL_TITLE).click();

  // Move the cursor away from the top first.
  await page.evaluate(() => {
    const view = (window as any).__brinkView;
    view.dispatch({ selection: { anchor: 20 } });
  });

  await page.locator(".host-example-reveal").click();
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).__brinkView.state.selection.main.head),
    )
    .toBe(0);
});

test("drag to another dock persists across a reload; ?ext=none loads clean", async ({
  page,
}) => {
  await gotoStudio(page);

  // Open it first (closed by default), then drag the strip icon from the
  // right strip to bottom.end.
  await stripButton(page, "right", PANEL_TITLE).click();
  await expect(page.locator(`.shell-dock-right [data-toolwindow="${PANEL_ID}"]`)).toBeVisible();
  await startDrag(page, stripButton(page, "right", PANEL_TITLE));
  await dragOver(page, page.locator('.shell-strip-dropzone[data-zone="bottom.end"]'));
  await page.mouse.up();
  await expect(stripButton(page, "bottom", PANEL_TITLE)).toBeVisible();
  await expect(
    page.locator(`.shell-dock-bottom [data-toolwindow="${PANEL_ID}"]`),
  ).toBeVisible();

  // Reload (extension installed): the dragged placement is restored —
  // bottom strip icon, window still open down there.
  await gotoStudio(page);
  await expect(stripButton(page, "bottom", PANEL_TITLE)).toBeVisible();
  await expect(stripButton(page, "right", PANEL_TITLE)).toHaveCount(0);
  await expect(
    page.locator(`.shell-dock-bottom [data-toolwindow="${PANEL_ID}"]`),
  ).toBeVisible();

  // Reload WITHOUT the extension: the persisted layout still mentions the
  // host panel, which must be dropped silently (§7.1) — no icon anywhere,
  // no console errors, shell intact.
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  await gotoStudio(page, "?ext=none");
  await expect(stripButton(page, "left", "Binder")).toBeVisible();
  await expect(page.locator(`.shell-strip-btn[aria-label="${PANEL_TITLE}"]`)).toHaveCount(0);
  await expect(panel(page)).toHaveCount(0);
  expect(errors).toEqual([]);

  // And reloading WITH it again falls back to its default placement (the
  // stored placement was dropped on the ?ext=none load): right strip.
  await gotoStudio(page);
  await expect(stripButton(page, "right", PANEL_TITLE)).toBeVisible();
});
