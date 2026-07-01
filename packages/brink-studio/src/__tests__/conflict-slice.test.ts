/**
 * Conflict slice tests (issue #320, Track V).
 *
 * The slice mirrors B1 conflicts into reactive state for the merge view and
 * routes the three resolutions back through the bound ProjectSession. These
 * tests prove: a recorded conflict shows up in `conflicts`/`conflictPaths`,
 * and each resolve action calls the matching ProjectSession method AND drops
 * the conflict from the slice (so the surface tears down). A stale or
 * unknown-path resolution is a no-op.
 */

import { describe, it, expect, vi } from "vitest";
import { createStudioStore, conflictPaths, type ProjectSession } from "@brink/studio-store";
import {
  DocumentSessions,
  InMemoryFileProvider,
  ProjectSession as RealProjectSession,
  type FileConflict,
} from "@brink/ink-editor";
import { initWasm } from "@brink-lang/web";
import { EditorView } from "@codemirror/view";

const CONFLICT: FileConflict = {
  path: "main.ink",
  disk: "host edit",
  buffer: "studio edit",
  baseline: "original",
};

/** A ProjectSession test double recording the resolve calls. */
function stubProject() {
  return {
    resolveConflictUseDisk: vi.fn(),
    resolveConflictKeepMine: vi.fn(),
    resolveConflictMerged: vi.fn(),
  } as unknown as ProjectSession & {
    resolveConflictUseDisk: ReturnType<typeof vi.fn>;
    resolveConflictKeepMine: ReturnType<typeof vi.fn>;
    resolveConflictMerged: ReturnType<typeof vi.fn>;
  };
}

function makeStore() {
  const store = createStudioStore();
  const project = stubProject();
  store.setState({ _project: project });
  return { store, project };
}

describe("conflict slice", () => {
  it("starts with no conflicts", () => {
    const { store } = makeStore();
    expect(store.getState().conflicts).toEqual({});
    expect(conflictPaths(store.getState().conflicts)).toEqual([]);
  });

  it("setConflict records a conflict keyed by path", () => {
    const { store } = makeStore();
    store.getState().setConflict(CONFLICT);
    expect(store.getState().conflicts).toEqual({ "main.ink": CONFLICT });
    expect(conflictPaths(store.getState().conflicts)).toEqual(["main.ink"]);
  });

  it("setConflict replaces a prior conflict for the same path", () => {
    const { store } = makeStore();
    store.getState().setConflict(CONFLICT);
    const next: FileConflict = { ...CONFLICT, disk: "host edit 2" };
    store.getState().setConflict(next);
    expect(store.getState().conflicts).toEqual({ "main.ink": next });
  });

  it("conflictPaths is sorted (deterministic)", () => {
    const { store } = makeStore();
    store.getState().setConflict({ ...CONFLICT, path: "z.ink" });
    store.getState().setConflict({ ...CONFLICT, path: "a.ink" });
    expect(conflictPaths(store.getState().conflicts)).toEqual(["a.ink", "z.ink"]);
  });

  it("resolveUseDisk calls the project with the disk text and drops the conflict", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().resolveUseDisk("main.ink");
    expect(project.resolveConflictUseDisk).toHaveBeenCalledWith("main.ink", "host edit");
    expect(store.getState().conflicts).toEqual({});
  });

  it("resolveKeepMine calls the project and drops the conflict", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().resolveKeepMine("main.ink");
    expect(project.resolveConflictKeepMine).toHaveBeenCalledWith("main.ink");
    expect(store.getState().conflicts).toEqual({});
  });

  it("resolveMerge calls the project with the merged text and drops the conflict", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().resolveMerge("main.ink", "merged result");
    expect(project.resolveConflictMerged).toHaveBeenCalledWith("main.ink", "merged result");
    expect(store.getState().conflicts).toEqual({});
  });

  it("clearConflict drops a conflict without resolving it", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().clearConflict("main.ink");
    expect(store.getState().conflicts).toEqual({});
    expect(project.resolveConflictKeepMine).not.toHaveBeenCalled();
    expect(project.resolveConflictUseDisk).not.toHaveBeenCalled();
  });

  it("resolving an unknown path is a no-op", () => {
    const { store, project } = makeStore();
    store.getState().resolveUseDisk("nope.ink");
    expect(project.resolveConflictUseDisk).not.toHaveBeenCalled();
    expect(store.getState().conflicts).toEqual({});
  });
});

// ── Mounted-view re-sync after a content-mutating resolution ─────────
//
// The stubbed tests above only prove the slice routes the resolve to the
// ProjectSession. They CANNOT catch the silent-data-loss bug: a resolution
// that mutates the wasm session out-of-band (Use disk / Apply merge) must
// also `documents.invalidateFile(path)` so every MOUNTED CM6 view re-syncs
// from the session — otherwise the open editor keeps showing the stale
// pre-resolution buffer (and the next keystroke clobbers the resolve).
//
// These wire a REAL ProjectSession + DocumentSessions + a mounted EditorView
// through the store and assert the visible document after each resolution —
// the locked verify step "Use disk → buffer becomes the on-disk text".

const MAIN_INK = "-> start\n=== start ===\nHello.\n-> END\n";

async function makeMountedConflict(): Promise<{
  store: ReturnType<typeof createStudioStore>;
  documents: DocumentSessions;
  project: RealProjectSession;
  view: EditorView;
  container: HTMLElement;
  dispose: () => void;
}> {
  await initWasm();
  const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
  const project = new RealProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();

  const documents = new DocumentSessions(project);
  const store = createStudioStore();
  store.getState().initialize(project, documents);

  // Mount a live CM6 view for main.ink, the way the shell does.
  const container = document.createElement("div");
  document.body.appendChild(container);
  const dispose = documents.mountView("main.ink", "g1", container);
  documents.setFocused("main.ink", "g1");
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
  if (!view) throw new Error("no editor mounted");

  // The studio has an unsaved, divergent edit, and the host rewrote the file
  // on disk: a standing conflict. The view shows the studio buffer.
  project.applyEdit("main.ink", "studio edit");
  documents.invalidateFile("main.ink");
  expect(view.state.doc.toString()).toBe("studio edit");

  store.getState().setConflict({
    path: "main.ink",
    disk: "host edit",
    buffer: "studio edit",
    baseline: MAIN_INK,
  });

  return { store, documents, project, view, container, dispose };
}

describe("conflict resolution re-syncs the mounted editor (#320, B2)", () => {
  it("resolveUseDisk: the open editor shows the on-disk text", async () => {
    const { store, project, view, container, dispose } = await makeMountedConflict();

    store.getState().resolveUseDisk("main.ink");

    // The session re-baselined to disk AND the mounted view re-synced. Without
    // the invalidateFile/triggerCompile pairing the view would still read
    // "studio edit" — the silent data-loss this test guards against.
    expect(project.getSession().getFileSource("main.ink")).toBe("host edit");
    expect(view.state.doc.toString()).toBe("host edit");
    expect(store.getState().conflicts).toEqual({});

    dispose();
    container.remove();
  });

  it("resolveMerge: the open editor shows the merged text", async () => {
    const { store, project, view, container, dispose } = await makeMountedConflict();

    store.getState().resolveMerge("main.ink", "studio edit + host edit");

    expect(project.getSession().getFileSource("main.ink")).toBe("studio edit + host edit");
    expect(view.state.doc.toString()).toBe("studio edit + host edit");
    expect(store.getState().conflicts).toEqual({});

    dispose();
    container.remove();
  });

  it("resolveKeepMine: the open editor keeps the studio buffer (no re-sync needed)", async () => {
    const { store, view, container, dispose } = await makeMountedConflict();

    store.getState().resolveKeepMine("main.ink");

    // Keep-mine never mutates session content, so the view is untouched.
    expect(view.state.doc.toString()).toBe("studio edit");
    expect(store.getState().conflicts).toEqual({});

    dispose();
    container.remove();
  });
});
