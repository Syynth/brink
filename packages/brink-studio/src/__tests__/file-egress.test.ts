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
} from "@brink-lang/editor";
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

/** Drive a project to a standing conflict on main.ink: dirty studio edit +
 *  divergent on-disk change. Returns the project and the captured conflict. */
async function makeConflictedProject(): Promise<{
  provider: WatchedFileProvider;
  project: ProjectSession;
  conflict: FileConflict;
}> {
  const { provider, project, conflicts } = await makeWatchedProject();
  project.applyEdit("main.ink", "studio edit");
  provider.emitExternalChange("main.ink", "host edit");
  expect(project.conflictedPaths()).toEqual(["main.ink"]);
  return { provider, project, conflict: conflicts[0]! };
}

describe("ProjectSession conflict resolution (#320, Track V)", () => {
  it("resolveConflictUseDisk: buffer becomes disk, re-baselined, clean", async () => {
    const { project, conflict } = await makeConflictedProject();

    project.resolveConflictUseDisk("main.ink", conflict.disk);

    expect(project.getSession().getFileSource("main.ink")).toBe("host edit");
    // Re-baselined to disk → no longer dirty, conflict cleared.
    expect(project.dirtyPaths()).toEqual([]);
    expect(project.conflictedPaths()).toEqual([]);
    expect(project.hasConflict("main.ink")).toBe(false);
  });

  it("resolveConflictKeepMine: buffer kept, stays dirty, conflict cleared", async () => {
    const { project } = await makeConflictedProject();

    project.resolveConflictKeepMine("main.ink");

    // The kept buffer is untouched and still diverges from the host baseline,
    // so it stays dirty — but the conflict flag is gone.
    expect(project.getSession().getFileSource("main.ink")).toBe("studio edit");
    expect(project.dirtyPaths()).toEqual(["main.ink"]);
    expect(project.conflictedPaths()).toEqual([]);
  });

  it("resolveConflictMerged: merged text becomes the dirty buffer, conflict cleared", async () => {
    const { project } = await makeConflictedProject();

    project.resolveConflictMerged("main.ink", "studio edit + host edit");

    expect(project.getSession().getFileSource("main.ink")).toBe("studio edit + host edit");
    // The merged result diverges from baseline → still dirty (saveable), but
    // the conflict is resolved.
    expect(project.dirtyPaths()).toEqual(["main.ink"]);
    expect(project.conflictedPaths()).toEqual([]);
  });

  it("a save after Keep-mine re-baselines the kept buffer (conflict already clear)", async () => {
    const { project } = await makeConflictedProject();
    project.resolveConflictKeepMine("main.ink");
    expect(project.dirtyPaths()).toEqual(["main.ink"]);

    project.markFilesSaved(["main.ink"]);

    expect(project.dirtyPaths()).toEqual([]);
    expect(project.conflictedPaths()).toEqual([]);
  });
});

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

    store.getState().setSearchReplaceOpen(true);
    store.getState().acceptAllSearchMatches();
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
    const match = store.getState().searchResults!.files[0]!.matches[0]!;
    store.getState().acceptSearchMatch(match.id);

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

// ── Host-save under the overlay contract (D2, 2026-08-07 ruling) ────
//
// A provider WITH `requestSave` owns the canonical write: the save
// commands await it and only re-baseline on success. Combined with
// `egressPersists: false`, the egress flush feeds a ring and dirty
// survives delivery — only ⌘S clears it. These run against the real wasm
// session + a real ProjectSession, so they pin the whole-stack contract.

class HostSaveProvider extends InMemoryFileProvider {
  saves: Array<string[] | undefined> = [];
  failNext = false;
  /**
   * Edits STAGE here rather than committing straight to `files` (the
   * `InMemoryFileProvider` backing store `readFile` answers from) — mirrors
   * the real `TauriFileProvider`'s `staged` map (D2 overlay contract): only
   * `requestSave`'s own write commits to "disk". Without this separation,
   * `readFile` would always answer with the latest edit rather than "what a
   * write actually persisted", and the #2426 mid-write tests below (which
   * rely on that gap to prove a stale write doesn't get confirmed) would be
   * proving nothing.
   */
  private hostStaged = new Map<string, string>();
  /** Set by `holdNextSave()`; `requestSave` awaits it before resolving, so a
   *  test can stage an edit while the write is still "in flight" (#2426). */
  private gate: Promise<void> | null = null;
  private releaseGate: (() => void) | null = null;
  /** Set by `holdNextRead()`; `readFile` awaits it before resolving, so a
   *  test can land an edit while a disk-confirmation read (#2435) is itself
   *  still in flight. */
  private readGate: Promise<void> | null = null;
  private releaseReadGate: (() => void) | null = null;

  onFileChanged(path: string, content: string): void {
    this.hostStaged.set(path, content);
  }

  /** The next `requestSave` call blocks until `releaseSave()` is called. */
  holdNextSave(): void {
    this.gate = new Promise((resolve) => {
      this.releaseGate = resolve;
    });
  }

  /** Unblock a `requestSave` call parked by `holdNextSave()`. */
  releaseSave(): void {
    this.releaseGate?.();
    this.gate = null;
    this.releaseGate = null;
  }

  /** The next `readFile` call (the disk-confirmation read `readProviderFile`
   *  drives, issue #2435) blocks until `releaseRead()` is called. */
  holdNextRead(): void {
    this.readGate = new Promise((resolve) => {
      this.releaseReadGate = resolve;
    });
  }

  /** Unblock a `readFile` call parked by `holdNextRead()`. */
  releaseRead(): void {
    this.releaseReadGate?.();
    this.readGate = null;
    this.releaseReadGate = null;
  }

  async readFile(path: string): Promise<string> {
    if (this.readGate) await this.readGate;
    return super.readFile(path);
  }

  async requestSave(paths?: string[]): Promise<void> {
    // The content to write is captured at CALL time — before the gate, like
    // `TauriFileProvider.writeStaged`'s own `pending` snapshot happens
    // before its `invoke("write_file")` — so an edit landing while this
    // call is held open does not retroactively change what this write
    // persists.
    const wanted = paths === undefined ? null : new Set(paths);
    const pending = [...this.hostStaged.entries()].filter(
      ([rel]) => wanted === null || wanted.has(rel),
    );
    if (this.gate) await this.gate;
    if (this.failNext) {
      this.failNext = false;
      throw new Error("disk full");
    }
    this.saves.push(paths);
    for (const [rel, content] of pending) {
      await super.createFile(rel, content); // commits to `files` — this class's "disk"
      // Only drop the staged entry if it still matches what was just
      // written — an edit staged while this write was held open must
      // survive for the next `requestSave` to pick up (mirrors
      // `writeStaged`'s own #2403-review discipline).
      if (this.hostStaged.get(rel) === content) {
        this.hostStaged.delete(rel);
      }
    }
  }
}

async function makeOverlayProject(): Promise<Egress & { host: HostSaveProvider }> {
  await initWasm();
  const host = new HostSaveProvider({ "main.ink": MAIN_INK, "side.ink": SIDE_INK });
  const batches: FileChange[][] = [];
  const project = new ProjectSession({
    provider: host,
    entryFile: "main.ink",
    egressPersists: false,
    onFilesChanged: (changes) => batches.push(changes),
  });
  await project.initialize();
  return { provider: host, host, project, batches };
}

async function microtasks(): Promise<void> {
  for (let i = 0; i < 6; i += 1) await Promise.resolve();
}

describe("host-save (overlay contract)", () => {
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

  it("egress delivery does NOT clear dirty — the ring hears, dirty survives", async () => {
    // No editor view mounted: advancing fake timers with a live CM view
    // trips its jsdom-hostile measure pass; the overlay contract is a
    // session-level property anyway.
    const egress = await makeOverlayProject();
    egress.project.applyEdit("main.ink", `// overlay\n${MAIN_INK}`);
    vi.advanceTimersByTime(500); // egress flush = ring feed
    expect(egress.batches).toHaveLength(1);
    expect(egress.project.dirtyPaths()).toEqual(["main.ink"]); // STILL dirty
  });

  it("⌘S canonically saves through the host and re-baselines on success", async () => {
    const egress = await makeOverlayProject();
    const h = commandHarness(egress);

    h.view.dispatch({ changes: { from: 0, to: 0, insert: "// overlay\n" } });
    h.commands.dispatch("file.save");
    await microtasks();
    expect(egress.host.saves).toEqual([["main.ink"]]); // canonical, narrowed
    expect(egress.project.dirtyPaths()).toEqual([]);
    expect(h.notifications.getState().visible.map((n) => n.message)).toContain(
      "Saved main.ink",
    );

    h.dispose();
    h.container.remove();
  });

  it("a rejected host save keeps the file dirty and reports; retry succeeds", async () => {
    const egress = await makeOverlayProject();
    const h = commandHarness(egress);

    h.view.dispatch({ changes: { from: 0, to: 0, insert: "x" } });
    egress.host.failNext = true;
    h.commands.dispatch("file.save");
    await microtasks();
    expect(egress.project.dirtyPaths()).toEqual(["main.ink"]); // NOT re-baselined
    expect(
      h.notifications.getState().visible.some((n) => n.message.startsWith("Save failed")),
    ).toBe(true);

    h.commands.dispatch("file.save"); // retry
    await microtasks();
    expect(egress.host.saves).toEqual([["main.ink"]]);
    expect(egress.project.dirtyPaths()).toEqual([]);

    h.dispose();
    h.container.remove();
  });

  it("file.saveAll requests a whole-project host save and re-baselines all", async () => {
    const egress = await makeOverlayProject();
    const h = commandHarness(egress);

    egress.project.applyEdit("side.ink", "bulk edited");
    h.view.dispatch({ changes: { from: 0, to: 0, insert: "y" } });
    h.commands.dispatch("file.saveAll");
    await microtasks();
    expect(egress.host.saves).toEqual([undefined]); // unnarrowed = everything
    expect(egress.project.dirtyPaths()).toEqual([]);

    h.dispose();
    h.container.remove();
  });

  // ── #2426: an edit landing while the host write is still in flight
  // must not be retired against the content that was actually written ──

  it("file.save: an edit landing mid-write is NOT marked clean against the old content", async () => {
    const egress = await makeOverlayProject();
    const h = commandHarness(egress);

    h.view.dispatch({ changes: { from: 0, to: 0, insert: "// v1\n" } }); // stage v1
    egress.host.holdNextSave();
    h.commands.dispatch("file.save"); // starts the write; parked on the gate
    await microtasks();
    expect(egress.host.saves).toEqual([]); // write has not resolved yet

    // Stage v2 directly into the session while the v1 write is still open —
    // mirrors an edit landing mid-flight without going through another
    // save command.
    h.view.dispatch({ changes: { from: 0, to: 0, insert: "// v2\n" } });
    h.documents.flushFocused();

    egress.host.releaseSave(); // the v1 write completes now
    await microtasks();
    // A second flush: the mid-write guard's re-check now awaits an extra
    // hop (`readProviderFile`, issue #2435) after the write settles.
    await microtasks();

    expect(egress.host.saves).toEqual([["main.ink"]]); // v1 WAS written
    // v2 must still be dirty: the write persisted v1, not v2.
    expect(egress.project.dirtyPaths()).toEqual(["main.ink"]);
    expect(
      h.notifications.getState().visible.some((n) => n.message.includes("still unsaved")),
    ).toBe(true);

    h.dispose();
    h.container.remove();
  });

  it("file.saveAll: an edit landing mid-write is NOT re-baselined by markAllSaved", async () => {
    const egress = await makeOverlayProject();
    const h = commandHarness(egress);

    egress.project.applyEdit("side.ink", "bulk v1"); // dirty without a view
    h.view.dispatch({ changes: { from: 0, to: 0, insert: "// v1\n" } });
    egress.host.holdNextSave();
    h.commands.dispatch("file.saveAll"); // starts the whole-project write
    await microtasks();
    expect(egress.host.saves).toEqual([]); // write has not resolved yet

    // "side.ink" moves on while the batch write is still open.
    egress.project.applyEdit("side.ink", "bulk v2");

    egress.host.releaseSave();
    await microtasks();
    // A second flush: the mid-write guard's re-check now awaits an extra
    // hop (`readProviderFile`, issue #2435) after the write settles.
    await microtasks();

    expect(egress.host.saves).toEqual([undefined]); // one whole-project write
    // side.ink moved on after being captured — it stays dirty.
    expect(egress.project.dirtyPaths()).toEqual(["side.ink"]);
    expect(egress.project.getSession().getFileSource("side.ink")).toBe("bulk v2");
    expect(
      h.notifications.getState().visible.some((n) => n.message.includes("still unsaved")),
    ).toBe(true);

    h.dispose();
    h.container.remove();
  });

  // ── #2435 review finding: the disk-confirmation re-check itself has a
  // TOCTOU window — an edit landing on ANY saved path (settled or moved)
  // while the batch's confirmation reads are still in flight must not be
  // silently retired against content that was never actually confirmed ──

  it("file.saveAll: an edit landing on an already-settled path during the batch's disk-confirmation read is not silently marked saved", async () => {
    const egress = await makeOverlayProject();
    const h = commandHarness(egress);

    // main.ink will end up in the `moved` bucket (a genuine #2426 mid-write
    // divergence) purely to force a real disk-confirmation read for the
    // batch to await; side.ink is the settled file this test cares about.
    egress.project.applyEdit("main.ink", "main v1");
    egress.project.applyEdit("side.ink", "side v1");
    egress.host.holdNextSave();
    h.commands.dispatch("file.saveAll");
    await microtasks();
    expect(egress.host.saves).toEqual([]); // write has not resolved yet

    // main.ink moves on while the batch write is still open.
    egress.project.applyEdit("main.ink", "main v2");

    // Gate the disk-confirmation read before releasing the write, so the
    // batch's Promise.all over `moved` paths is genuinely in flight when
    // the next edit lands.
    egress.host.holdNextRead();
    egress.host.releaseSave(); // the batch write completes, persisting v1 for both files
    await microtasks();
    await microtasks(); // reach the parked disk-confirmation read for main.ink

    // side.ink — already computed as "settled" (its content still matched
    // the pre-save snapshot when the batch write resolved) — moves on RIGHT
    // NOW, while that confirmation read is still parked. Disk only ever
    // received "side v1" for side.ink.
    egress.project.applyEdit("side.ink", "side v2");

    egress.host.releaseRead(); // main.ink's confirmation read resolves
    await microtasks();
    await microtasks();

    // side.ink's "side v2" was never written or confirmed — it must stay
    // dirty, not be silently retired as saved because `saved` was computed
    // before this edit landed.
    expect(egress.project.dirtyPaths()).toEqual(["main.ink", "side.ink"]);
    expect(egress.project.getSession().getFileSource("side.ink")).toBe("side v2");
    expect(
      h.notifications.getState().visible.some((n) => n.message.includes("still unsaved")),
    ).toBe(true);

    h.dispose();
    h.container.remove();
  });
});

// ── #2435: a `requestSave` QUEUED behind another in-flight write must not
// be false-flagged just because content moved on since the pre-save
// snapshot — with writes serialized (`TauriFileProvider`, #2403), the
// queued write can legitimately pick up the later edit and persist it
// before the guard even checks. `HostSaveProvider` above can't reproduce
// this: its `onFileChanged` (inherited from `InMemoryFileProvider`) commits
// straight to `files`, so its `readFile` always answers with the latest
// edit rather than "what a write actually persisted" — there is no lag for
// a queued write to legitimately close. `QueuedHostSaveProvider` restores
// that lag by mirroring `TauriFileProvider`'s own staged-map + serialized-
// queue algorithm (`writeStaged`/`enqueueSave`) closely enough to exercise
// the real race. ──

/**
 * Mirrors `TauriFileProvider`'s staged-write + write-serialization
 * semantics (`packages/brink-desktop/src/tauri-provider.ts`): edits STAGE
 * into `hostStaged`, decoupled from `files` (the `InMemoryFileProvider`
 * backing store `readFile` answers from — this class's stand-in for
 * "disk"), and `requestSave` only reads `hostStaged` once its own turn in
 * the write queue arrives, not at call time. Each staged write is held open
 * on a gate until `releaseWrite()` — the write serialization itself is real
 * (`enqueueSave`'s chain), so only one gate is ever open at a time; the
 * test drives the queue by releasing gates in order.
 */
class QueuedHostSaveProvider extends InMemoryFileProvider {
  private hostStaged = new Map<string, string>();
  private saving: Promise<unknown> = Promise.resolve();
  private gates: Array<() => void> = [];
  /** Set by `holdNextRead()`; `readFile` awaits it before resolving, so a
   *  test can land an edit while a disk-confirmation read (#2435) is itself
   *  still in flight. */
  private readGate: Promise<void> | null = null;
  private releaseReadGate: (() => void) | null = null;

  onFileChanged(path: string, content: string): void {
    this.hostStaged.set(path, content);
  }

  /** Unblock the oldest still-held write. */
  releaseWrite(): void {
    const release = this.gates.shift();
    if (release === undefined) throw new Error("no write is currently held open");
    release();
  }

  /** The next `readFile` call (the disk-confirmation read `readProviderFile`
   *  drives, issue #2435) blocks until `releaseRead()` is called. */
  holdNextRead(): void {
    this.readGate = new Promise((resolve) => {
      this.releaseReadGate = resolve;
    });
  }

  /** Unblock a `readFile` call parked by `holdNextRead()`. */
  releaseRead(): void {
    this.releaseReadGate?.();
    this.readGate = null;
    this.releaseReadGate = null;
  }

  async readFile(path: string): Promise<string> {
    if (this.readGate) await this.readGate;
    return super.readFile(path);
  }

  async requestSave(paths?: string[]): Promise<void> {
    const next = this.saving.then(
      () => this.writeStaged(paths),
      () => this.writeStaged(paths),
    );
    this.saving = next.catch(() => undefined);
    return next;
  }

  private async writeStaged(paths?: string[]): Promise<void> {
    const wanted = paths === undefined ? null : new Set(paths);
    const pending = [...this.hostStaged.entries()].filter(
      ([rel]) => wanted === null || wanted.has(rel),
    );
    for (const [rel, content] of pending) {
      await new Promise<void>((resolve) => this.gates.push(resolve));
      await super.createFile(rel, content); // commits to `files` — this class's "disk"
      // Only drop the staged entry if it still matches what was just
      // written — mirrors `writeStaged`'s own #2403-review discipline: an
      // edit staged while this write was in flight must survive for the
      // next `requestSave` to pick up.
      if (this.hostStaged.get(rel) === content) {
        this.hostStaged.delete(rel);
      }
    }
  }
}

/** `registerFileCommands`' `documents` dependency, stubbed for tests that
 *  drive edits through `project.applyEdit` directly (no mounted CM6 view;
 *  `file.saveAll` still needs `flushAll`/`flushFocused` to exist). */
function noViewDocuments(): DocumentSessions {
  return {
    flushFocused: () => null,
    flushAll: () => [],
  } as unknown as DocumentSessions;
}

/** `registerFileCommands`' `documents` dependency for `file.save` tests that
 *  drive edits through `project.applyEdit` directly (no mounted CM6 view) —
 *  `flushFocused` reports `path` as the always-focused document, the way a
 *  real `DocumentSessions` would with one mounted, focused view. Without
 *  this, `file.save` can never be reached from a headless harness — see the
 *  #2435 review finding on `noViewDocuments` above. */
function focusedDocuments(path: string): DocumentSessions {
  return {
    flushFocused: () => path,
    flushAll: () => [],
  } as unknown as DocumentSessions;
}

async function makeQueuedOverlayProject(): Promise<Egress & { host: QueuedHostSaveProvider }> {
  await initWasm();
  const host = new QueuedHostSaveProvider({ "main.ink": MAIN_INK, "side.ink": SIDE_INK });
  const batches: FileChange[][] = [];
  const project = new ProjectSession({
    provider: host,
    entryFile: "main.ink",
    egressPersists: false,
    onFilesChanged: (changes) => batches.push(changes),
  });
  await project.initialize();
  return { provider: host, host, project, batches };
}

describe("host-save queued writes (#2435)", () => {
  function harness(egress: Egress) {
    const commands = new CommandRegistry();
    const notifications = new NotificationCenter();
    registerFileCommands(commands, {
      project: egress.project,
      documents: noViewDocuments(),
      notify: (n) => void notifications.notify(n),
    });
    return { commands, notifications };
  }

  /** Same as `harness`, but with `path` reported as the focused document so
   *  `file.save` (not just `file.saveAll`) is reachable. */
  function focusedHarness(egress: Egress, path: string) {
    const commands = new CommandRegistry();
    const notifications = new NotificationCenter();
    registerFileCommands(commands, {
      project: egress.project,
      documents: focusedDocuments(path),
      notify: (n) => void notifications.notify(n),
    });
    return { commands, notifications };
  }

  it("a write queued behind another in-flight write persists the later edit it legitimately caught up to — no false warning", async () => {
    const egress = await makeQueuedOverlayProject();
    const h = harness(egress);

    egress.project.applyEdit("main.ink", "v1");
    h.commands.dispatch("file.saveAll"); // call A: starts writing "v1" immediately
    await microtasks();

    h.commands.dispatch("file.saveAll"); // call B: queues behind call A's still-open write
    await microtasks();

    // The later edit lands while call B is queued (not yet reading
    // `hostStaged`) — a queued write, not a live one, is what will pick
    // this up.
    egress.project.applyEdit("main.ink", "v2");

    egress.host.releaseWrite(); // call A's write completes, persisting "v1"
    await microtasks();
    await microtasks(); // generous margin: call A's own confirmation read + notify
    // Call A's own write genuinely diverged (it persisted "v1" while the
    // session had already moved to "v2") — it correctly warns for its own
    // save, same as the #2426 tests above. That's not this test's claim;
    // clear the notifications call A raised before asserting on call B.
    h.notifications.getState().visible.forEach((n) => h.notifications.dismiss(n.id));

    egress.host.releaseWrite(); // call B's write completes, persisting "v2"
    await microtasks();
    await microtasks(); // generous margin: call B's own confirmation read + notify

    // Call B's write wrote exactly the content the session ends up
    // holding — confirmed via the provider's actual disk content, not the
    // pre-save snapshot — so it must be treated as saved.
    expect(await egress.host.readFile("main.ink")).toBe("v2");
    expect(egress.project.dirtyPaths()).toEqual([]);
    expect(
      h.notifications.getState().visible.some((n) => n.message.includes("still unsaved")),
    ).toBe(false);
    expect(
      h.notifications.getState().visible.some((n) => n.message === "Saved 1 file"),
    ).toBe(true);
  });

  it("an edit landing mid-write (during the write's own flight, not merely queued) still raises the warning and stays dirty", async () => {
    const egress = await makeQueuedOverlayProject();
    const h = harness(egress);

    egress.project.applyEdit("main.ink", "v1");
    h.commands.dispatch("file.saveAll"); // starts writing "v1" immediately
    await microtasks();

    // The edit lands DURING this save's own write, not while a second call
    // is queued behind it — the genuine #2426 divergence.
    egress.project.applyEdit("main.ink", "v2");

    egress.host.releaseWrite(); // the write completes, persisting "v1"
    await microtasks();
    await microtasks(); // generous margin: the confirmation read + notify

    expect(await egress.host.readFile("main.ink")).toBe("v1"); // disk still holds the old content
    expect(egress.project.dirtyPaths()).toEqual(["main.ink"]); // stays dirty
    expect(
      h.notifications.getState().visible.some((n) => n.message.includes("still unsaved")),
    ).toBe(true);
  });

  // `file.save`'s confirmed arm (`onDisk === current` → `markSavedAndNotify`)
  // had no test at all before this: both tests above dispatch
  // `file.saveAll`, and `noViewDocuments()` hard-codes `flushFocused: () =>
  // null`, so `file.save` always took the "no editor focused" early return
  // and could never reach it (#2435 review finding).

  it("file.save: a write queued behind another in-flight write persists the later edit it legitimately caught up to — no false warning", async () => {
    const egress = await makeQueuedOverlayProject();
    const h = focusedHarness(egress, "main.ink");

    egress.project.applyEdit("main.ink", "v1");
    h.commands.dispatch("file.save"); // call A: starts writing "v1" immediately
    await microtasks();

    h.commands.dispatch("file.save"); // call B: queues behind call A's still-open write
    await microtasks();

    // The later edit lands while call B is queued (not yet reading
    // `hostStaged`) — a queued write, not a live one, is what will pick
    // this up.
    egress.project.applyEdit("main.ink", "v2");

    egress.host.releaseWrite(); // call A's write completes, persisting "v1"
    await microtasks();
    await microtasks(); // generous margin: call A's own confirmation read + notify
    // Call A's own write genuinely diverged — it correctly warns for its
    // own save, same as the #2426 tests above. Clear that before asserting
    // on call B.
    h.notifications.getState().visible.forEach((n) => h.notifications.dismiss(n.id));

    egress.host.releaseWrite(); // call B's write completes, persisting "v2"
    await microtasks();
    await microtasks(); // generous margin: call B's own confirmation read + notify

    expect(await egress.host.readFile("main.ink")).toBe("v2");
    expect(egress.project.dirtyPaths()).toEqual([]);
    expect(
      h.notifications.getState().visible.some((n) => n.message.includes("still unsaved")),
    ).toBe(false);
    expect(
      h.notifications.getState().visible.some((n) => n.message === "Saved main.ink"),
    ).toBe(true);
  });

  // ── #2435 review finding: `file.save`'s own confirmation re-check has a
  // TOCTOU window too — an edit landing while the `readProviderFile` await
  // is itself in flight must not be confirmed against a stale pre-await
  // snapshot ──

  it("file.save: an edit landing during the disk-confirmation read is not marked saved against stale content (TOCTOU)", async () => {
    const egress = await makeQueuedOverlayProject();
    const h = focusedHarness(egress, "main.ink");

    egress.project.applyEdit("main.ink", "v1");
    h.commands.dispatch("file.save"); // call A: starts writing "v1" immediately
    await microtasks();

    h.commands.dispatch("file.save"); // call B: queues behind call A's still-open write
    await microtasks();

    // main.ink moves on while call B is queued — call B will legitimately
    // pick this up when its turn in the write queue arrives.
    egress.project.applyEdit("main.ink", "v2");

    egress.host.releaseWrite(); // call A's write completes, persisting "v1"
    await microtasks();
    await microtasks();
    h.notifications.getState().visible.forEach((n) => h.notifications.dismiss(n.id));

    // Gate call B's disk-confirmation read before releasing its write, so
    // that read is genuinely in flight when the next edit lands.
    egress.host.holdNextRead();
    egress.host.releaseWrite(); // call B's write completes, persisting "v2"
    await microtasks(); // call B reaches its confirmation read and parks on the gate

    // A THIRD edit lands while call B's `readProviderFile` is still
    // resolving — disk holds "v2" (what call B is confirming against), but
    // the session has already moved on to "v3", which was never written or
    // confirmed.
    egress.project.applyEdit("main.ink", "v3");

    egress.host.releaseRead(); // call B's confirmation read resolves: onDisk === "v2"
    await microtasks();
    await microtasks();

    // Disk only ever received "v2" — "v3" was never written or confirmed,
    // so it must still be reported dirty, not silently retired against a
    // disk read that only ever verified the older "v2".
    expect(await egress.host.readFile("main.ink")).toBe("v2");
    expect(egress.project.dirtyPaths()).toEqual(["main.ink"]);
    expect(
      h.notifications.getState().visible.some((n) => n.message.includes("still unsaved")),
    ).toBe(true);
  });
});
