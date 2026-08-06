/**
 * External-change view sync (#320's clean path, completed by the D2
 * watcher work): `ProjectSession`'s external handler updates the wasm
 * session, but a MOUNTED view kept its stale text — and the stale view's
 * next flush would silently revert the external update (the reverse of
 * the clobber #320 fixed). Found live by brink-desktop's fs watcher, the
 * clean path's first real producer: the session and Player showed the
 * external text while the visible editor didn't.
 *
 * `DocumentSessions.refreshExternal` is the repair; `mountStudio` wires it
 * through the previously-consumer-less `onExternalFileChange` hook. The
 * wire itself is exercised by the desktop app live-drive; this pins the
 * mechanism.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  DocumentSessions,
  InMemoryFileProvider,
  ProjectSession,
} from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";

const MAIN_INK = "-> start\n=== start ===\nHello apple.\n-> END\n";
const EXTERNAL = "-> start\n=== start ===\nHello EXTERNAL pear.\n-> END\n";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("refreshExternal", () => {
  it("re-syncs a mounted stale view from the session without creating an edit", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();
    const documents = new DocumentSessions(project);

    const container = document.createElement("div");
    document.body.appendChild(container);
    const dispose = documents.mountView("main.ink", "g1", container);
    const dom = container.querySelector(".cm-editor");
    const view = dom && (await import("@codemirror/view")).EditorView.findFromDOM(dom as HTMLElement);
    if (!view) throw new Error("no editor mounted");

    // The external change lands in the SESSION (clean buffer ⇒ no conflict)…
    provider.pushExternalChange("main.ink", EXTERNAL);
    expect(project.getSession().getFileSource("main.ink")).toBe(EXTERNAL);
    // …but the mounted view is stale — the bug shape this test documents.
    expect(view.state.doc.toString()).toBe(MAIN_INK);

    documents.refreshExternal("main.ink");
    expect(view.state.doc.toString()).toBe(EXTERNAL);

    // The re-sync is sync-annotated: it must not register as a user edit —
    // no dirty state, nothing queued for the egress.
    expect(project.dirtyPaths()).toEqual([]);

    dispose();
    container.remove();
  });

  it("clears cached-only slots so an unmounted tab rebuilds from the session", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();
    const documents = new DocumentSessions(project);

    const container = document.createElement("div");
    document.body.appendChild(container);
    const dispose = documents.mountView("main.ink", "g1", container);
    dispose(); // unmount: the slot keeps a cached EditorState
    container.remove();

    provider.pushExternalChange("main.ink", EXTERNAL);
    documents.refreshExternal("main.ink");

    // Remount: the view must show the external content, not the cached state.
    const container2 = document.createElement("div");
    document.body.appendChild(container2);
    const dispose2 = documents.mountView("main.ink", "g1", container2);
    const dom2 = container2.querySelector(".cm-editor");
    const view2 =
      dom2 && (await import("@codemirror/view")).EditorView.findFromDOM(dom2 as HTMLElement);
    if (!view2) throw new Error("no editor remounted");
    expect(view2.state.doc.toString()).toBe(EXTERNAL);

    dispose2();
    container2.remove();
  });
});
