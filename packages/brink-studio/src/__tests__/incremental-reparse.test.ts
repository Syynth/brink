/**
 * Per-keystroke wasm-traffic budget (#14). The editor used to marshal the full
 * document across the wasm boundary several times per keystroke — element-type
 * pushed the source, then the decoration/query providers each re-pushed it, and
 * each push's no-op guard pulled the whole source back out of wasm just to
 * compare. This test mounts a real view, types one character, and asserts the
 * de-duplicated budget: exactly one `updateDocument`, and zero `getViewSourceDoc`
 * round-trips.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { DocumentSessions, InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm, type EditorSessionHandle } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

const MAIN = "-> start\n=== start ===\nHello world.\n-> END\n";

interface Harness {
  project: ProjectSession;
  documents: DocumentSessions;
  view: EditorView;
  container: HTMLElement;
  session: EditorSessionHandle;
  dispose: () => void;
}

async function mount(): Promise<Harness> {
  await initWasm();
  const provider = new InMemoryFileProvider({ "main.ink": MAIN });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const documents = new DocumentSessions(project);
  const container = document.createElement("div");
  document.body.appendChild(container);
  const dispose = documents.mountView("main.ink", "g1", container);
  documents.setFocused("main.ink", "g1");
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return { project, documents, view, container, session: project.getSession(), dispose };
}

describe("per-keystroke wasm traffic (#14)", () => {
  let h: Harness;

  beforeEach(async () => {
    h = await mount();
  });

  afterEach(() => {
    h.dispose();
    h.container.remove();
  });

  it("pushes the source once and never round-trips it back out per keystroke", () => {
    // Spy AFTER mount so the initial mount's pushes don't count.
    const updateDocument = vi.spyOn(h.session, "updateDocument");
    const getViewSource = vi.spyOn(h.session, "getViewSourceDoc");

    // One user keystroke: insert a character at the cursor.
    h.view.dispatch({ changes: { from: 0, insert: "x" } });

    // Exactly one push of the new source; the no-op guard does not pull the
    // whole source back out of wasm to decide "changed".
    expect(updateDocument).toHaveBeenCalledTimes(1);
    expect(getViewSource).toHaveBeenCalledTimes(0);
  });

  it("does not re-push when the doc is unchanged (idempotent edits)", () => {
    h.view.dispatch({ changes: { from: 0, insert: "x" } });
    const updateDocument = vi.spyOn(h.session, "updateDocument");
    // A selection-only transaction (no doc change) must push nothing.
    h.view.dispatch({ selection: { anchor: 1 } });
    expect(updateDocument).toHaveBeenCalledTimes(0);
  });
});
