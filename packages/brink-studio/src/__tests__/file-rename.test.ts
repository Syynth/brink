/**
 * File-rename/move tests (#164 Stage 3, PR B): ProjectSession.renameFile
 * (session re-key + provider rename/fallback + INCLUDE-referrer rewrite +
 * created/deleted/modified egress) and the store's renameFile / moveFile
 * actions with inverse-rename undo.
 *
 * Runs against the brink-web mock, whose `rename_file` returns a MoveResult
 * with the moved content verbatim plus a cross_file_edit for any file whose
 * INCLUDE names the old basename — enough to exercise the apply/egress
 * plumbing. (The real inbound/outbound INCLUDE math is covered by Rust unit
 * tests in brink-ide.)
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession, type FileChange } from "@brink/ink-editor";
import { initWasm } from "@brink-lang/web";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";

const MAIN = "INCLUDE lib.ink\n-> start\n=== start ===\nHello.\n-> END\n";
const MAIN_RENAMED = "INCLUDE util.ink\n-> start\n=== start ===\nHello.\n-> END\n";
const LIB = "=== helper ===\n-> END\n";

interface Egress {
  provider: InMemoryFileProvider;
  project: ProjectSession;
  batches: FileChange[][];
}

async function makeProject(
  files: Record<string, string> = { "main.ink": MAIN, "lib.ink": LIB },
): Promise<Egress> {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const batches: FileChange[][] = [];
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    onFilesChanged: (changes) => batches.push(changes),
  });
  await project.initialize();
  return { provider, project, batches };
}

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

// ── ProjectSession.renameFile ───────────────────────────────────────

describe("ProjectSession.renameFile", () => {
  it("re-keys the file, rewrites referrers, and emits created/deleted/modified", async () => {
    const { provider, project, batches } = await makeProject();

    const referrers = await project.renameFile("lib.ink", "util.ink");
    expect(referrers).toEqual(["main.ink"]);

    const session = project.getSession();
    expect(session.getFileSource("util.ink")).toBe(LIB);
    expect(session.getFileSource("lib.ink")).toBeNull();
    expect(project.getFiles()["main.ink"]).toBe(MAIN_RENAMED);
    expect(await provider.requestFile("util.ink")).toBe(LIB);
    expect(await provider.requestFile("lib.ink")).toBeNull();

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([
      [
        { path: "lib.ink", type: "deleted" },
        { path: "main.ink", type: "modified", content: MAIN_RENAMED },
        { path: "util.ink", type: "created", content: LIB },
      ],
    ]);
  });

  it("falls back to create+delete when the provider lacks renameFile", async () => {
    await initWasm();
    const provider = new InMemoryFileProvider({ "main.ink": MAIN, "lib.ink": LIB });
    (provider as { renameFile?: unknown }).renameFile = undefined;
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();

    await project.renameFile("lib.ink", "util.ink");
    expect(await provider.requestFile("util.ink")).toBe(LIB);
    expect(await provider.requestFile("lib.ink")).toBeNull();
  });

  it("canRenameFiles reflects provider capability", async () => {
    const { project } = await makeProject();
    expect(project.canRenameFiles()).toBe(true);
  });

  it("throws when the destination already exists", async () => {
    const { project } = await makeProject();
    await expect(project.renameFile("lib.ink", "main.ink")).rejects.toThrow();
  });
});

// ── Store renameFile / moveFile + undo ──────────────────────────────

describe("store.renameFile", () => {
  it("re-keys the open tabs in place, renames, and undo restores the original path + includes", async () => {
    const { project } = await makeProject();
    const renameDoc = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _renameDocPath: renameDoc,
    });

    await store.getState().renameFile("lib.ink", "util.ink");

    expect(renameDoc).toHaveBeenCalledWith("lib.ink", "util.ink");
    const session = project.getSession();
    expect(session.getFileSource("util.ink")).toBe(LIB);
    expect(session.getFileSource("lib.ink")).toBeNull();
    expect(project.getFiles()["main.ink"]).toBe(MAIN_RENAMED);
    expect(store.getState().undoStack).toHaveLength(1);
    expect(store.getState().undoStack[0]!.kind).toBe("rename");

    // Undo is the inverse rename — file back at lib.ink, INCLUDE restored.
    await store.getState().undo();
    expect(session.getFileSource("lib.ink")).toBe(LIB);
    expect(session.getFileSource("util.ink")).toBeNull();
    expect(project.getFiles()["main.ink"]).toBe(MAIN);
    expect(store.getState().undoStack).toHaveLength(0);
  });

  it("a failed rename (name collision) leaves the open tabs and file intact", async () => {
    const { project } = await makeProject();
    const renameDoc = vi.fn();
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _renameDocPath: renameDoc,
      _notify: notify,
    });

    // lib.ink → main.ink collides; the rename must fail without side effects.
    await store.getState().renameFile("lib.ink", "main.ink");

    expect(renameDoc).not.toHaveBeenCalled(); // tabs untouched
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ severity: "error" }));
    expect(store.getState().undoStack).toHaveLength(0);
    const session = project.getSession();
    expect(session.getFileSource("lib.ink")).toBe(LIB); // file still there
    expect(project.getFiles()["main.ink"]).toBe(MAIN); // referrer untouched
  });

  it("moveFile relocates into a folder and round-trips on undo", async () => {
    const { project } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    await store.getState().moveFile("lib.ink", "scenes/lib.ink");
    const session = project.getSession();
    expect(session.getFileSource("scenes/lib.ink")).toBe(LIB);
    expect(session.getFileSource("lib.ink")).toBeNull();
    // (Directory-aware INCLUDE rewriting is verified by the Rust op tests; the
    // mock only rewrites by basename, so the move's referrer edit is a no-op
    // here — this test covers the session re-key + undo round-trip.)

    await store.getState().undo();
    expect(session.getFileSource("lib.ink")).toBe(LIB);
    expect(session.getFileSource("scenes/lib.ink")).toBeNull();
    expect(store.getState().undoStack).toHaveLength(0);
  });
});
