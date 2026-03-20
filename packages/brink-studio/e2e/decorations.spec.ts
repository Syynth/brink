import { test, expect, type Page } from "@playwright/test";

/** Get the 1-based line numbers of all lines with brink-knot-header class. */
async function getKnotHeaderLines(page: Page): Promise<number[]> {
  return page.evaluate(() => {
    const lines = document.querySelectorAll(".cm-line");
    const result: number[] = [];
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].classList.contains("brink-knot-header")) {
        result.push(i + 1);
      }
    }
    return result;
  });
}

/** Get 1-based line numbers where text contains "=== ... ===" */
async function getKnotTextLines(page: Page): Promise<number[]> {
  return page.evaluate(() => {
    const lines = document.querySelectorAll(".cm-line");
    const result: number[] = [];
    for (let i = 0; i < lines.length; i++) {
      if (/===\s+\w+\s+===/.test(lines[i].textContent ?? "")) {
        result.push(i + 1);
      }
    }
    return result;
  });
}

test.describe("decoration tracking", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".brink-knot-header", { timeout: 5000 });
  });

  test("knot header decorations match text after each newline insertion", async ({ page }) => {
    // Check initial state
    let decoLines = await getKnotHeaderLines(page);
    let textLines = await getKnotTextLines(page);
    console.log(`Initial: decos=${JSON.stringify(decoLines)}, text=${JSON.stringify(textLines)}`);
    expect(decoLines).toEqual(textLines);

    // Place cursor at the very start of the document
    await page.locator(".cm-content").focus();
    // Use CM dispatch to set cursor at position 0
    await page.evaluate(() => {
      const cm = document.querySelector(".cm-editor") as HTMLElement;
      const view = (cm as any).cmView?.view;
      if (view) {
        view.dispatch({ selection: { anchor: 0 } });
      }
    });

    // Press Enter once — adds newline at pos 0
    await page.keyboard.press("Enter");
    await page.waitForTimeout(600); // wait for debounced compile

    decoLines = await getKnotHeaderLines(page);
    textLines = await getKnotTextLines(page);
    console.log(`After 1 Enter: decos=${JSON.stringify(decoLines)}, text=${JSON.stringify(textLines)}`);
    expect(decoLines).toEqual(textLines);

    // Press Enter again
    await page.keyboard.press("Enter");
    await page.waitForTimeout(600);

    decoLines = await getKnotHeaderLines(page);
    textLines = await getKnotTextLines(page);
    console.log(`After 2 Enters: decos=${JSON.stringify(decoLines)}, text=${JSON.stringify(textLines)}`);
    expect(decoLines).toEqual(textLines);

    // Press Enter a third time
    await page.keyboard.press("Enter");
    await page.waitForTimeout(600);

    decoLines = await getKnotHeaderLines(page);
    textLines = await getKnotTextLines(page);
    console.log(`After 3 Enters: decos=${JSON.stringify(decoLines)}, text=${JSON.stringify(textLines)}`);
    expect(decoLines).toEqual(textLines);
  });
});
