/**
 * Folder-rename tests (issue #2587): the Binder's `renameFolder` action wired
 * to the atomic `rename_dir` op (#314) instead of a per-file `renameFile`
 * loop.
 *
 * ## The bug this file's first describe block proves (RED before the fix)
 *
 * #314 built `rename_dir` specifically because a per-file rename loop cannot
 * give the single pre-move snapshot needed to keep inbound `INCLUDE`s (from
 * files outside the moved folder) and intra-folder sibling `INCLUDE`s
 * mutually consistent during a directory move. The studio mock makes this
 * concretely observable: `rename_file`'s cross-file-edit rewrite matches on
 * *basename* only (`crates/brink-web` mock, `__mocks__/brink-web.ts`'s
 * `rename_file`), so a folder move that changes only the directory PREFIX —
 * every moved file keeps its own basename — produces zero cross-file edits
 * under the per-file loop: each `rename_file` call sees `oldBase === newBase`
 * and no-ops. `rename_dir`'s mock, by contrast, rewrites inbound `INCLUDE`s
 * by *prefix* (`oldPrefix/` → `newPrefix/`), which is what the real Rust op
 * actually does. So a per-file-loop `renameFolder` leaves an outside
 * referrer's `INCLUDE` pointing at the OLD (now-nonexistent) path — a
 * dangling include the atomic op does not produce.
 *
 * This is not a mock artifact: it is the exact defect class #314 exists to
 * prevent, made observable by the mock's already-established (and
 * independently useful) basename-vs-prefix distinction between its two
 * rename ops.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession, type FileChange } from "@brink-lang/editor";
import { EditorSessionHandle, initWasm } from "@brink-lang/web";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";

const MAIN = "INCLUDE chapters/a.ink\n-> start\n=== start ===\nHello.\n-> END\n";
const A = "=== sceneA ===\n-> END\n";
const B = "=== sceneB ===\n-> END\n";

interface Egress {
  provider: InMemoryFileProvider;
  project: ProjectSession;
  batches: FileChange[][];
}

async function makeProject(
  files: Record<string, string> = {
    "main.ink": MAIN,
    "chapters/a.ink": A,
    "chapters/b.ink": B,
  },
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

/** See `file-rename.test.ts`'s `settleRename` — both `renameFile` and
 *  `renameDir` are deferred off the paint path via `scheduleIdleWork`. */
async function settleAll<T>(pending: Promise<T>): Promise<T> {
  pending.catch(() => {});
  await vi.runAllTimersAsync();
  return pending;
}

/** Reach into the mock's raw session to seed a read-only file — the
 *  wasm-boundary equivalent of the real constructor's stdlib mount (mirrors
 *  `file-delete.test.ts`'s `markReadOnly` helper). */
function markReadOnly(handle: EditorSessionHandle, path: string, source: string): void {
  (
    handle as unknown as {
      session: { __mockMarkReadOnlyForTest(path: string, source: string): void };
    }
  ).session.__mockMarkReadOnlyForTest(path, source);
}

describe("store.renameFolder produces INCLUDE-consistent state for a same-basename folder move (#2587)", () => {
  it("rewrites an outside referrer's INCLUDE by prefix, not just basename", async () => {
    const { project } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const session = project.getSession();

    await settleAll(
      store
        .getState()
        .renameFolder("chapters/", "acts/", ["chapters/a.ink", "chapters/b.ink"]),
    );

    // Both files actually moved.
    expect(session.getFileSource("acts/a.ink")).toBe(A);
    expect(session.getFileSource("acts/b.ink")).toBe(B);
    expect(session.getFileSource("chapters/a.ink")).toBeNull();
    expect(session.getFileSource("chapters/b.ink")).toBeNull();

    // The outside referrer's INCLUDE must follow the move — this is the
    // exact guarantee #314 built `rename_dir` for. A per-file loop (basename
    // matching only) leaves this line naming a path that no longer exists.
    expect(project.getFiles()["main.ink"]).toBe(
      "INCLUDE acts/a.ink\n-> start\n=== start ===\nHello.\n-> END\n",
    );
  });

  it("pushes one undoable step that restores the original prefix and the referrer's INCLUDE", async () => {
    const { project } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const session = project.getSession();

    await settleAll(
      store
        .getState()
        .renameFolder("chapters/", "acts/", ["chapters/a.ink", "chapters/b.ink"]),
    );
    expect(store.getState().undoStack).toHaveLength(1);

    await settleAll(store.getState().undo());

    expect(session.getFileSource("chapters/a.ink")).toBe(A);
    expect(session.getFileSource("chapters/b.ink")).toBe(B);
    expect(session.getFileSource("acts/a.ink")).toBeNull();
    expect(session.getFileSource("acts/b.ink")).toBeNull();
    expect(project.getFiles()["main.ink"]).toBe(MAIN);
    expect(store.getState().undoStack).toHaveLength(0);
  });

  it("re-keys open tabs for every moved file", async () => {
    const { project } = await makeProject();
    const renameDoc = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _renameDocPath: renameDoc,
    });

    await settleAll(
      store
        .getState()
        .renameFolder("chapters/", "acts/", ["chapters/a.ink", "chapters/b.ink"]),
    );

    expect(renameDoc).toHaveBeenCalledWith("chapters/a.ink", "acts/a.ink");
    expect(renameDoc).toHaveBeenCalledWith("chapters/b.ink", "acts/b.ink");
  });

  it("a refused move (destination collision) leaves every file untouched and pushes no undo entry", async () => {
    const { project } = await makeProject({
      "main.ink": MAIN,
      "chapters/a.ink": A,
      "chapters/b.ink": B,
      "acts/a.ink": "=== existing ===\n-> END\n",
    });
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments(), _notify: notify });
    const session = project.getSession();

    await settleAll(
      store
        .getState()
        .renameFolder("chapters/", "acts/", ["chapters/a.ink", "chapters/b.ink"]),
    );

    // All-or-nothing (#2587): the collision on a.ink refuses the WHOLE move,
    // not just that one file — b.ink (which would have moved cleanly) stays
    // put too, unlike the old per-file loop's silently-skip-and-continue
    // semantics. A partial move here would reintroduce the exact
    // inconsistency #314 exists to prevent.
    expect(session.getFileSource("chapters/a.ink")).toBe(A);
    expect(session.getFileSource("chapters/b.ink")).toBe(B);
    expect(session.getFileSource("acts/b.ink")).toBeNull();
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ severity: "error" }));
    expect(store.getState().undoStack).toHaveLength(0);
  });

  it("does nothing for an empty folder selection (no wasm call, no notification)", async () => {
    const { project } = await makeProject();
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments(), _notify: notify });

    await settleAll(store.getState().renameFolder("ghost/", "other/", []));

    expect(notify).not.toHaveBeenCalled();
    expect(store.getState().undoStack).toHaveLength(0);
  });
});

describe("store.renameFolder runs off the paint path (#2587, mirroring #2776)", () => {
  it("commits the pending busy-state synchronously, before the deferred wasm call runs", async () => {
    const { project } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    const pending = store
      .getState()
      .renameFolder("chapters/", "acts/", ["chapters/a.ink", "chapters/b.ink"]);

    expect(store.getState().structuralOpPending).not.toBeNull();
    // The heavy call has NOT run yet: nothing has moved.
    expect(project.getSession().getFileSource("chapters/a.ink")).toBe(A);
    expect(project.getSession().getFileSource("acts/a.ink")).toBeNull();

    await settleAll(pending);

    expect(project.getSession().getFileSource("acts/a.ink")).toBe(A);
    expect(store.getState().structuralOpPending).toBeNull();
  });
});

describe("ProjectSession.renameDir refuses to sweep a mounted/read-only file into the move (#2916 review)", () => {
  // `rename_dir` discovers its own file set from the db (every file under
  // `oldPrefix`) rather than from a caller-filtered list, and — unlike
  // `rename_file` — neither the real Rust op nor the wasm binding checks
  // whether any of them is a mounted stdlib copy. A project with its own
  // `std/` folder could otherwise have a folder move fork a mounted copy
  // into the project. The fence lives entirely on the TS side
  // (`ProjectSession.renameDir`), so this exercises it directly, then
  // through the store.

  it("throws before any session mutation", async () => {
    await initWasm();
    const session = new EditorSessionHandle();
    markReadOnly(session, "chapters/mounted.brink", "=== mounted ===\n-> END\n");
    const provider = new InMemoryFileProvider({
      "main.ink": MAIN,
      "chapters/a.ink": A,
      "chapters/b.ink": B,
    });
    const project = new ProjectSession({ provider, entryFile: "main.ink", session });
    await project.initialize();

    const pending = project.renameDir("chapters", "acts");
    await expect(settleAll(pending)).rejects.toThrow(/read-only/);

    expect(project.getSession().getFileSource("chapters/a.ink")).toBe(A);
    expect(project.getSession().getFileSource("chapters/b.ink")).toBe(B);
    expect(project.getSession().getFileSource("acts/a.ink")).toBeNull();
  });

  it("surfaces as one error notification through store.renameFolder, with no undo entry", async () => {
    await initWasm();
    const session = new EditorSessionHandle();
    markReadOnly(session, "chapters/mounted.brink", "=== mounted ===\n-> END\n");
    const provider = new InMemoryFileProvider({
      "main.ink": MAIN,
      "chapters/a.ink": A,
      "chapters/b.ink": B,
    });
    const notify = vi.fn();
    const project = new ProjectSession({ provider, entryFile: "main.ink", session });
    await project.initialize();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments(), _notify: notify });

    await settleAll(
      store
        .getState()
        .renameFolder("chapters/", "acts/", ["chapters/a.ink", "chapters/b.ink"]),
    );

    expect(project.getSession().getFileSource("chapters/a.ink")).toBe(A);
    expect(project.getSession().getFileSource("acts/a.ink")).toBeNull();
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ severity: "error" }));
    expect(store.getState().undoStack).toHaveLength(0);
  });
});

describe("ProjectSession.renameDir does not clobber a file when the destination nests inside the source (#2916 review)", () => {
  // Renaming folder `a/` to `a/nested/` moves BOTH `a/a.ink` (→
  // `a/nested/a.ink`) and `a/nested/a.ink` (→ `a/nested/nested/a.ink`) — so
  // `a/nested/a.ink` is simultaneously the FIRST entry's destination and the
  // SECOND entry's source. Applying each entry as a self-contained
  // write-then-remove (old code) let the second entry's removal of
  // `a/nested/a.ink` wipe out the correct content the first entry had just
  // written there, permanently losing that file — not merely a skip, real
  // data loss. Neither the real Rust op nor the mock refuses this shape
  // (the destination is occupied by a file that is itself moving, so the
  // collision check doesn't fire), so this is a reachable production case.
  const OUTER = "=== outerKnot ===\n-> END\n";
  const INNER = "=== innerKnot ===\n-> END\n";
  const ENTRY = "-> start\n=== start ===\nHello.\n-> END\n";

  it("keeps both moved files' own content, in both the session and the provider", async () => {
    const { project, provider } = await makeProject({
      "main.ink": ENTRY,
      "a/a.ink": OUTER,
      "a/nested/a.ink": INNER,
    });
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });
    const session = project.getSession();

    await settleAll(
      store.getState().renameFolder("a/", "a/nested/", ["a/a.ink", "a/nested/a.ink"]),
    );

    // Session state.
    expect(session.getFileSource("a/nested/a.ink")).toBe(OUTER);
    expect(session.getFileSource("a/nested/nested/a.ink")).toBe(INNER);
    expect(session.getFileSource("a/a.ink")).toBeNull();

    // Provider state — the write/delete pass had the identical apply-order
    // bug, independent of the session pass above.
    await expect(provider.readFile("a/nested/a.ink")).resolves.toBe(OUTER);
    await expect(provider.readFile("a/nested/nested/a.ink")).resolves.toBe(INNER);
    await expect(provider.readFile("a/a.ink")).rejects.toThrow();
  });
});

describe("binder.undo() leaves a refused directory-move entry on the stack (#2916 review)", () => {
  it("does not pop the entry or send a false-success 'Undid:' notification when the inverse move is refused", async () => {
    const { project } = await makeProject();
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments(), _notify: notify });
    const session = project.getSession();

    await settleAll(
      store
        .getState()
        .renameFolder("chapters/", "acts/", ["chapters/a.ink", "chapters/b.ink"]),
    );
    expect(store.getState().undoStack).toHaveLength(1);

    // Something now occupies the OLD location, so the inverse move
    // (undo swaps the prefixes back) collides.
    await project.addFile("chapters/a.ink", "=== collide ===\n-> END\n");
    notify.mockClear();

    await settleAll(store.getState().undo());

    // Refused: nothing moved back, the entry stays on the stack — it must
    // stay recoverable, not be silently discarded as if the undo had
    // succeeded — and only the error notification (raised by
    // `applyDirRename`) fired, with no false-success "Undid: ..." stacked
    // on top of it.
    expect(session.getFileSource("acts/a.ink")).toBe(A);
    expect(session.getFileSource("acts/b.ink")).toBe(B);
    expect(session.getFileSource("chapters/a.ink")).toBe("=== collide ===\n-> END\n");
    expect(store.getState().undoStack).toHaveLength(1);
    expect(notify).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ severity: "error" }));
  });
});
