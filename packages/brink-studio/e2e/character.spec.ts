import { test, expect, type Page } from "@playwright/test";

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

async function getDocText(page: Page): Promise<string> {
  return page.evaluate(() => (window as any).__brinkView.state.doc.toString());
}

async function getCursorPos(page: Page): Promise<number> {
  return page.evaluate(
    () => (window as any).__brinkView.state.selection.main.head,
  );
}

async function getCursorLine(page: Page): Promise<number> {
  return page.evaluate(() => {
    const view = (window as any).__brinkView;
    return view.state.doc.lineAt(view.state.selection.main.head).number;
  });
}

async function setCursor(page: Page, pos: number): Promise<void> {
  await page.evaluate((p) => {
    const view = (window as any).__brinkView;
    view.dispatch({ selection: { anchor: p } });
    view.focus();
  }, pos);
}

async function getLineText(page: Page, lineNum: number): Promise<string> {
  return page.evaluate((n) => {
    const view = (window as any).__brinkView;
    return view.state.doc.line(n).text;
  }, lineNum);
}

/** Get the visible (rendered) text of a .cm-line matching the given class. */
async function getVisibleLineText(
  page: Page,
  cls: string,
): Promise<string | null> {
  return page.locator(`.cm-line.${cls}`).first().textContent();
}

// ── Tests ──────────────────────────────────────────────────────────

test.describe("character lines", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".cm-content", { timeout: 5000 });
  });

  // ── 1. Template creation ────────────────────────────────────────

  test("Tab on double-blank creates character template with sigils hidden immediately", async ({
    page,
  }) => {
    await setEditorContent(page, "\n\n");
    await setCursor(page, 1); // second blank line
    await page.keyboard.press("Tab");

    // Raw doc should have the full sigil structure
    const doc = await getDocText(page);
    expect(doc).toContain("@:<>");

    // Sigils should be hidden even before any name is typed
    const visible = await getVisibleLineText(page, "brink-character");
    expect(visible).not.toContain("@");
    expect(visible).not.toContain(":<>");
  });

  // ── 2. Typing a name ───────────────────────────────────────────

  test("typing a name into character template renders styled with sigils hidden", async ({
    page,
  }) => {
    await setEditorContent(page, "\n\n");
    await setCursor(page, 1);
    await page.keyboard.press("Tab");
    await page.keyboard.type("JOHN");

    // Raw doc
    const doc = await getDocText(page);
    expect(doc).toContain("@JOHN:<>");

    // Visible: should show JOHN, not @JOHN:<>
    const visible = await getVisibleLineText(page, "brink-character");
    expect(visible).toContain("JOHN");
    expect(visible).not.toContain("@");
    expect(visible).not.toContain(":<>");
  });

  // ── 4. Arrow right escapes to next line ─────────────────────────

  test("arrow right through name skips hidden sigils and reaches next line", async ({
    page,
  }) => {
    // @JOHN:<>  (line 1, 8 chars)
    // hello     (line 2)
    await setEditorContent(page, "@JOHN:<>\nhello");

    // Place cursor at start of name (after @)
    await setCursor(page, 1);
    expect(await getCursorLine(page)).toBe(1);

    // Track cursor position at each step
    const positions: number[] = [await getCursorPos(page)];
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press("ArrowRight");
      positions.push(await getCursorPos(page));
    }

    // With atomicRanges on @ (0-1) and :<> (5-8):
    // Expected: 1, 2, 3, 4, 5, 9, 10, 11, 12, 13
    // (skip from 5 to 9, jumping over :<>\n)
    // At minimum, after the name we must reach line 2
    expect(positions).toContain(9); // start of "hello" on line 2

    // Should never get stuck — no two consecutive identical positions
    for (let i = 1; i < positions.length; i++) {
      expect(positions[i]).not.toBe(positions[i - 1]);
    }
  });

  // ── 5. Home/End don't get stuck on double-press ─────────────────

  test("End pressed twice does not move cursor into trailing sigil", async ({
    page,
  }) => {
    await setEditorContent(page, "@JOHN:<>\nhello");
    // Place cursor in middle of name: @JO|HN:<>
    await setCursor(page, 3);

    await page.keyboard.press("End");
    const pos1 = await getCursorPos(page);

    // Second End should not move further into :<>
    await page.keyboard.press("End");
    const pos2 = await getCursorPos(page);
    expect(pos2).toBe(pos1);
  });

  test("Home pressed twice does not move cursor into leading sigil", async ({
    page,
  }) => {
    await setEditorContent(page, "@JOHN:<>\nhello");
    // Place cursor in middle of name: @JO|HN:<>
    await setCursor(page, 3);

    await page.keyboard.press("Home");
    const pos1 = await getCursorPos(page);

    // Second Home should not move further into @
    await page.keyboard.press("Home");
    const pos2 = await getCursorPos(page);
    expect(pos2).toBe(pos1);
  });

  // ── 7. Backspace at start of name strips all sigils ─────────────

  test("backspace at start of name strips all sigils to plain narrative", async ({
    page,
  }) => {
    await setEditorContent(page, "@JOHN:<>");

    // Cursor right after @, before J
    await setCursor(page, 1);
    await page.keyboard.press("Backspace");

    // Should become plain "JOHN" — no @ or :<> remaining
    const doc = await getDocText(page);
    expect(doc.trim()).toBe("JOHN");
  });

  // ── 8. Delete at end of name folds next line ────────────────────

  test("delete at end of name folds next line content into name", async ({
    page,
  }) => {
    // @John:<>
    // Doe
    await setEditorContent(page, "@John:<>\nDoe");

    // Cursor after "John", before ":<>" → position 5
    await setCursor(page, 5);
    await page.keyboard.press("Delete");

    // Should become @JohnDoe:<> on one line
    const line1 = await getLineText(page, 1);
    expect(line1).toBe("@JohnDoe:<>");
  });

  // ── 8b. Enter mid-name + Delete round-trip ──────────────────────

  test("Enter splits name, left+Delete rejoins — full round-trip", async ({
    page,
  }) => {
    await setEditorContent(page, "@JohnDoe:<>");

    // A: cursor between John and Doe → @John|Doe:<>
    await setCursor(page, 5); // @=0, J=1, o=2, h=3, n=4, cursor=5 before D
    await page.keyboard.press("Enter");

    // B: should be @John:<> and Doe on next line
    expect(await getLineText(page, 1)).toBe("@John:<>");
    expect(await getLineText(page, 2)).toBe("Doe");
    expect(await getCursorLine(page)).toBe(2);

    // Press left to go back to end of line 1 (after name, before :<>)
    await page.keyboard.press("ArrowLeft");
    expect(await getCursorLine(page)).toBe(1);

    // C: press Delete to rejoin
    await page.keyboard.press("Delete");

    // D: should be @JohnDoe:<> again
    expect(await getLineText(page, 1)).toBe("@JohnDoe:<>");
  });

  // ── 10. Enter with content splits correctly ─────────────────────

  test("Enter mid-name creates character line + plain text on next line", async ({
    page,
  }) => {
    await setEditorContent(page, "@JohnDoe:<>");

    // Cursor between John and Doe
    await setCursor(page, 5);
    await page.keyboard.press("Enter");

    // Line 1 should keep the character structure with left part
    expect(await getLineText(page, 1)).toBe("@John:<>");

    // Line 2 should be plain text (not @Doe:<>)
    expect(await getLineText(page, 2)).toBe("Doe");

    // Cursor should be at start of line 2
    expect(await getCursorLine(page)).toBe(2);
  });

  // ── 11. Enter on empty character clears line ────────────────────

  test("Enter on empty character template clears the line", async ({
    page,
  }) => {
    await setEditorContent(page, "@:<>");

    // Cursor between @ and :<>
    await setCursor(page, 1);
    await page.keyboard.press("Enter");

    // Line should be empty
    const doc = await getDocText(page);
    expect(doc.trim()).toBe("");
  });

  // ── 12. Tab with content converts to parenthetical preserving name

  test("Tab on character with content wraps name in parenthetical", async ({
    page,
  }) => {
    await setEditorContent(page, "@JOHN:<>");

    // Cursor within the name
    await setCursor(page, 3);
    await page.keyboard.press("Tab");

    // Should become (JOHN)<>, not empty ()<>
    const line = await getLineText(page, 1);
    expect(line).toBe("(JOHN)<>");
  });

  // ── 14. Backspace on empty character clears with sigils hidden ──

  test("backspace on empty character template clears entire line", async ({
    page,
  }) => {
    await setEditorContent(page, "@:<>");

    await setCursor(page, 1); // between @ and :<>
    await page.keyboard.press("Backspace");

    const doc = await getDocText(page);
    expect(doc.trim()).toBe("");
  });

  // ── Enter at boundaries ─────────────────────────────────────────

  test("Enter at end of name inserts blank line below, keeps character line intact", async ({
    page,
  }) => {
    await setEditorContent(page, "@JOHN:<>");
    await setCursor(page, 5); // after JOHN, before :<>
    await page.keyboard.press("Enter");

    expect(await getLineText(page, 1)).toBe("@JOHN:<>");
    expect(await getLineText(page, 2)).toBe("");
    expect(await getCursorLine(page)).toBe(2);
  });

  test("Enter at start of name pushes name to next line, keeps template", async ({
    page,
  }) => {
    await setEditorContent(page, "@JOHN:<>");
    await setCursor(page, 1); // after @, before J
    await page.keyboard.press("Enter");

    expect(await getLineText(page, 1)).toBe("@:<>");
    expect(await getLineText(page, 2)).toBe("JOHN");
    expect(await getCursorLine(page)).toBe(2);
  });

  // ── Backspace from next line folds into character name ──────────

  test("backspace at start of line after character folds content into name", async ({
    page,
  }) => {
    // Simulate: @D:<> then ETECTIVE on next line (split name scenario)
    await setEditorContent(page, "@D:<>\nETECTIVE");
    // Cursor at start of ETECTIVE (line 2, pos 0)
    const line2Start = 6; // "@D:<>\n" = 5 chars + newline
    await setCursor(page, line2Start);
    await page.keyboard.press("Backspace");

    expect(await getLineText(page, 1)).toBe("@DETECTIVE:<>");
  });

  test("backspace at start of blank line after character goes to name end", async ({
    page,
  }) => {
    await setEditorContent(page, "@JOHN:<>\n");
    const line2Start = 9; // "@JOHN:<>\n"
    await setCursor(page, line2Start);
    await page.keyboard.press("Backspace");

    // Should fold empty line into character, leaving @JOHN:<>
    expect(await getLineText(page, 1)).toBe("@JOHN:<>");
    // Cursor should be at name end
    expect(await getCursorPos(page)).toBe(5);
  });

  // ── Status bar dropdown produces correct sigils ─────────────────

  test("converting narrative to character via dropdown wraps content correctly", async ({
    page,
  }) => {
    await setEditorContent(page, "JOHN");
    await setCursor(page, 2);

    // Click the element type button in the status bar to open dropdown
    await page.locator(".brink-status-element-btn").click();
    // Click "Character" in the dropdown
    await page.locator(".brink-element-dropdown-item", { hasText: "Character" }).click();

    const line = await getLineText(page, 1);
    expect(line).toBe("@JOHN:<>");
  });
});
