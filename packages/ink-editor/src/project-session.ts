/**
 * ProjectSession — bridges a FileProvider with an EditorSession.
 *
 * Handles multi-file loading, INCLUDE resolution, file creation, provider
 * write-back, and project compilation (cached by the session's mutation
 * generation, so several live views can each ask for "the current compile"
 * without recompiling an unchanged project).
 *
 * It also owns the {@link FileChangeHub} — the single seam every content
 * mutation reports through (issues #154/#137). The CM6 edit flush calls
 * `notifyFileChanged`; bulk edit paths (binder structural ops, search
 * replace) call `applyEdit`; `addFile` records creations. All of them feed
 * both the provider write-back and the host egress callback.
 *
 * Per-view document state (wasm document handles, CM6 states, mirroring)
 * lives in DocumentSessions — this class owns only project-level concerns.
 */

import type { FileProvider } from "./provider.js";
import { EditorSessionHandle } from "@brink-lang/web";
import type { CompileResult } from "@brink/wasm-types";
import { FileChangeHub, type FileChange, type FileConflict } from "./file-change-hub.js";

export interface ProjectSessionOptions {
  provider: FileProvider;
  entryFile: string;
  /** Re-use an existing session, or a new one is created. */
  session?: EditorSessionHandle;
  /** Called when an external file change is detected. */
  onExternalFileChange?: (path: string, content: string | null) => void;
  /**
   * Called when an external change collides with an unsaved studio buffer
   * (issue #320). The studio keeps the dirty buffer (the SAFE DEFAULT) and
   * flags the path conflicted; this hook lets a merge/diff surface (Track V)
   * reconcile the host's on-disk content with the kept buffer.
   */
  onFileConflict?: (conflict: FileConflict) => void;
  /**
   * Host egress callback (issue #154): receives debounced, batched change
   * notifications for every session-content mutation. See FileChangeHub.
   */
  onFilesChanged?: (changes: FileChange[]) => void;
  /** Trailing debounce for `onFilesChanged` batches (default 500 ms). */
  changeDebounceMs?: number;
}

export class ProjectSession {
  private provider: FileProvider;
  private entryFile: string;
  private session: EditorSessionHandle;
  private readonly changes: FileChangeHub;
  private onExternalFileChange?: (path: string, content: string | null) => void;
  private onFileConflict?: (conflict: FileConflict) => void;
  private unsubscribeExternal?: () => void;
  private destroyed = false;
  private lastCompile: { generation: number; result: CompileResult } | null = null;

  constructor(options: ProjectSessionOptions) {
    this.provider = options.provider;
    this.entryFile = options.entryFile;
    this.session = options.session ?? new EditorSessionHandle();
    this.onExternalFileChange = options.onExternalFileChange;
    this.onFileConflict = options.onFileConflict;
    this.changes = new FileChangeHub({
      getContent: (path) => (this.destroyed ? null : this.session.getFileSource(path)),
      onFlush: options.onFilesChanged,
      onFileConflict: options.onFileConflict,
      debounceMs: options.changeDebounceMs,
    });
  }

  /** Load all files from provider and resolve INCLUDEs. */
  async initialize(): Promise<void> {
    const files = await this.provider.listFiles();
    for (const file of files) {
      const content = await this.provider.readFile(file);
      this.session.updateFile(file, content);
      // Host-loaded content is the clean baseline: the project starts with
      // zero dirty files, and a no-op edit flush never reaches the host.
      this.changes.setBaseline(file, content);
    }

    await this.resolveIncludes();

    // Register external change callback if the provider supports it. Keep the
    // unsubscribe so destroy() can detach it — otherwise a later external change
    // would call into a freed wasm session (use-after-free).
    this.unsubscribeExternal = this.provider.onExternalChange?.((path, content) => {
      if (this.destroyed) return;

      // Guard against silent data loss (issue #320): if the host rewrites a
      // file the studio has an unsaved, divergent buffer for, overwriting the
      // wasm buffer + re-baselining would clobber the pending edit with no
      // recourse. Detect that BEFORE mutating anything.
      const conflict =
        content === null ? null : this.changes.detectExternalConflict(path, content);
      if (conflict !== null) {
        // SAFE DEFAULT: keep the editor buffer (no updateFile), do not
        // re-baseline (no applyExternal) — flag the path conflicted and hand
        // both versions to the host for reconciliation (Track V merge view).
        this.changes.markConflicted(path);
        this.onFileConflict?.(conflict);
        return;
      }

      if (content === null) {
        this.session.removeFile(path);
      } else {
        this.session.updateFile(path, content);
      }
      // No conflict (clean buffer, or buffer already equals disk): the host's
      // content is the new truth — re-baseline, supersede any pending
      // studio-side change for the path (no echo back to the host).
      this.changes.applyExternal(path, content);
      this.onExternalFileChange?.(path, content);
    });
  }

  /** Underlying wasm session. */
  getSession(): EditorSessionHandle {
    return this.session;
  }

  /** The entry file for compilation. */
  getEntryFile(): string {
    return this.entryFile;
  }

  /** Create a new file and add it to the session (`file.new`). Recorded as
   *  a "created" change — the host learns about the file's existence. */
  async addFile(path: string, content: string = ""): Promise<void> {
    await this.provider.createFile(path, content);
    this.session.updateFile(path, content);
    this.changes.record(path, "created");
  }

  /** Remove a file from the wasm session (does not delete from provider). */
  closeFile(path: string): void {
    this.session.removeFile(path);
  }

  /** Whether the provider can delete files (drives the binder's delete UI). */
  canDeleteFiles(): boolean {
    return this.provider.deleteFile !== undefined;
  }

  /** Delete a file: remove it from the provider and the wasm session, and
   *  record a "deleted" change so the host's mirror drops it too. Unlike
   *  {@link closeFile} (session-only eviction), this is a real deletion. The
   *  caller is responsible for snapshotting content first if undo is wanted
   *  and for closing any open views (see the store's `deleteFile`). */
  async deleteFile(path: string): Promise<void> {
    await this.provider.deleteFile?.(path);
    this.session.removeFile(path);
    this.changes.record(path, "deleted");
  }

  /** Whether files can be renamed/moved. True when the provider has an atomic
   *  rename, or can delete (so the create+delete fallback can drop the old
   *  file). Drives the binder's rename/move affordances. */
  canRenameFiles(): boolean {
    return this.provider.renameFile !== undefined || this.provider.deleteFile !== undefined;
  }

  /**
   * Rename/move a file, rewriting `INCLUDE` references. The session's rename op
   * (pure) computes the moved content + the referencing files' edits; this
   * applies them: writes the content under `newPath`, drops `oldPath`, and
   * rewrites referrers — recording created/deleted/modified so the host mirror
   * follows. Returns the referrer paths whose `INCLUDE`s were rewritten (so the
   * caller can refresh their views). Throws if the op fails (unknown source, or
   * `newPath` taken).
   */
  async renameFile(oldPath: string, newPath: string): Promise<string[]> {
    if (oldPath === newPath) return [];
    const result = this.session.renameFile(oldPath, newPath);
    if (!result.ok) {
      throw new Error(result.error ?? `cannot rename ${oldPath}`);
    }
    const newSource = result.new_source ?? this.session.getFileSource(oldPath) ?? "";

    // Session: add the moved file under its new key, drop the old one.
    this.session.updateFile(newPath, newSource);
    this.session.removeFile(oldPath);

    // Cross-file INCLUDE rewrites — through the shared apply-edits seam.
    const referrers: string[] = [];
    for (const edit of result.cross_file_edits) {
      this.applyEdit(edit.path, edit.new_source);
      referrers.push(edit.path);
    }

    // Provider: atomic rename, or create-new + delete-old fallback.
    if (this.provider.renameFile) {
      await this.provider.renameFile(oldPath, newPath);
    } else {
      await this.provider.createFile(newPath, newSource);
      await this.provider.deleteFile?.(oldPath);
    }

    // Host egress for the moved file itself.
    this.changes.record(newPath, "created");
    this.changes.record(oldPath, "deleted");

    return referrers;
  }

  /**
   * Compile the project from its entry file. Cached against the session's
   * mutation generation: with several live views each compiling on their own
   * debounce, only the first compile after a change does real work.
   */
  compileProject(): CompileResult {
    const generation = this.session.generation;
    if (this.lastCompile !== null && this.lastCompile.generation === generation) {
      return this.lastCompile.result;
    }
    const result = this.session.compileProject(this.entryFile);
    this.lastCompile = { generation, result };
    return result;
  }

  /**
   * Report that `path`'s session content changed: provider write-back plus
   * a "modified" record on the change hub (host egress). Every mutation
   * path lands here — the CM6 edit flush calls it directly; bulk edits go
   * through {@link applyEdit}. No-op changes (content equal to the host
   * baseline) are dropped by the hub.
   */
  notifyFileChanged(path: string): void {
    const source = this.session.getFileSource(path);
    if (source !== null) {
      this.provider.onFileChanged?.(path, source);
    }
    this.changes.record(path, "modified");
  }

  /**
   * The shared apply-edits helper (issue #137): rewrite a file's session
   * content AND report it. Bulk edit paths (binder structural ops, search
   * replace, binder undo) MUST use this instead of raw `updateFile` so the
   * provider write-back and the host egress callback always see them.
   */
  applyEdit(path: string, newSource: string): void {
    this.session.updateFile(path, newSource);
    this.notifyFileChanged(path);
  }

  // ── Host egress (issue #154) ─────────────────────────────────────

  /** Deliver pending change notifications to the host now (save commands,
   *  unmount) instead of waiting for the debounce. */
  flushFileChanges(): FileChange[] {
    return this.changes.flush();
  }

  /** Re-baseline `paths` to their current content (explicit save). */
  markFilesSaved(paths: Iterable<string>): void {
    this.changes.markSaved(paths);
  }

  /** Re-baseline every session file (file.saveAll). */
  markAllSaved(): void {
    this.changes.markSaved(this.session.listFiles().map((f) => f.path));
  }

  /** Snapshot of every session file's current content, by path (sorted). */
  getFiles(): Record<string, string> {
    const files: Record<string, string> = {};
    const paths = this.session
      .listFiles()
      .map((f) => f.path)
      .sort();
    for (const path of paths) {
      const source = this.session.getFileSource(path);
      if (source !== null) files[path] = source;
    }
    return files;
  }

  /** Paths whose content diverges from the last-saved/notified baseline. */
  dirtyPaths(): string[] {
    return this.changes.dirtyPaths();
  }

  /** Paths whose dirty buffer collided with an external change and was kept,
   *  not yet reconciled (issue #320). */
  conflictedPaths(): string[] {
    return this.changes.conflictedPaths();
  }

  /** Observe the dirty-file count (drives the public-state summary). */
  setDirtyListener(listener: ((dirtyCount: number) => void) | undefined): void {
    this.changes.setDirtyListener(listener);
  }

  /**
   * Re-resolve INCLUDEs across all loaded files, loading missing files from
   * the provider — the next compile picks up newly discovered files.
   */
  async refreshIncludes(): Promise<void> {
    await this.resolveIncludes();
  }

  /** Request save via provider. */
  async save(): Promise<void> {
    await this.provider.requestSave?.();
  }

  /** Ask the provider for a file not yet in the session; loads it if found. */
  async requestFile(path: string): Promise<string | null> {
    const existing = this.session.getFileSource(path);
    if (existing !== null) return existing;
    const content = await this.provider.requestFile(path);
    if (content !== null) {
      this.session.updateFile(path, content);
      this.changes.setBaseline(path, content);
    }
    return content;
  }

  /** Tear down. Detaches the external-change listener before freeing the
   *  session so a late callback can't touch freed wasm memory. Pending
   *  change notifications must be flushed by the caller BEFORE destroy
   *  (mountStudio's unmount does) — destroy only cancels. */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.changes.dispose();
    this.unsubscribeExternal?.();
    this.unsubscribeExternal = undefined;
    this.session.free();
  }

  /** Resolve INCLUDEs across all loaded files, loading missing files from the provider. */
  private async resolveIncludes(): Promise<void> {
    const visited = new Set<string>();
    const queue = this.session.listFiles().map((f) => f.path);

    while (queue.length > 0) {
      const current = queue.shift()!;
      if (visited.has(current)) continue;
      visited.add(current);

      const includes = this.session.getFileIncludes(current);
      for (const inc of includes) {
        if (inc.loaded) {
          // Already in session — but still need to check its includes
          if (!visited.has(inc.resolved)) {
            queue.push(inc.resolved);
          }
          continue;
        }

        const content = await this.provider.requestFile(inc.resolved);
        if (content !== null) {
          this.session.updateFile(inc.resolved, content);
          // Provider-supplied content = host-synced = clean baseline.
          this.changes.setBaseline(inc.resolved, content);
          queue.push(inc.resolved);
        }
      }
    }
  }
}
