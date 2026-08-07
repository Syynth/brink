/**
 * External deletion of an open file: keep the view, mark orphaned; ⌘S
 * recreates (issue #2371, ruled 2026-08-07 in docs/decision-log.md).
 *
 * `mountStudio`'s `onExternalFileChange` used to skip deletions entirely
 * (`if (content !== null)` — a deliberate remainder left by the #320 view-sync
 * fix, see external-view-sync.test.ts). This pins the pieces the issue named:
 *
 *  1. `FileChangeHub.applyExternal(path, null)` — verified here rather than
 *     assumed — flags the path ORPHANED, and (only once something recreates
 *     the session content from a kept buffer) DIRTY by the hub's existing
 *     no-baseline rule.
 *  2. `DocumentSessions.markOrphaned` — what `onExternalFileChange` now calls
 *     on a deletion instead of ignoring it: the open view's buffer is never
 *     touched (no refresh, no close), and the file is recreated in the wasm
 *     session from that kept buffer so IDE queries and a later save keep
 *     working, even when the buffer was never edited after the deletion (the
 *     no-op-push cache on `DocHandle.pushSource` would otherwise skip the
 *     save-time push entirely).
 *  3. The full ⌘S cycle, both for a host-save provider (the desktop D2
 *     overlay contract — the real target of this ruling) and for the plain
 *     `InMemoryFileProvider` playground path the issue itself names.
 *  4. `StudioApi.getOrphanedFiles()` — the studio-ui pull surface a host
 *     renders a tab badge from, mirroring `getDirtyFiles()`.
 *
 * `mountStudio`'s own wiring is a two-line conditional
 * (`content !== null ? refreshExternal : markOrphaned`) reproduced verbatim
 * in `makeOrphanProject` below; like external-view-sync.test.ts's sibling
 * fix, the wire itself is exercised by the desktop app live-drive and these
 * tests pin the mechanism it calls into.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  DocumentSessions,
  FileChangeHub,
  InMemoryFileProvider,
  ProjectSession,
  type FileChange,
} from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import { CommandRegistry, NotificationCenter } from "@brink/studio-shell";
import { createStudioStore } from "@brink/studio-store";
import { createStudioApi } from "@brink/studio-ui";
import { registerFileCommands } from "../file-commands.js";

const MAIN_INK = "-> start\n=== start ===\nHello apple.\n-> END\n";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

// ── Half 1: the hub primitive, in isolation ─────────────────────────
//
// The issue explicitly asks to verify this rather than assume it — a plain
// stub session (no ProjectSession/wasm involved), matching the harness style
// of overlay-persistence.test.ts's "Half 1".

function stubHub(deliveryPersists?: boolean) {
  const files = new Map<string, string>();
  const flushes: FileChange[][] = [];
  const hub = new FileChangeHub({
    getContent: (path) => files.get(path) ?? null,
    onFlush: (changes) => flushes.push(changes),
    debounceMs: 500,
    deliveryPersists,
  });
  return { files, flushes, hub };
}

describe("FileChangeHub.applyExternal(path, null) — issue #2371", () => {
  it("flags orphaned; NOT dirty when nothing recreates the session content", () => {
    const { files, hub } = stubHub();
    hub.setBaseline("a.ink", "on disk");
    files.delete("a.ink"); // the session dropped it, like ProjectSession.removeFile

    hub.applyExternal("a.ink", null);

    expect(hub.isOrphaned("a.ink")).toBe(true);
    expect(hub.orphanedPaths()).toEqual(["a.ink"]);
    // No buffer survived to recreate the session content — nothing to
    // report as dirty yet (matches ProjectSession.removeFile running BEFORE
    // applyExternal, so `getContent` already reads null here).
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("recreating the session content (a kept buffer) makes it dirty too, with no baseline", () => {
    const { files, hub } = stubHub();
    hub.setBaseline("a.ink", "on disk");
    files.delete("a.ink");
    hub.applyExternal("a.ink", null);

    // The kept buffer is pushed back into the session (what
    // `DocumentSessions.markOrphaned` does via `ProjectSession.recreateOrphaned`).
    files.set("a.ink", "on disk"); // even byte-identical to the old baseline
    hub.record("a.ink", "modified");

    expect(hub.isOrphaned("a.ink")).toBe(true);
    expect(hub.dirtyPaths()).toEqual(["a.ink"]); // no baseline ⇒ dirty, per the existing rule
  });

  it("markSaved (a canonical save) clears both orphaned and dirty", () => {
    const { files, hub } = stubHub();
    hub.setBaseline("a.ink", "on disk");
    files.delete("a.ink");
    hub.applyExternal("a.ink", null);
    files.set("a.ink", "on disk");
    hub.record("a.ink", "modified");
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);

    hub.markSaved(["a.ink"]);

    expect(hub.isOrphaned("a.ink")).toBe(false);
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("the path reappearing on disk (a non-null applyExternal) clears orphaned", () => {
    const { files, hub } = stubHub();
    hub.setBaseline("a.ink", "on disk");
    files.delete("a.ink");
    hub.applyExternal("a.ink", null);
    expect(hub.isOrphaned("a.ink")).toBe(true);

    files.set("a.ink", "back again");
    hub.applyExternal("a.ink", "back again");

    expect(hub.isOrphaned("a.ink")).toBe(false);
    expect(hub.orphanedPaths()).toEqual([]);
  });

  it("a write-through flush() (deliveryPersists: true, the default) clears orphaned too — delivery IS its persistence", () => {
    const { files, hub } = stubHub(true);
    hub.setBaseline("a.ink", "on disk");
    files.delete("a.ink");
    hub.applyExternal("a.ink", null);
    files.set("a.ink", "on disk");
    hub.record("a.ink", "modified");

    hub.flush();

    expect(hub.isOrphaned("a.ink")).toBe(false);
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("an overlay flush() (deliveryPersists: false) does NOT clear orphaned — only markSaved does", () => {
    const { files, hub } = stubHub(false);
    hub.setBaseline("a.ink", "on disk");
    files.delete("a.ink");
    hub.applyExternal("a.ink", null);
    files.set("a.ink", "on disk");
    hub.record("a.ink", "modified");

    hub.flush(); // feeds the backup ring only

    expect(hub.isOrphaned("a.ink")).toBe(true);
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);
  });
});

// ── Half 2: the full mechanism — ProjectSession + DocumentSessions ──

/** `mountStudio`'s own `onExternalFileChange` wire, reproduced verbatim. */
async function makeOrphanProject(provider: InMemoryFileProvider) {
  await initWasm();
  let documents: DocumentSessions | undefined;
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    onExternalFileChange: (path, content) => {
      if (content !== null) documents?.refreshExternal(path);
      else documents?.markOrphaned(path);
    },
  });
  await project.initialize();
  documents = new DocumentSessions(project);
  return { project, documents };
}

function mountMain(documents: DocumentSessions) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const dispose = documents.mountView("main.ink", "g1", container);
  documents.setFocused("main.ink", "g1");
  const dom = container.querySelector(".cm-editor");
  const view = dom === null ? null : findView(dom as HTMLElement);
  if (view === null) throw new Error("no editor mounted");
  return { container, dispose, view };
}

// Mirrors external-view-sync.test.ts's dynamic import of EditorView.
async function importEditorView() {
  return (await import("@codemirror/view")).EditorView;
}
function findView(dom: HTMLElement) {
  // Populated lazily below (vitest hoists imports; this keeps the helper
  // synchronous for callers that already awaited `importEditorView` once).
  return cachedEditorView?.findFromDOM(dom) ?? null;
}
let cachedEditorView: Awaited<ReturnType<typeof importEditorView>> | undefined;

describe("DocumentSessions.markOrphaned — issue #2371", () => {
  beforeEach(async () => {
    cachedEditorView = await importEditorView();
  });

  it("with no open view: orphaned but not dirty, nothing to preserve", async () => {
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const { project } = await makeOrphanProject(provider);

    provider.pushExternalChange("main.ink", null);

    expect(project.orphanedPaths()).toEqual(["main.ink"]);
    expect(project.dirtyPaths()).toEqual([]);
    expect(project.getSession().getFileSource("main.ink")).toBeNull();
  });

  it("with a mounted view: the buffer survives untouched, orphaned AND dirty immediately, session recreated", async () => {
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const { project, documents } = await makeOrphanProject(provider);
    const { container, dispose, view } = mountMain(documents);

    provider.pushExternalChange("main.ink", null);

    // Never auto-closed, never refreshed (the #320 clean-path repair only
    // ever fires for `content !== null`) — the buffer is exactly what it was.
    expect(view.state.doc.toString()).toBe(MAIN_INK);
    expect(project.orphanedPaths()).toEqual(["main.ink"]);
    expect(project.dirtyPaths()).toEqual(["main.ink"]);
    // Recreated in the session from the kept buffer — IDE queries against
    // the still-open handle keep working instead of degrading to "unknown
    // handle" for every hover/compile/completions call.
    expect(project.getSession().getFileSource("main.ink")).toBe(MAIN_INK);

    dispose();
    container.remove();
  });

  it("editing the orphaned buffer keeps it recreated with the new text", async () => {
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const { project, documents } = await makeOrphanProject(provider);
    const { container, dispose, view } = mountMain(documents);

    provider.pushExternalChange("main.ink", null);
    view.dispatch({ changes: { from: 0, to: 0, insert: "// resurrected\n" } });
    // The editor's own compile debounce pushes the edit through
    // `slot.handle.pushSource` — force it now rather than waiting on timers.
    documents.flushFocused();

    expect(project.getSession().getFileSource("main.ink")).toBe(
      "// resurrected\n" + MAIN_INK,
    );
    expect(project.orphanedPaths()).toEqual(["main.ink"]);
    expect(project.dirtyPaths()).toEqual(["main.ink"]);

    dispose();
    container.remove();
  });
});

// ── Half 3: the full ⌘S cycle ────────────────────────────────────────

/** A minimal host-save provider (the desktop D2 overlay contract this
 *  ruling targets): edits stage via `onFileChanged`; `requestSave` is the
 *  only real "disk" write — mirrors `TauriFileProvider`'s staged+requestSave
 *  split closely enough to prove ⌘S, not the deletion detection, is what
 *  writes the file back. */
class HostSaveProvider extends InMemoryFileProvider {
  staged = new Map<string, string>();
  writes: { path: string; content: string }[] = [];

  onFileChanged(path: string, content: string): void {
    this.staged.set(path, content);
  }

  async requestSave(paths?: string[]): Promise<void> {
    const wanted = paths === undefined ? null : new Set(paths);
    for (const [path, content] of [...this.staged.entries()]) {
      if (wanted !== null && !wanted.has(path)) continue;
      this.writes.push({ path, content });
      await this.createFile(path, content); // the "disk" write
      this.staged.delete(path);
    }
  }
}

describe("⌘S recreates the file — issue #2371 full cycle", () => {
  beforeEach(async () => {
    cachedEditorView = await importEditorView();
  });

  function saveHarness(documents: DocumentSessions, project: ProjectSession) {
    const commands = new CommandRegistry();
    const notifications = new NotificationCenter();
    registerFileCommands(commands, {
      project,
      documents,
      notify: (n) => void notifications.notify(n),
    });
    return commands;
  }

  it("host-save (desktop overlay contract): ⌘S recreates the file on disk even with an UNEDITED buffer", async () => {
    const provider = new HostSaveProvider({ "main.ink": MAIN_INK });
    const { project, documents } = await makeOrphanProject(provider);
    const { container, dispose } = mountMain(documents);
    const commands = saveHarness(documents, project);

    // Sync the view's content through its handle once BEFORE the deletion —
    // exactly what a real session does (the editor's own compile debounce
    // already pushed the mounted text at least once). This is what makes the
    // regression real: without `markOrphaned`'s session recreation, the
    // handle's no-op-push cache (`DocHandle.pushSource`'s `lastPushed`, still
    // holding this same MAIN_INK text) would silently skip the save-time
    // push entirely, and nothing would ever reach `requestSave`.
    documents.flushFocused();

    provider.pushExternalChange("main.ink", null);
    expect(await provider.requestFile("main.ink")).toBeNull(); // really gone

    expect(commands.dispatch("file.save")).toBe(true);
    // The save is async (project.save().then(...)) — drain the microtask/
    // timer queue under fake timers until it settles.
    await vi.runAllTimersAsync();

    expect(project.orphanedPaths()).toEqual([]);
    expect(provider.writes).toEqual([{ path: "main.ink", content: MAIN_INK }]);
    expect(await provider.requestFile("main.ink")).toBe(MAIN_INK);
    expect(project.dirtyPaths()).toEqual([]);

    dispose();
    container.remove();
  });

  it("host-save: ⌘S writes the EDITED buffer, not the pre-deletion text", async () => {
    const provider = new HostSaveProvider({ "main.ink": MAIN_INK });
    const { project, documents } = await makeOrphanProject(provider);
    const { container, dispose, view } = mountMain(documents);

    provider.pushExternalChange("main.ink", null);
    view.dispatch({ changes: { from: 0, to: 0, insert: "// resurrected\n" } });
    const commands = saveHarness(documents, project);

    commands.dispatch("file.save");
    await vi.runAllTimersAsync();

    expect(project.orphanedPaths()).toEqual([]);
    expect(provider.writes).toEqual([
      { path: "main.ink", content: "// resurrected\n" + MAIN_INK },
    ]);
    expect(project.dirtyPaths()).toEqual([]);

    dispose();
    container.remove();
  });

  it("playground path (no host save): InMemoryFileProvider.pushExternalChange(path, null) then file.save recreates it", async () => {
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const { project, documents } = await makeOrphanProject(provider);
    const { container, dispose } = mountMain(documents);
    const commands = saveHarness(documents, project);

    provider.pushExternalChange("main.ink", null);
    expect(await provider.requestFile("main.ink")).toBeNull();
    expect(project.orphanedPaths()).toEqual(["main.ink"]);
    expect(project.dirtyPaths()).toEqual(["main.ink"]);

    expect(commands.dispatch("file.save")).toBe(true);

    expect(await provider.requestFile("main.ink")).toBe(MAIN_INK);
    expect(project.orphanedPaths()).toEqual([]);
    expect(project.dirtyPaths()).toEqual([]);

    dispose();
    container.remove();
  });
});

// ── Half 4: the studio-ui pull surface ──────────────────────────────

describe("StudioApi.getOrphanedFiles — issue #2371", () => {
  it("mirrors getDirtyFiles: empty before binding, reads through the project, tracks the orphan lifecycle", async () => {
    const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
    const { project } = await makeOrphanProject(provider);
    const store = createStudioStore();
    const commands = new CommandRegistry();
    const notifications = new NotificationCenter();
    const api = createStudioApi({ store, commands, notifications });

    expect(api.getOrphanedFiles()).toEqual([]); // before the project binds

    store.setState({ _project: project });
    expect(api.getOrphanedFiles()).toEqual([]);

    provider.pushExternalChange("main.ink", null);
    expect(api.getOrphanedFiles()).toEqual(["main.ink"]);

    project.markFilesSaved(["main.ink"]);
    expect(api.getOrphanedFiles()).toEqual([]);
  });
});
