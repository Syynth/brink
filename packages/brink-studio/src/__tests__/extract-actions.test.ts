/**
 * Extract selection → knot / function code actions (#315 H).
 *
 * Two layers, mirroring inline-rename.test.ts:
 *  - Pure logic — `extractCodeActions` (selection → synthetic action list) and
 *    `isExtractAction` (the dispatch discriminator).
 *  - The in-editor flow — a real CM6 `EditorView` (jsdom) wired with
 *    `codeActionsExtension` + `extractActionsExtension`, driven through
 *    Ctrl-. → the menu → "Extract to knot" → the name prompt → a stubbed
 *    `computeExtract` (the test owns the safe/unsafe verdict) → apply.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState, EditorSelection } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import type { CodeAction, StructuralResult } from "@brink/wasm-types";
import {
  extractCodeActions,
  isExtractAction,
  EXTRACT_TO_KNOT_ACTION,
  EXTRACT_TO_FUNCTION_ACTION,
} from "@brink-lang/editor";
// The extension factories are internal to @brink-lang/editor; import them from
// the built entry the studio consumes (source) via the package's extensions
// module. They are re-exported for the studio wiring, but for a focused unit
// test we mount them directly through brinkStudio's public option surface.
import { brinkStudio } from "@brink-lang/editor";

const DOC = "=== opening ===\nThe lights dim.\nA figure steps.\n-> END\n";

const safe = (name: string): StructuralResult => ({
  ok: true,
  path: "main.ink",
  new_source: `=== opening ===\n-> ${name} ->\n-> END\n\n=== ${name} ===\nThe lights dim.\nA figure steps.\n->->\n`,
  cross_file_edits: [],
  introduced_diagnostics: [],
  safe: true,
});

const unsafe = (name: string): StructuralResult => ({
  ...safe(name),
  introduced_diagnostics: [
    {
      severity: "error",
      code: "E030",
      message: "gather label out of scope",
      path: "main.ink",
      line: 2,
      col: 1,
    },
  ],
  safe: false,
});

// ── Pure logic ──────────────────────────────────────────────────────

function stateWithSelection(from: number, to: number): EditorState {
  return EditorState.create({ doc: DOC, selection: EditorSelection.single(from, to) });
}

describe("extract action list", () => {
  it("offers Extract to knot/function for a multi-line selection", () => {
    // Select the two body lines.
    const from = DOC.indexOf("The lights");
    const to = DOC.indexOf("-> END");
    const actions = extractCodeActions(stateWithSelection(from, to));
    expect(actions.map((a) => a.title)).toEqual(["Extract to knot", "Extract to function"]);
    expect(actions[0].data.action).toBe(EXTRACT_TO_KNOT_ACTION);
    expect(actions[1].data.action).toBe(EXTRACT_TO_FUNCTION_ACTION);
  });

  it("offers nothing for an empty selection or a single-line selection", () => {
    expect(extractCodeActions(stateWithSelection(5, 5))).toEqual([]);
    // Within a single line.
    const from = DOC.indexOf("The lights");
    expect(extractCodeActions(stateWithSelection(from, from + 4))).toEqual([]);
  });

  it("isExtractAction discriminates the synthetic actions", () => {
    const knot: CodeAction = {
      title: "Extract to knot",
      kind: "refactor.extract",
      data: { action: EXTRACT_TO_KNOT_ACTION },
    };
    const fmt: CodeAction = { title: "Format", kind: "source", data: { action: "FormatStitch" } };
    expect(isExtractAction(knot)).toBe(true);
    expect(isExtractAction(fmt)).toBe(false);
  });
});

// ── Widget integration ──────────────────────────────────────────────

interface Applied {
  kind: string;
  name: string;
  result: StructuralResult;
}

/** A view wired with the code-actions + extract-prompt extensions;
 *  `computeExtract` is stubbed so the test controls the verdict. */
function mount(verdict: (name: string) => StructuralResult) {
  const computes: Array<{ kind: string; start: number; end: number; name: string }> = [];
  const applied: Applied[] = [];
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        brinkStudio({
          // Minimal required options (unused by this test's path).
          compile: () => ({ ok: true, diagnostics: [] }) as never,
          getSemanticTokens: () => [],
          getTokenTypeNames: () => [],
          // The code-actions menu with the extract seam wired.
          getCodeActions: () => [],
          computeExtract: (kind, start, end, name) => {
            computes.push({ kind, start, end, name });
            return verdict(name);
          },
          applyExtract: (kind, result, name) => applied.push({ kind, name, result }),
        }),
      ],
    }),
    parent: document.body,
  });
  return { view, computes, applied };
}

function selectBody(view: EditorView): void {
  const from = DOC.indexOf("The lights");
  const to = DOC.indexOf("-> END");
  view.dispatch({ selection: EditorSelection.single(from, to) });
}

function openMenu(view: EditorView): HTMLElement {
  // Run the Ctrl-. / Cmd-. keymap the way CM6 dispatches it (jsdom does not
  // route a raw keydown through the keymap facet). `runScopeHandlers` is CM6's
  // public entry for exactly this.
  const handled = runScopeHandlers(
    view,
    new KeyboardEvent("keydown", { key: ".", ctrlKey: true }),
    "editor",
  );
  const menu = document.querySelector<HTMLElement>(".brink-code-actions-menu");
  if (menu === null) throw new Error(`code-actions menu not opened (handled=${handled})`);
  return menu;
}

function menuItem(menu: HTMLElement, title: string): HTMLButtonElement {
  const items = [...menu.querySelectorAll<HTMLButtonElement>(".brink-code-action-item")];
  const item = items.find((b) => b.textContent === title);
  if (item === undefined) throw new Error(`menu item "${title}" not found`);
  return item;
}

function promptInput(): HTMLInputElement {
  const el = document.querySelector<HTMLInputElement>(".brink-inline-rename-input");
  if (el === null) throw new Error("extract name prompt not mounted");
  return el;
}

describe("extract widget flow", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it("Ctrl-. on a multi-line selection lists the extract actions", () => {
    const { view } = mount(() => safe("scene"));
    selectBody(view);
    const menu = openMenu(view);
    const titles = [...menu.querySelectorAll(".brink-code-action-item")].map((b) => b.textContent);
    expect(titles).toContain("Extract to knot");
    expect(titles).toContain("Extract to function");
    view.destroy();
  });

  it("choosing Extract to knot opens the name prompt → computes → applies (safe)", async () => {
    const { view, computes, applied } = mount(() => safe("scene"));
    selectBody(view);
    const menu = openMenu(view);
    menuItem(menu, "Extract to knot").click();
    // Menu closed, prompt mounted.
    expect(document.querySelector(".brink-code-actions-menu")).toBeNull();
    const input = promptInput();
    input.value = "scene";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));

    // The compute call is deferred off the paint path (#722) — nothing has
    // run yet right after Enter; wait a tick for it to settle.
    expect(computes).toHaveLength(0);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(computes).toHaveLength(1);
    expect(computes[0]).toMatchObject({ kind: "knot", name: "scene" });
    expect(applied).toHaveLength(1);
    expect(applied[0]).toMatchObject({ kind: "knot", name: "scene" });
    // A safe extract applies immediately and tears the prompt down.
    expect(document.querySelector(".brink-inline-rename-input")).toBeNull();
    view.destroy();
  });

  it("an unsafe extract surfaces the breakage report and applies only on force", async () => {
    const { view, applied } = mount((name) => unsafe(name));
    selectBody(view);
    const menu = openMenu(view);
    menuItem(menu, "Extract to function").click();
    const input = promptInput();
    input.value = "calc";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    // The compute call is deferred off the paint path (#722); wait a tick.
    await new Promise((resolve) => setTimeout(resolve, 0));

    // No immediate apply — the report is shown instead.
    expect(applied).toHaveLength(0);
    const report = document.querySelector<HTMLElement>(".brink-inline-rename-report")!;
    expect(report.hidden).toBe(false);
    expect(report.querySelectorAll(".brink-inline-rename-report-item")).toHaveLength(1);
    const force = report.querySelector<HTMLButtonElement>(".brink-inline-rename-force")!;
    expect(force.textContent).toBe("Extract anyway");

    // "Extract anyway" commits the unsafe extraction.
    force.click();
    expect(applied).toHaveLength(1);
    expect(applied[0]).toMatchObject({ kind: "function", name: "calc" });
    view.destroy();
  });

  it("destroying the editor tears the prompt down (no leaked DOM)", () => {
    const { view } = mount(() => safe("scene"));
    selectBody(view);
    const menu = openMenu(view);
    menuItem(menu, "Extract to knot").click();
    promptInput().value = "scene";
    view.destroy();
    expect(document.querySelector(".brink-inline-rename-input")).toBeNull();
    expect(document.querySelector(".brink-code-actions-menu")).toBeNull();
  });
});
