/**
 * CM6 read-only wiring for a mounted stdlib file's view (issue #2306/#2343,
 * "Mounted stdlib presents as a read-only library node", presentation half).
 *
 * `EditorSession::is_read_only`/`ProjectSession.applyEdit` already refuse a
 * write to a mounted path at the wasm/session layer (#2342) — this file
 * covers the layer above that: a CM6 view mounted on such a file must be
 * genuinely non-editable (`EditorState.readOnly` + `EditorView.editable`),
 * not merely have its keystrokes silently no-op on the next wasm round-trip
 * (`DocumentSessions.slotExtensions`, `packages/ink-editor/src/
 * document-sessions.ts`).
 *
 * Runs against the brink-web mock (src/__mocks__/brink-web.ts), whose
 * `__mockMarkReadOnlyForTest` seam marks a path read-only the same way the
 * real `EditorSession::new()` marks every stdlib key on construction.
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import {
  DocumentSessions,
  ProjectSession,
  InMemoryFileProvider,
} from "@brink-lang/editor";
import { EditorSessionHandle, initWasm } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

const MOUNTED_PATH = "std/core.brink";
const MOUNTED_TEXT = "=== core ===\n-> DONE\n";
const MAIN_INK = "-> DONE\n";

/** Reach into the mock's raw session to seed a read-only file — the
 *  wasm-boundary equivalent of the real constructor's stdlib mount (see the
 *  module doc). `EditorSessionHandle.session` is intentionally private on
 *  the production type; the cast is a test-only seam (mirrors
 *  `mounted-stdlib-readonly.test.ts`'s identical helper). */
function markReadOnly(handle: EditorSessionHandle, path: string, source: string): void {
  (
    handle as unknown as {
      session: { __mockMarkReadOnlyForTest(path: string, source: string): void };
    }
  ).session.__mockMarkReadOnlyForTest(path, source);
}

function viewIn(container: HTMLElement): EditorView {
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return view;
}

async function makeHarness(): Promise<{
  documents: DocumentSessions;
  container: HTMLElement;
  cleanup: () => void;
}> {
  await initWasm();
  const session = new EditorSessionHandle();
  markReadOnly(session, MOUNTED_PATH, MOUNTED_TEXT);
  const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
  const project = new ProjectSession({ provider, entryFile: "main.ink", session });
  await project.initialize();

  const documents = new DocumentSessions(project, {});
  const container = document.createElement("div");
  document.body.appendChild(container);
  return {
    documents,
    container,
    cleanup: () => {
      documents.dispose();
      container.remove();
    },
  };
}

describe("DocumentSessions: mounted-file views are genuinely read-only", () => {
  let harness: Awaited<ReturnType<typeof makeHarness>>;

  beforeEach(async () => {
    harness = await makeHarness();
  });

  afterEach(() => {
    harness.cleanup();
  });

  it("a mounted file's view sets EditorState.readOnly and disables EditorView.editable", () => {
    harness.documents.mountView(MOUNTED_PATH, "group-1", harness.container);
    const view = viewIn(harness.container);

    expect(view.state.readOnly).toBe(true);
    expect(view.state.facet(EditorView.editable)).toBe(false);
    // `EditorView.editable.of(false)` reflects onto the content DOM's
    // `contenteditable` attribute — the actual mechanism that stops a real
    // keystroke from landing in the document.
    expect(view.contentDOM.getAttribute("contenteditable")).toBe("false");
  });

  it("an ordinary project file's view stays fully editable", () => {
    harness.documents.mountView("main.ink", "group-1", harness.container);
    const view = viewIn(harness.container);

    expect(view.state.readOnly).toBe(false);
    expect(view.state.facet(EditorView.editable)).toBe(true);
    expect(view.contentDOM.getAttribute("contenteditable")).not.toBe("false");
  });

  it("a raw dispatch against a read-only view's state is not itself blocked (readOnly gates commands/keymaps, not dispatch) — the mounted copy still can't be forked because updateDocument refuses the write on the next wasm round-trip", () => {
    // This pins the mechanism boundary so a future reader doesn't assume
    // `EditorState.readOnly` alone stops `view.dispatch()` — it doesn't;
    // it only disables the built-in editing *commands* (`@codemirror/
    // commands`) that check `state.readOnly` before applying. The DOM-level
    // `contenteditable="false"` above is what stops real user keystrokes;
    // `DocHandle.pushSource` (issue #2306) is the second, wasm-level fence
    // for any change that reaches the CM state regardless.
    harness.documents.mountView(MOUNTED_PATH, "group-1", harness.container);
    const view = viewIn(harness.container);
    view.dispatch({ changes: { from: 0, insert: "X" } });
    expect(view.state.doc.toString().startsWith("X")).toBe(true);
  });
});
