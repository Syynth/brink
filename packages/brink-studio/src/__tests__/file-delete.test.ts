/**
 * File-delete tests (#164 Stage 2): ProjectSession.deleteFile (provider +
 * session eviction + a "deleted" egress change), the store's deleteFile /
 * deleteFolder actions (snapshot → close tabs → delete → recompile), and the
 * recreate-undo path that restores a deleted file as a *created* change (the
 * host has no such path anymore, so it must not egress as "modified").
 *
 * Runs against the brink-web mock (src/__mocks__/brink-web.ts), whose
 * EditorSession supports update_file / remove_file / get_file_source /
 * list_files — enough to exercise the full delete + undo round-trip. (The
 * dangling-INCLUDE diagnostic a delete leaves behind needs real compilation;
 * that is covered by live verification, not this layer.)
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession, type FileChange } from "@brink-lang/editor";
import { EditorSessionHandle, initWasm } from "@brink-lang/web";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";

const MAIN_INK = "-> start\n=== start ===\nHello apple.\n-> END\n";
const SIDE_INK = "=== side ===\nAnother apple here.\n-> END\n";
const MOUNTED_PATH = "std/conventions/screenplay.brink";
const MOUNTED_TEXT = "=== screenplay ===\n-> DONE\n";

interface Egress {
  provider: InMemoryFileProvider;
  project: ProjectSession;
  batches: FileChange[][];
}

async function makeProject(
  files: Record<string, string> = { "main.ink": MAIN_INK, "side.ink": SIDE_INK },
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

/** Reach into the mock's raw session to seed a read-only file — the
 *  wasm-boundary equivalent of the real constructor's stdlib mount (mirrors
 *  `mounted-stdlib-readonly.test.ts`'s `markReadOnly` helper). */
function markReadOnly(handle: EditorSessionHandle, path: string, source: string): void {
  (
    handle as unknown as {
      session: { __mockMarkReadOnlyForTest(path: string, source: string): void };
    }
  ).session.__mockMarkReadOnlyForTest(path, source);
}

/** Like {@link makeProject}, but with `MOUNTED_PATH` pre-seeded as a
 *  mounted/read-only file, for the delete-refusal tests below. */
async function makeProjectWithMount(): Promise<Egress> {
  await initWasm();
  const session = new EditorSessionHandle();
  markReadOnly(session, MOUNTED_PATH, MOUNTED_TEXT);
  const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK, "side.ink": SIDE_INK });
  const batches: FileChange[][] = [];
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    session,
    onFilesChanged: (changes) => batches.push(changes),
  });
  await project.initialize();
  return { provider, project, batches };
}

/** Stub the per-view machinery the store delete path calls. */
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

// ── ProjectSession.deleteFile ───────────────────────────────────────

describe("ProjectSession.deleteFile", () => {
  it("removes the file from the provider and session and emits a deleted change", async () => {
    const { provider, project, batches } = await makeProject();
    const providerDelete = vi.spyOn(provider, "deleteFile");

    await project.deleteFile("side.ink");

    expect(providerDelete).toHaveBeenCalledWith("side.ink");
    expect(await provider.requestFile("side.ink")).toBeNull();
    expect(project.getSession().getFileSource("side.ink")).toBeNull();
    expect(Object.keys(project.getFiles())).toEqual(["main.ink"]);

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([[{ path: "side.ink", type: "deleted" }]]);
  });

  it("canDeleteFiles reflects provider support", async () => {
    const { project } = await makeProject();
    expect(project.canDeleteFiles()).toBe(true);
  });

  it("a provider without deleteFile still evicts from the session (no throw)", async () => {
    await initWasm();
    // A provider that omits the optional deleteFile capability.
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK, "side.ink": SIDE_INK });
    (provider as { deleteFile?: unknown }).deleteFile = undefined;
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();

    expect(project.canDeleteFiles()).toBe(false);
    await expect(project.deleteFile("side.ink")).resolves.toBe(true);
    expect(project.getSession().getFileSource("side.ink")).toBeNull();
  });

  it("refuses a mounted stdlib path: no provider write, no session mutation, returns false (issue #2343 review finding)", async () => {
    const { provider, project, batches } = await makeProjectWithMount();
    const providerDelete = vi.spyOn(provider, "deleteFile");

    expect(project.isReadOnly(MOUNTED_PATH)).toBe(true);
    await expect(project.deleteFile(MOUNTED_PATH)).resolves.toBe(false);

    expect(providerDelete).not.toHaveBeenCalled();
    expect(project.getSession().getFileSource(MOUNTED_PATH)).not.toBeNull();
    vi.advanceTimersByTime(500);
    expect(batches).toEqual([]);
  });
});

// ── Store deleteFile + recreate-undo ────────────────────────────────

describe("store.deleteFile", () => {
  it("closes the file's tabs, deletes it, and emits a deleted change", async () => {
    const { project, batches } = await makeProject();
    const closeDocs = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _closeDocsForPath: closeDocs,
    });

    await store.getState().deleteFile("side.ink");

    expect(closeDocs).toHaveBeenCalledWith("side.ink");
    expect(Object.keys(project.getFiles())).toEqual(["main.ink"]);
    expect(store.getState().undoStack).toHaveLength(1);

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([[{ path: "side.ink", type: "deleted" }]]);
  });

  it("undo re-creates the deleted file as a created change with its content", async () => {
    const { project, batches } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    await store.getState().deleteFile("side.ink");
    vi.advanceTimersByTime(500);

    await store.getState().undo();
    vi.advanceTimersByTime(500);

    expect(project.getSession().getFileSource("side.ink")).toBe(SIDE_INK);
    expect(store.getState().undoStack).toHaveLength(0);
    expect(batches).toEqual([
      [{ path: "side.ink", type: "deleted" }],
      [{ path: "side.ink", type: "created", content: SIDE_INK }],
    ]);
  });

  it("a vanished file is a no-op (no undo entry, no egress)", async () => {
    const { project, batches } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    await store.getState().deleteFile("ghost.ink");
    vi.advanceTimersByTime(500);

    expect(store.getState().undoStack).toHaveLength(0);
    expect(batches).toEqual([]);
  });

  it("a mounted stdlib path is skipped before any tab is closed, with a warning notification (issue #2343 review finding)", async () => {
    const { project, batches } = await makeProjectWithMount();
    const closeDocs = vi.fn();
    const notifications: unknown[] = [];
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _closeDocsForPath: closeDocs,
      _notify: (n) => notifications.push(n),
    });

    await store.getState().deleteFile(MOUNTED_PATH);
    vi.advanceTimersByTime(500);

    // The tab must never be closed for a file whose delete gets refused —
    // closing it is itself destructive to the user's view state.
    expect(closeDocs).not.toHaveBeenCalled();
    expect(project.getSession().getFileSource(MOUNTED_PATH)).not.toBeNull();
    expect(store.getState().undoStack).toHaveLength(0);
    expect(batches).toEqual([]);
    expect(notifications).toEqual([
      expect.objectContaining({ severity: "warning", source: "binder" }),
    ]);
  });
});

// ── Store deleteFolder (batch) ──────────────────────────────────────

describe("store.deleteFolder", () => {
  it("deletes every file under the prefix in one undoable batch", async () => {
    const { project, batches } = await makeProject({
      "main.ink": MAIN_INK,
      "scenes/a.ink": "=== a ===\n-> END\n",
      "scenes/b.ink": "=== b ===\n-> END\n",
    });
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    await store.getState().deleteFolder("scenes/", ["scenes/a.ink", "scenes/b.ink"]);

    expect(Object.keys(project.getFiles())).toEqual(["main.ink"]);
    expect(store.getState().undoStack).toHaveLength(1);

    vi.advanceTimersByTime(500);
    // Sorted batch — both files, one delete each.
    expect(batches).toEqual([
      [
        { path: "scenes/a.ink", type: "deleted" },
        { path: "scenes/b.ink", type: "deleted" },
      ],
    ]);

    // One undo restores both, as created.
    await store.getState().undo();
    vi.advanceTimersByTime(500);
    expect(Object.keys(project.getFiles()).sort()).toEqual([
      "main.ink",
      "scenes/a.ink",
      "scenes/b.ink",
    ]);
    expect(store.getState().undoStack).toHaveLength(0);
  });
});
