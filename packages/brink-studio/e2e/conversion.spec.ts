import { test, expect, type Page } from "@playwright/test";

/**
 * Exhaustive conversion tests.
 *
 * Every convertible element type can be converted to every other type
 * via the inline element picker (Alt+Enter + key). The content ("hello")
 * must survive every conversion. Round-trips (A->B->A) must produce the
 * original line.
 */

// ── Helpers ────────────────────────────────────────────────────────

async function setEditorContent(page: Page, content: string) {
  await page.evaluate((text) => {
    const view = (window as any).__brinkView;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    });
  }, content);
  await page.waitForTimeout(700);
}

async function getLineText(page: Page, lineNum: number): Promise<string> {
  return page.evaluate((n) => {
    const view = (window as any).__brinkView;
    return view.state.doc.line(n).text;
  }, lineNum);
}

async function setCursor(page: Page, pos: number): Promise<void> {
  await page.evaluate((p) => {
    const view = (window as any).__brinkView;
    view.dispatch({ selection: { anchor: p } });
    view.focus();
  }, pos);
}

async function convertViaPickerKey(page: Page, key: string): Promise<void> {
  await page.keyboard.press("Alt+Enter");
  await page.waitForTimeout(50);
  await page.keyboard.press(key);
  await page.waitForTimeout(50);
}

// ── Type definitions ──────────────────────────────────────────────

const TYPES = [
  { label: "Narrative",       line: "hello",        key: "n" },
  { label: "Choice-star",     line: "* hello",      key: "c" },
  { label: "Choice-plus",     line: "+ hello",      key: "s" },
  { label: "Gather",          line: "- hello",      key: "g" },
  { label: "Divert",          line: "-> hello",     key: "d" },
  { label: "Logic",           line: "~ hello",      key: "l" },
  { label: "Comment",         line: "// hello",     key: "/" },
  { label: "Tag",             line: "# hello",      key: "t" },
  { label: "Knot",            line: "=== hello",    key: "k" },
  { label: "Stitch",          line: "= hello",      key: "h" },
  { label: "Character",       line: "@hello:<>",    key: "@" },
  { label: "Parenthetical",   line: "(hello)<>",    key: "p" },
];

// ── Pairwise conversion tests ─────────────────────────────────────

test.describe("pairwise conversions", () => {
  for (const from of TYPES) {
    for (const to of TYPES) {
      if (from.label === to.label) continue;

      test(`${from.label} to ${to.label}`, async ({ page }) => {
        await page.goto("/");
        await page.waitForSelector(".cm-content", { timeout: 5000 });

        await setEditorContent(page, from.line);
        await setCursor(page, 1);
        await convertViaPickerKey(page, to.key);

        const result = await getLineText(page, 1);
        expect(result).toBe(to.line);
      });
    }
  }
});

// ── Round-trip tests ──────────────────────────────────────────────

test.describe("round-trip conversions", () => {
  for (const from of TYPES) {
    for (const to of TYPES) {
      if (from.label === to.label) continue;

      test(`${from.label} to ${to.label} and back`, async ({ page }) => {
        await page.goto("/");
        await page.waitForSelector(".cm-content", { timeout: 5000 });

        await setEditorContent(page, from.line);
        await setCursor(page, 1);

        // Convert to target
        await convertViaPickerKey(page, to.key);
        const intermediate = await getLineText(page, 1);
        expect(intermediate).toBe(to.line);

        // Convert back
        await convertViaPickerKey(page, from.key);
        const final = await getLineText(page, 1);
        expect(final).toBe(from.line);
      });
    }
  }
});
