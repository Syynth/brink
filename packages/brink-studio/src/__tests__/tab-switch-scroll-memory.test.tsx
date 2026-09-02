/**
 * Per-tab scroll memory (#3355): switching an editor tab away and back must
 * keep the author's place in a long file instead of resetting to the top.
 *
 * `EditorArea` mounts only the ACTIVE tab's document (`EditorGroupView`
 * renders `<Doc key={documentKey(tab.ref)} .../>`), so switching tabs swaps
 * the `key` on that one child slot: the deactivated tab's `InkFileDocument`
 * unmounts and the newly-active tab's mounts, in the SAME React commit.
 * `InkFileDocument`'s mount/unmount effect is what has to snapshot the
 * scroll position on the way out (via `DocumentSessions.mountView`'s
 * returned dispose, which calls `unmountSlot`) — and it must run that
 * snapshot while its container is STILL ATTACHED to the document, because a
 * plain `useEffect` cleanup for a component removed in a sibling-swap like
 * this fires only after the whole commit (including the DOM removal) has
 * already landed: React flushes passive-effect cleanups once per commit,
 * strictly after every mutation in it. By the time that cleanup runs, the
 * container is a detached node — a real browser reports degenerate (zero)
 * layout for it (confirmed live against a running studio dev build: setting
 * `scrollTop` then switching tabs and back landed back at the top every
 * time), which is exactly the bug report. `useLayoutEffect` cleanup, by
 * contrast, runs synchronously as part of the SAME commit that removes the
 * node, before it comes out of the tree.
 *
 * jsdom has no real layout engine, so it does not reproduce the *symptom*
 * (its `scrollTop` is just a stored number, indifferent to attachment) —
 * only the *mechanism* is checkable here: whether the container is still
 * attached when `DocumentSessions`'s real unmount snapshot runs. That is
 * exactly the fact the fix changes, so it is what this test pins down. The
 * existing `DocumentSessions` unit tests (`document-sessions.test.ts`) call
 * `mountView`'s returned dispose function directly from the test body —
 * that bypasses React's scheduler entirely, which is why they keep passing
 * even when the real, click-driven path is broken.
 */

import { describe, it, expect, afterEach } from "vitest";
import { act, createElement, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { InkFileDocument, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import { DocumentSessions, ProjectSession, InMemoryFileProvider } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function longFile(prefix: string, lineCount: number): string {
  return Array.from({ length: lineCount }, (_, i) => `${prefix} line ${i}`).join("\n");
}

const FILE_A = longFile("a", 300);
const FILE_B = longFile("b", 5);

let root: Root | null = null;
let container: HTMLDivElement | null = null;
let documents: DocumentSessions | null = null;
let store: ReturnType<typeof createStudioStore> | null = null;

afterEach(() => {
  if (root !== null) act(() => root!.unmount());
  documents?.dispose();
  container?.remove();
  root = null;
  container = null;
  documents = null;
  store = null;
});

/** Mirrors `EditorGroupView`'s single-active-tab slot: a different `key`
 *  swaps which document's `InkFileDocument` is mounted, in one commit —
 *  the exact shape a real tab click produces via `setActiveTab`. */
function tabSlot(active: "a" | "b"): ReactElement {
  const docId = active === "a" ? "a.ink" : "b.ink";
  return createElement(InkFileDocument, {
    key: docId,
    doc: { typeId: "ink-file", docId, title: docId },
    groupId: "group-1",
    active: true,
  });
}

function currentView(): EditorView {
  const dom = container!.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");
  return view;
}

async function setUp(instrument?: (docs: DocumentSessions) => void): Promise<void> {
  await initWasm();
  const provider = new InMemoryFileProvider({ "a.ink": FILE_A, "b.ink": FILE_B });
  const project = new ProjectSession({ provider, entryFile: "a.ink" });
  await project.initialize();
  documents = new DocumentSessions(project, {});
  // Must run before the first render below: InkFileDocument's mount effect
  // captures whichever `mountView` is current AT MOUNT TIME in its returned
  // dispose closure, so instrumenting after the first tab is already up
  // would only affect later mounts, never the one this test unmounts first.
  instrument?.(documents);

  store = createStudioStore();
  store.setState({ _documents: documents, _project: project as unknown as never });

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);

  act(() => {
    root!.render(createElement(StoreProvider, { store: store!, children: tabSlot("a") }));
  });
}

describe("per-tab scroll memory (#3355)", () => {
  it("snapshots the deactivated tab's view while its container is still attached", async () => {
    // Instrument the real DocumentSessions instance InkFileDocument talks
    // to: record whether `container` — the DOM node `unmountSlot`'s
    // snapshot reads through (`slot.view.scrollDOM` lives inside it) — is
    // still in the document at the moment mountView's real dispose runs.
    const attachedAtSnapshot: boolean[] = [];
    await setUp((docs) => {
      const realMountView = docs.mountView.bind(docs);
      docs.mountView = (docKey, groupId, mountContainer) => {
        const dispose = realMountView(docKey, groupId, mountContainer);
        return () => {
          attachedAtSnapshot.push(document.body.contains(mountContainer));
          dispose();
        };
      };
    });

    // Switch away from a.ink (unmounts it) to b.ink (mounts it) — the exact
    // commit shape a tab click produces.
    act(() => {
      root!.render(createElement(StoreProvider, { store: store!, children: tabSlot("b") }));
    });

    expect(attachedAtSnapshot).toEqual([true]);
  });

  it("restores the first visible line — not the top — after switching back", async () => {
    await setUp();

    act(() => {
      currentView().scrollDOM.scrollTop = 2000;
    });
    const scrollTopBefore = currentView().scrollDOM.scrollTop;
    expect(scrollTopBefore).toBeGreaterThan(0);
    const lineBefore = currentView().lineBlockAtHeight(scrollTopBefore).from;

    act(() => {
      root!.render(createElement(StoreProvider, { store: store!, children: tabSlot("b") }));
    });
    expect(currentView().state.doc.toString()).toBe(FILE_B);

    act(() => {
      root!.render(createElement(StoreProvider, { store: store!, children: tabSlot("a") }));
    });

    const restored = currentView();
    expect(restored.state.doc.toString()).toBe(FILE_A);
    expect(restored.scrollDOM.scrollTop).toBeGreaterThan(0);
    const lineAfter = restored.lineBlockAtHeight(restored.scrollDOM.scrollTop).from;
    expect(Math.abs(lineAfter - lineBefore)).toBeLessThan(FILE_A.length / 20);
  });
});
