/**
 * File-content egress tests (issues #154/#137): every session mutation path
 * routes through the project's shared notify seam — CM6 edit flushes,
 * binder structural ops, search replace, file.new — and the seam feeds both
 * the provider write-back and the host `onFilesChanged` callback. Plus the
 * file.save / file.saveAll commands (immediate flush + notification), the
 * dirty lifecycle, and the StudioApi pull surface (getFiles/getDirtyFiles,
 * the dirtyFiles public-state summary).
 *
 * Runs against the brink-web mock (src/__mocks__/brink-web.ts).
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  DocumentSessions,
  InMemoryFileProvider,
  ProjectSession,
  type FileChange,
  type FileConflict,
} from "@brink/ink-editor";
import { initWasm } from "@brink-lang/web";
import type { StructuralResult } from "@brink/wasm-types";
import { CommandRegistry, Keymap, NotificationCenter } from "@brink/studio-shell";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";
import { createStudioApi } from "@brink/studio-ui";
import { EditorView } from "@codemirror/view";
import { registerFileCommands } from "../file-commands.js";

const MAIN_INK = "-> start\n=== start ===\nHello apple.\n-> END\n";
const SIDE_INK = "=== side ===\nAnother apple here.\n-> END\n";

interface Egress {
  provider: InMemoryFileProvider;
  project: ProjectSession;
  batches: FileChange[][];
}

async function makeProject(opts: { withHook?: boolean } = {}): Promise<Egress> {
  await initWasm();
  const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK, "side.ink": SIDE_INK });
  const batches: FileChange[][] = [];
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    onFilesChanged: (opts.withHook ?? true) ? (changes) => batches.push(changes) : undefined,
  });
  await project.initialize();
  return { provider, project, batches };
}

/** Stub the per-view machinery the bulk-edit slices call after editing. */
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

// ── ProjectSession seam ─────────────────────────────────────────────

describe("ProjectSession egress seam", () => {
  it("starts clean: mount files are the baseline", async () => {
    const { project } = await makeProject();
    expect(project.dirtyPaths()).toEqual([]);
  });

  it("applyEdit writes the session, the provider, and the host batch", async () => {
    const { provider, project, batches } = await makeProject();
    const onFileChanged = vi.spyOn(provider, "onFileChanged");

    project.applyEdit("main.ink", "REWRITTEN");
    expect(project.getSession().getFileSource("main.ink")).toBe("REWRITTEN");
    expect(onFileChanged).toHaveBeenCalledWith("main.ink", "REWRITTEN");

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([[{ path: "main.ink", type: "modified", content: "REWRITTEN" }]]);
  });

  it("addFile (file.new) reports a created change", async () => {
    const { provider, project, batches } = await makeProject();
    await project.addFile("fresh.ink", "");
    expect(await provider.requestFile("fresh.ink")).toBe("");
    expect(project.dirtyPaths()).toEqual(["fresh.ink"]);

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([[{ path: "fresh.ink", type: "created", content: "" }]]);
  });

  it("notifyFileChanged with unchanged content never reaches the host", async () => {
    const { project, batches } = await makeProject();
    project.notifyFileChanged("main.ink"); // e.g. the initial compile flush
    vi.advanceTimersByTime(500);
    expect(batches).toEqual([]);
    expect(project.dirtyPaths()).toEqual([]);
  });

  it("getFiles snapshots every session file, sorted by path", async () => {
    const { project } = await makeProject();
    project.applyEdit("side.ink", "changed");
    expect(project.getFiles()).toEqual({ "main.ink": MAIN_INK, "side.ink": "changed" });
    expect(Object.keys(project.getFiles())).toEqual(["main.ink", "side.ink"]);
  });

  it("flushFileChanges delivers immediately; markFilesSaved clears dirty without a hook", async () => {
    const { project, batches } = await makeProject();
    project.applyEdit("main.ink", "now");
    const delivered = project.flushFileChanges();
    expect(delivered).toEqual([{ path: "main.ink", type: "modified", content: "now" }]);
    expect(batches).toHaveLength(1);
    expect(project.dirtyPaths()).toEqual([]); // delivery re-baselines

    const hookless = await makeProject({ withHook: false });
    hookless.project.applyEdit("main.ink", "now");
    expect(hookless.project.flushFileChanges()).toEqual([]);
    expect(hookless.project.dirtyPaths()).toEqual(["main.ink"]);
    hookless.project.markFilesSaved(["main.ink"]);
    expect(hookless.project.dirtyPaths()).toEqual([]);
  });

  it("reports the dirty count through setDirtyListener", async () => {
    const { project } = await makeProject();
    const counts: number[] = [];
    project.setDirtyListener((n) => counts.push(n));
    project.applyEdit("main.ink", "v2");
    project.markAllSaved();
    expect(counts).toEqual([0, 1, 0]); // immediate report on bind, then transitions
  });
});

// ── External-change conflict handling, end-to-end (#320) ────────────
//
// These drive a REAL ProjectSession through `provider.onExternalChange` —
// the production handler in project-session.ts that contained the silent
// data-loss bug. Unlike the FileChangeHub unit tests (which exercise the
// hub primitives in isolation), these prove the handler itself does NOT
// clobber the live wasm buffer, fires `onFileConflict`, and flags the path
// conflicted. A reintroduction of the original two-part clobber
// (updateFile + applyExternal on a dirty path) would fail these.

/** InMemoryFileProvider that also emits external (on-disk) changes, so a
 *  ProjectSession's `provider.onExternalChange` registration can be driven
 *  the way a real filesystem watcher would (issue #320). */
class WatchedFileProvider extends InMemoryFileProvider {
  private watchers = new Set<(path: string, content: string | null) => void>();

  onExternalChange(callback: (path: string, content: string | null) => void): () => void {
    this.watchers.add(callback);
    return () => this.watchers.delete(callback);
  }

  /** Simulate the host rewriting `path` on disk to `content` (null = delete)
   *  and notifying watchers — exactly what a filesystem watcher would do. */
  emitExternalChange(path: string, content: string | null): void {
    if (content === null) void this.deleteFile(path);
    else void this.createFile(path, content);
    for (const w of this.watchers) w(path, content);
  }
}

async function makeWatchedProject(): Promise<{
  provider: WatchedFileProvider;
  project: ProjectSession;
  conflicts: FileConflict[];
  externals: { path: string; content: string | null }[];
}> {
  await initWasm();
  const provider = new WatchedFileProvider({ "main.ink": MAIN_INK, "side.ink": SIDE_INK });
  const conflicts: FileConflict[] = [];
  const externals: { path: string; content: string | null }[] = [];
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    onFileConflict: (c) => conflicts.push(c),
    onExternalFileChange: (path, content) => externals.push({ path, content }),
  });
  await project.initialize();
  return { provider, project, conflicts, externals };
}

describe("ProjectSession external-change handler (#320)", () => {
  it("does NOT clobber a dirty wasm buffer; flags conflicted; fires onFileConflict", async () => {
    const { provider, project, conflicts, externals } = await makeWatchedProject();

    // The studio has an unsaved, divergent edit to main.ink.
    project.applyEdit("main.ink", "studio edit");
    expect(project.dirtyPaths()).toEqual(["main.ink"]);

    // The host rewrites main.ink on disk to something else.
    provider.emitExternalChange("main.ink", "host edit");

    // The live wasm buffer is UNTOUCHED — the unsaved edit survives. This is
    // the exact data-loss the original two-part clobber caused.
    expect(project.getSession().getFileSource("main.ink")).toBe("studio edit");
    // The path is flagged conflicted (safe default), still dirty.
    expect(project.conflictedPaths()).toEqual(["main.ink"]);
    expect(project.dirtyPaths()).toEqual(["main.ink"]);
    // The conflict hook fired with all three texts for a merge surface.
    expect(conflicts).toEqual([
      { path: "main.ink", disk: "host edit", buffer: "studio edit", baseline: MAIN_INK },
    ]);
    // The non-conflict external-change callback did NOT fire (no re-baseline).
    expect(externals).toEqual([]);
  });

  it("a clean (non-dirty) path is updated by the external change — no conflict", async () => {
    const { provider, project, conflicts, externals } = await makeWatchedProject();
    expect(project.dirtyPaths()).toEqual([]);

    provider.emitExternalChange("main.ink", "host rewrite");

    // Clean buffer: the host content wins, the wasm buffer updates.
    expect(project.getSession().getFileSource("main.ink")).toBe("host rewrite");
    expect(project.conflictedPaths()).toEqual([]);
    expect(project.dirtyPaths()).toEqual([]); // re-baselined to disk
    expect(conflicts).toEqual([]);
    expect(externals).toEqual([{ path: "main.ink", content: "host rewrite" }]);
  });

  it("an external change matching the dirty buffer is not a conflict", async () => {
    const { provider, project, conflicts, externals } = await makeWatchedProject();
    project.applyEdit("main.ink", "converged");
    expect(project.dirtyPaths()).toEqual(["main.ink"]);

    // The host wrote the same text the studio edited to: nothing to reconcile.
    provider.emitExternalChange("main.ink", "converged");

    expect(project.getSession().getFileSource("main.ink")).toBe("converged");
    expect(project.conflictedPaths()).toEqual([]);
    expect(project.dirtyPaths()).toEqual([]); // re-baselined; buffer === disk
    expect(conflicts).toEqual([]);
    expect(externals).toEqual([{ path: "main.ink", content: "converged" }]);
  });
});

// ── Bulk-edit paths route through the seam (#137) ───────────────────

describe("binder structural ops (#137)", () => {
  it("applyMoveResult write-backs and notifies for the moved file and cross-file edits", async () => {
    const { provider, project, batches } = await makeProject();
    const onFileChanged = vi.spyOn(provider, "onFileChanged");
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    const result: StructuralResult = {
      ok: true,
      path: "main.ink",
      new_source: "moved main",
      cross_file_edits: [{ path: "side.ink", new_source: "retargeted side" }],
      safe: true,
      introduced_diagnostics: [],
    };
    await store.getState().applyMoveResult(result, "Moved start", ["main.ink"]);

    // Provider write-back — the original #137 gap.
    expect(onFileChanged).toHaveBeenCalledWith("main.ink", "moved main");
    expect(onFileChanged).toHaveBeenCalledWith("side.ink", "retargeted side");

    // Host egress: one debounced batch naming both files.
    vi.advanceTimersByTime(500);
    expect(batches).toEqual([
      [
        { path: "main.ink", type: "modified", content: "moved main" },
        { path: "side.ink", type: "modified", content: "retargeted side" },
      ],
    ]);
  });

  it("undo notifies the reverted content", async () => {
    const { project, batches } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    const result: StructuralResult = {
      ok: true,
      path: "main.ink",
      new_source: "moved",
      cross_file_edits: [],
      safe: true,
      introduced_diagnostics: [],
    };
    await store.getState().applyMoveResult(result, "Moved start", ["main.ink"]);
    vi.advanceTimersByTime(500);
    await store.getState().undo();
    vi.advanceTimersByTime(500);

    expect(batches).toHaveLength(2);
    expect(batches[1]).toEqual([{ path: "main.ink", type: "modified", content: MAIN_INK }]);
  });
});

describe("search replace (#137)", () => {
  it("replace-all notifies one batch naming every edited file", async () => {
    const { provider, project, batches } = await makeProject();
    const onFileChanged = vi.spyOn(provider, "onFileChanged");
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    store.getState().setSearchQuery("apple");
    store.getState().setSearchReplace("pear");
    store.getState().runSearch();
    expect(store.getState().searchResults?.totalMatches).toBe(2);

    store.getState().replaceAllSearchMatches();
    expect(onFileChanged).toHaveBeenCalledTimes(2);

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([
      [
        { path: "main.ink", type: "modified", content: MAIN_INK.replace("apple", "pear") },
        { path: "side.ink", type: "modified", content: SIDE_INK.replace("apple", "pear") },
      ],
    ]);
  });

  it("single-match replace notifies that file", async () => {
    const { project, batches } = await makeProject();
    const store = createStudioStore();
    store.setState({ _project: project, _documents: stubDocuments() });

    store.getState().setSearchQuery("Hello");
    store.getState().setSearchReplace("Howdy");
    store.getState().runSearch();
    const file = store.getState().searchResults!.files[0]!;
    store.getState().replaceSearchMatch(file.path, file.matches[0]!);

    vi.advanceTimersByTime(500);
    expect(batches).toEqual([
      [{ path: "main.ink", type: "modified", content: MAIN_INK.replace("Hello", "Howdy") }],
    ]);
  });
});

// ── Save commands ───────────────────────────────────────────────────

describe("file.save / file.saveAll", () => {
  function commandHarness(egress: Egress) {
    const documents = new DocumentSessions(egress.project);
    const commands = new CommandRegistry();
    const notifications = new NotificationCenter();
    registerFileCommands(commands, {
      project: egress.project,
      documents,
      notify: (n) => void notifications.notify(n),
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const dispose = documents.mountView("main.ink", "g1", container);
    documents.setFocused("main.ink", "g1");
    const dom = container.querySelector(".cm-editor");
    const view = dom === null ? null : EditorView.findFromDOM(dom as HTMLElement);
    if (!view) throw new Error("no editor mounted");
    return { documents, commands, notifications, container, dispose, view };
  }

  it("file.save flushes the focused editor, fires the host callback immediately, and notifies", async () => {
    const egress = await makeProject();
    const h = commandHarness(egress);

    // Type into the view (a user edit; the session flush is normally on the
    // editor's own debounce — save must bypass it).
    h.view.dispatch({ changes: { from: 0, to: 0, insert: "// edited\n" } });
    expect(h.commands.dispatch("file.save")).toBe(true);

    // Immediate: no debounce advance needed.
    expect(egress.batches).toHaveLength(1);
    expect(egress.batches[0]![0]).toMatchObject({ path: "main.ink", type: "modified" });
    expect(egress.batches[0]![0]!.content).toContain("// edited");
    expect(egress.project.dirtyPaths()).toEqual([]);
    expect(
      h.notifications.getState().visible.map((n) => n.message),
    ).toContain("Saved main.ink");

    h.dispose();
    h.container.remove();
  });

  it("file.save without a host hook still flushes, clears dirty, and notifies", async () => {
    const egress = await makeProject({ withHook: false });
    const h = commandHarness(egress);

    h.view.dispatch({ changes: { from: 0, to: 0, insert: "x" } });
    expect(() => h.commands.dispatch("file.save")).not.toThrow();
    expect(egress.project.getSession().getFileSource("main.ink")).toContain("x");
    expect(egress.project.dirtyPaths()).toEqual([]);
    expect(
      h.notifications.getState().visible.map((n) => n.message),
    ).toContain("Saved main.ink");

    h.dispose();
    h.container.remove();
  });

  it("file.saveAll flushes every dirty file and reports the count", async () => {
    const egress = await makeProject({ withHook: false });
    const h = commandHarness(egress);

    egress.project.applyEdit("side.ink", "bulk edited"); // dirty without a view
    h.view.dispatch({ changes: { from: 0, to: 0, insert: "y" } });
    h.commands.dispatch("file.saveAll");

    expect(egress.project.dirtyPaths()).toEqual([]);
    expect(
      h.notifications.getState().visible.map((n) => n.message),
    ).toContain("Saved 2 files");

    h.dispose();
    h.container.remove();
  });

  it("binds file.save to Mod-S by default", async () => {
    const egress = await makeProject();
    const h = commandHarness(egress);
    const keymap = Keymap.fromCommands(h.commands.list());
    expect(keymap.resolveChord({ key: "s", mod: true, shift: false, alt: false })).toBe(
      "file.save",
    );
    h.dispose();
    h.container.remove();
  });

  it("dirty lifecycle: mount clean → edit dirty → save clean", async () => {
    const egress = await makeProject({ withHook: false });
    const h = commandHarness(egress);
    expect(egress.project.dirtyPaths()).toEqual([]); // mount clean

    h.view.dispatch({ changes: { from: 0, to: 0, insert: "z" } });
    h.documents.flushAll(); // the editor's debounced flush, forced
    expect(egress.project.dirtyPaths()).toEqual(["main.ink"]); // edit dirty

    h.commands.dispatch("file.save");
    expect(egress.project.dirtyPaths()).toEqual([]); // save clean

    h.dispose();
    h.container.remove();
  });
});

// ── StudioApi pull surface ──────────────────────────────────────────

describe("StudioApi egress surface", () => {
  it("getFiles / getDirtyFiles read through the project; dirtyFiles rides public state", async () => {
    const { project } = await makeProject({ withHook: false });
    const store = createStudioStore();
    const commands = new CommandRegistry();
    const notifications = new NotificationCenter();
    const api = createStudioApi({ store, commands, notifications });

    // Before the project binds: empty, never throws.
    expect(api.getFiles()).toEqual({});
    expect(api.getDirtyFiles()).toEqual([]);

    store.setState({ _project: project });
    project.setDirtyListener((n) => store.getState().setDirtyFiles(n));
    expect(api.getFiles()).toEqual({ "main.ink": MAIN_INK, "side.ink": SIDE_INK });
    expect(api.select((s) => s.dirtyFiles)).toBe(0);

    const seen: number[] = [];
    const unsubscribe = api.subscribe((s) => s.dirtyFiles, (n) => seen.push(n));
    project.applyEdit("main.ink", "v2");
    expect(api.getDirtyFiles()).toEqual(["main.ink"]);
    expect(api.select((s) => s.dirtyFiles)).toBe(1);

    project.markAllSaved();
    expect(api.select((s) => s.dirtyFiles)).toBe(0);
    expect(seen).toEqual([1, 0]);
    unsubscribe();
  });
});
