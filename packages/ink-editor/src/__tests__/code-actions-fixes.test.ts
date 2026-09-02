/**
 * The code-actions menu lists the auto-fixes for the diagnostics under the
 * cursor (#3377, `docs/autofix-spec.md` §7): every tier, one click applies
 * one fix, and a `placeholder` fix moves the caret into the hole it left.
 *
 * A `Fix` is NOT a `CodeAction` — it carries its own `edits` instead of an
 * opaque `resolveCodeAction` payload — so the menu holds both currencies and
 * dispatches each through its own seam. These tests drive the real
 * `codeActionsExtension`; the wasm side is stubbed, since what is under test
 * is the menu's own wiring.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import type { CodeAction, Fix } from "@brink/wasm-types";
import { codeActionsExtension } from "../code-actions.js";
import { editorActionKeymap } from "../editor-actions.js";

const DOC = "=== opening ===\nThe lights dim.\n-> END\n";

const ACTIONS: CodeAction[] = [
  { title: "Sort knots alphabetically", kind: "source", data: { action: "SortKnots" } },
];

const SUGGESTED_FIX: Fix = {
  code: "E025",
  title: "Import `ambush` from `quest`",
  applicability: "suggested",
  edits: [{ path: "main.ink", start: 0, end: 0, new_text: "IMPORT { ambush } FROM quest\n" }],
};

const PLACEHOLDER_FIX: Fix = {
  code: "E173",
  title: "Add the required attribute",
  applicability: "placeholder",
  edits: [{ path: "main.ink", start: 4, end: 4, new_text: "" }],
  caret: { path: "main.ink", offset: 12 },
};

interface Harness {
  view: EditorView;
  appliedFixes: Fix[];
  selectedActions: CodeAction[];
}

function mount(fixes: Fix[], resolveFixCaret?: (fix: Fix) => number | null): Harness {
  const appliedFixes: Fix[] = [];
  const selectedActions: CodeAction[] = [];
  const view = new EditorView({
    state: EditorState.create({
      doc: DOC,
      extensions: [
        editorActionKeymap(),
        codeActionsExtension({
          getCodeActions: () => ACTIONS,
          getFixes: () => fixes,
          applyFix: (fix) => appliedFixes.push(fix),
          resolveFixCaret,
          onSelect: (action) => selectedActions.push(action),
        }),
      ],
    }),
    parent: document.body,
  });
  return { view, appliedFixes, selectedActions };
}

/** Run the Ctrl-. keymap the way CM6 dispatches it (jsdom does not route a
 *  raw keydown through the keymap facet). */
function openMenu(view: EditorView): HTMLElement {
  const handled = runScopeHandlers(
    view,
    new KeyboardEvent("keydown", { key: ".", ctrlKey: true }),
    "editor",
  );
  const menu = document.querySelector<HTMLElement>(".brink-code-actions-menu");
  if (menu === null) throw new Error(`code-actions menu not opened (handled=${handled})`);
  return menu;
}

function itemTitles(menu: HTMLElement): string[] {
  return [...menu.querySelectorAll(".brink-code-action-item")].map(
    (el) => el.textContent ?? "",
  );
}

let harness: Harness | null = null;

afterEach(() => {
  harness?.view.destroy();
  harness = null;
  document.querySelector(".brink-code-actions-menu")?.remove();
});

describe("code-actions menu — auto-fixes", () => {
  it("lists the fixes ahead of the structural code actions", () => {
    harness = mount([SUGGESTED_FIX]);
    const menu = openMenu(harness.view);
    expect(itemTitles(menu)).toEqual([
      "Import `ambush` from `quest`",
      "Sort knots alphabetically",
    ]);
  });

  it("routes a chosen fix to applyFix, not to the code-action seam", () => {
    harness = mount([SUGGESTED_FIX]);
    const menu = openMenu(harness.view);
    menu.querySelector<HTMLButtonElement>(".brink-code-action-item")?.click();
    expect(harness.appliedFixes).toEqual([SUGGESTED_FIX]);
    expect(harness.selectedActions).toEqual([]);
  });

  it("still routes a chosen code action to onSelect", () => {
    harness = mount([SUGGESTED_FIX]);
    const menu = openMenu(harness.view);
    const items = menu.querySelectorAll<HTMLButtonElement>(".brink-code-action-item");
    items[1]?.click();
    expect(harness.selectedActions).toEqual(ACTIONS);
    expect(harness.appliedFixes).toEqual([]);
  });

  it("moves the caret into a placeholder fix's hole", () => {
    harness = mount([PLACEHOLDER_FIX], (fix) => fix.caret?.offset ?? null);
    const menu = openMenu(harness.view);
    menu.querySelector<HTMLButtonElement>(".brink-code-action-item")?.click();
    expect(harness.appliedFixes).toEqual([PLACEHOLDER_FIX]);
    expect(harness.view.state.selection.main.head).toBe(12);
  });

  it("leaves the caret alone for a fix that names no hole", () => {
    harness = mount([SUGGESTED_FIX], () => 12);
    const before = harness.view.state.selection.main.head;
    const menu = openMenu(harness.view);
    menu.querySelector<HTMLButtonElement>(".brink-code-action-item")?.click();
    expect(harness.view.state.selection.main.head).toBe(before);
  });

  it("opens on fixes alone when there are no code actions", () => {
    const appliedFixes: Fix[] = [];
    const view = new EditorView({
      state: EditorState.create({
        doc: DOC,
        extensions: [
          editorActionKeymap(),
          codeActionsExtension({
            getCodeActions: () => [],
            getFixes: () => [SUGGESTED_FIX],
            applyFix: (fix) => appliedFixes.push(fix),
          }),
        ],
      }),
      parent: document.body,
    });
    harness = { view, appliedFixes, selectedActions: [] };
    expect(itemTitles(openMenu(view))).toEqual(["Import `ambush` from `quest`"]);
  });
});
