/**
 * Reachability of the `ok: false`/`safe: true` refusal shape through the real
 * `onApplyStructural` seams in `DocumentSessions` (#2578,
 * `docs/editor-consumer-guide.md` "Notes on the code-actions / extract
 * contract").
 *
 * `document-sessions.ts` documents two DIFFERENT reachability answers for the
 * same refusal shape:
 *
 *  - **Code actions** (`applyCodeAction`): forwards `resolveCodeAction`'s
 *    result to `onApplyStructural` UNCONDITIONALLY, with no `ok` filter.
 *  - **Extract** (`computeExtract`/`applyExtract`): `computeExtract` returns
 *    `null` on `!result.ok`, and `InlineNameInput` treats a `null` query
 *    result as "no commit" — so a refused extract never reaches
 *    `onApplyStructural`.
 *
 * `extract-actions.test.ts` already covers the extract widget's own
 * safe/unsafe/null-compute branches, but it stubs `computeExtract` directly
 * (via `brinkStudio(...)`), which bypasses `document-sessions.ts`'s real
 * `if (!result.ok) return null;` filter entirely. This file drives both
 * claims through a REAL mounted `DocumentSessions` view instead (the same
 * harness shape as `document-sessions.test.ts`), so it is document-sessions'
 * own code under test, not a stand-in for it.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorSelection } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import {
  DocumentSessions,
  ProjectSession,
  InMemoryFileProvider,
  type DocumentCallbacks,
} from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import type { StructuralResult } from "@brink/wasm-types";

const DOC = "=== opening ===\nThe lights dim.\nA figure steps.\n-> END\n";

interface Applied {
  path: string;
  description: string;
  result: StructuralResult;
}

async function mountHarness(applied: Applied[]) {
  await initWasm();
  const provider = new InMemoryFileProvider({ "main.ink": DOC });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const callbacks: DocumentCallbacks = {
    onApplyStructural: (req) => applied.push(req),
  };
  const documents = new DocumentSessions(project, callbacks);
  const container = document.createElement("div");
  document.body.appendChild(container);
  const dispose = documents.mountView("main.ink", "group-1", container);
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return { documents, view, dispose };
}

function openMenu(view: EditorView): HTMLElement {
  // Run the Ctrl-. / Cmd-. keymap the way CM6 dispatches it (jsdom does not
  // route a raw keydown through the keymap facet). `runScopeHandlers` is CM6's
  // public entry for exactly this — same technique as extract-actions.test.ts.
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

describe("onApplyStructural refusal reachability (#2578)", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it("an ok:false resolveCodeAction result reaches onApplyStructural unconditionally", async () => {
    const applied: Applied[] = [];
    const { view, dispose } = await mountHarness(applied);

    const menu = openMenu(view);
    menuItem(menu, "Mock quickfix").click();

    expect(applied).toHaveLength(1);
    expect(applied[0]!.result.ok).toBe(false);
    // The lie #2543/#2578 warn about: a refusal still ships `safe: true`.
    expect(applied[0]!.result.safe).toBe(true);
    expect(applied[0]!.result.error).toBe("invalid code-action data: mock action");

    dispose();
  });

  it("an ok:false extract result never reaches onApplyStructural", async () => {
    const applied: Applied[] = [];
    const { view, dispose } = await mountHarness(applied);

    // Select the body lines and choose "Extract to knot".
    const from = DOC.indexOf("The lights");
    const to = DOC.indexOf("-> END");
    view.dispatch({ selection: EditorSelection.single(from, to) });

    const menu = openMenu(view);
    menuItem(menu, "Extract to knot").click();
    const input = promptInput();
    // Naming the extraction after the file's existing "opening" knot triggers
    // a genuine `ok: false` (name collision) from the extract op.
    input.value = "opening";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));

    // The compute call is deferred off the paint path (#722); wait a tick.
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(applied).toEqual([]);
    // A null (refused) compute result tears the prompt down rather than
    // leaving it stuck open.
    expect(document.querySelector(".brink-inline-rename-input")).toBeNull();

    dispose();
  });
});
