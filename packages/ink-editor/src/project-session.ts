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

// The config filename `discoverProjectConfig`'s walk-up looks for (mirrors
// `brink_project_config::CONFIG_FILE_NAME` — see crates/internal/brink-project-config/src/lib.rs).
const PROJECT_CONFIG_FILENAME = "brink.toml";

/** True when `path`'s basename is the project-config filename, wherever in
 *  the tree it sits — the trigger for re-running `discoverProjectConfig`. */
function isProjectConfigPath(path: string): boolean {
  const slash = path.lastIndexOf("/");
  const base = slash >= 0 ? path.slice(slash + 1) : path;
  return base === PROJECT_CONFIG_FILENAME;
}

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
  /**
   * Whether `onFilesChanged` delivery counts as persistence (default
   * `true`, the write-through contract). Overlay hosts — whose egress
   * handler feeds a **backup ring** rather than canonical storage (the
   * celeris file model; brink-desktop D2) — set `false`: batches still
   * deliver, but dirty means "diverges from the last canonical save" and
   * only `markFilesSaved`/`markAllSaved` clears it. See
   * {@link FileChangeHubOptions.deliveryPersists}.
   */
  egressPersists?: boolean;
  /**
   * Unrecognized-key/lint-code warnings from the most recent `brink.toml`
   * discovery/apply (issue #2324) — forwarded verbatim from
   * `EditorSessionHandle.discoverProjectConfig`'s return value. Fires once
   * after `initialize()` loads the project's files (even with an empty
   * array — a host that wants to clear a previous warning list can rely on
   * that), and again every time a `brink.toml` in the session is created,
   * edited, renamed into/out of, or externally rewritten. Never fires for a
   * discovery error (malformed TOML / an invalid recognized-key value) — see
   * {@link onProjectConfigError} instead.
   */
  onProjectConfigWarnings?: (warnings: string[]) => void;
  /**
   * A `brink.toml` discovery/apply error (issue #2324): `discoverProjectConfig`
   * throws on malformed TOML or a recognized key with an invalid value (e.g.
   * `dialect = "brnik"`). Without this callback such an error would otherwise
   * propagate out of whichever call triggered discovery — `initialize()`,
   * `notifyFileChanged`/`applyEdit`, `addFile`, `deleteFile`, `renameFile`, or
   * the external-change handler — and a mid-edit typo in the one file this
   * feature exists to make effective would take the whole session down with
   * it. `applyProjectConfig` catches the throw at its single call site and
   * reports it here instead; the file's *previous* successfully-applied
   * config (if any) stays in effect until a valid edit re-discovers it.
   */
  onProjectConfigError?: (message: string) => void;
}

export class ProjectSession {
  private provider: FileProvider;
  private entryFile: string;
  private session: EditorSessionHandle;
  private readonly changes: FileChangeHub;
  private onExternalFileChange?: (path: string, content: string | null) => void;
  private onFileConflict?: (conflict: FileConflict) => void;
  private onProjectConfigWarnings?: (warnings: string[]) => void;
  private onProjectConfigError?: (message: string) => void;
  private unsubscribeExternal?: () => void;
  private destroyed = false;
  private lastCompile: { generation: number; result: CompileResult } | null = null;

  constructor(options: ProjectSessionOptions) {
    this.provider = options.provider;
    this.entryFile = options.entryFile;
    this.session = options.session ?? new EditorSessionHandle();
    this.onExternalFileChange = options.onExternalFileChange;
    this.onFileConflict = options.onFileConflict;
    this.onProjectConfigWarnings = options.onProjectConfigWarnings;
    this.onProjectConfigError = options.onProjectConfigError;
    this.changes = new FileChangeHub({
      getContent: (path) => (this.destroyed ? null : this.session.getFileSource(path)),
      onFlush: options.onFilesChanged,
      onFileConflict: options.onFileConflict,
      debounceMs: options.changeDebounceMs,
      deliveryPersists: options.egressPersists,
    });
  }

  /**
   * Re-run `brink.toml` discovery (issue #2324) and forward any warnings.
   * Uses `discoverProjectConfig` (#1414), not `applyProjectConfig` (#1005):
   * it walks the session's own already-loaded documents from `entryFile`
   * up to the tree root, so this class — which already loads every
   * provider file (including `brink.toml`, an ordinary project file) into
   * the session — needs no host-specific directory-walk/read code of its
   * own to locate or read the text. `applyProjectConfig` would require
   * this class (or its caller) to separately fetch the file's text through
   * the `FileProvider`, duplicating work `initialize()` already did.
   *
   * Safe to call whenever `brink.toml` might have changed — a missing file
   * is not an error (`discoverProjectConfig` returns `[]`), and a
   * recognized-key/lint-code warning list is forwarded even when empty.
   *
   * `discoverProjectConfig` throws on malformed TOML or a recognized key
   * with an invalid value (issue #2324's review finding): every caller of
   * this method — `initialize()`, `notifyFileChanged`/`applyEdit`,
   * `addFile`, `deleteFile`, `renameFile`, and the external-change handler —
   * is a place a mid-edit typo in `brink.toml` could otherwise take down,
   * from a mount-time failure with no editor to fix the file in, to an
   * uncaught exception on every subsequent keystroke. Caught here, once, at
   * the single call site all of them share, and reported through
   * {@link ProjectSessionOptions.onProjectConfigError} instead of
   * rethrowing.
   */
  private applyProjectConfig(): void {
    let warnings: string[];
    try {
      warnings = this.session.discoverProjectConfig(this.entryFile);
    } catch (err) {
      this.onProjectConfigError?.(err instanceof Error ? err.message : String(err));
      return;
    }
    this.onProjectConfigWarnings?.(warnings);
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

    // `brink.toml` (issue #2324): every project file is loaded above, so
    // discovery can run once, right here, before anything analyzes/compiles.
    this.applyProjectConfig();

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
      // `brink.toml` rewritten from outside the studio (issue #2324): the
      // file just landed in the session via `updateFile` above — re-run
      // discovery so an external edit is not silently ignored either.
      if (isProjectConfigPath(path)) this.applyProjectConfig();
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
    // A `brink.toml` created after mount (issue #2324) was previously
    // undiscoverable — the file wasn't there for `initialize()`'s discovery
    // call, and nothing re-ran it.
    if (isProjectConfigPath(path)) this.applyProjectConfig();
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
    // A deleted `brink.toml` (issue #2324) may uncover an ancestor
    // `brink.toml` discovery previously stopped short of (or find none,
    // which is not an error — see `applyProjectConfig`'s doc comment).
    if (isProjectConfigPath(path)) this.applyProjectConfig();
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

    // `brink.toml` moved into or out of the tree (issue #2324): the
    // ancestor `brink.toml` discovery finds by walk-up depends on exact
    // paths, so either direction can change what's discovered.
    if (isProjectConfigPath(oldPath) || isProjectConfigPath(newPath)) {
      this.applyProjectConfig();
    }

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
    // `brink.toml` edited in the studio (issue #2324) — CM6 edits (this is
    // the direct caller) and every bulk-edit path (through {@link applyEdit},
    // which calls this) both land here. The session's content for `path` is
    // already live by this point, so discovery picks up the new text.
    if (isProjectConfigPath(path)) this.applyProjectConfig();
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

  /** Whether `path` has a kept-but-unreconciled external conflict (#320). */
  hasConflict(path: string): boolean {
    return this.changes.isConflicted(path);
  }

  /**
   * Resolve an external conflict (issue #320, Track V) by taking the host's
   * on-disk content: overwrite the session buffer with `disk`, re-baseline to
   * it, and clear the conflict flag (the path goes clean). This is the
   * "Use disk" merge action — the studio's dirty edit is discarded in favor
   * of what landed on disk.
   */
  resolveConflictUseDisk(path: string, disk: string): void {
    this.session.updateFile(path, disk);
    // applyExternal re-baselines to `disk` and clears the conflict flag.
    this.changes.applyExternal(path, disk);
  }

  /**
   * Resolve an external conflict (issue #320, Track V) by KEEPING the studio
   * buffer: clear the conflict flag without touching the buffer or baseline.
   * The path stays dirty — the kept edit still diverges from disk and is
   * re-delivered on the next flush/save. This is the "Keep mine" merge action.
   */
  resolveConflictKeepMine(path: string): void {
    this.changes.clearConflict(path);
  }

  /**
   * Resolve an external conflict (issue #320, Track V) with a hand-merged
   * result: write `merged` through the shared apply-edits seam (so the host
   * egress + provider write-back see it) and clear the conflict flag. The
   * merged text becomes the new dirty buffer over the still-unchanged
   * baseline, so the user can save it normally.
   */
  resolveConflictMerged(path: string, merged: string): void {
    this.applyEdit(path, merged);
    this.changes.clearConflict(path);
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

  /** Request a canonical save via the provider (optionally narrowed to
   *  `paths` — see {@link FileProvider.requestSave}). Rejections propagate:
   *  the save commands rely on that to keep files dirty when the host's
   *  write fails. */
  async save(paths?: string[]): Promise<void> {
    await this.provider.requestSave?.(paths);
  }

  /** Whether the provider implements a host-side canonical save. The save
   *  commands branch on this: with a host save they await it and only
   *  re-baseline on success; without one (the standalone playground) the
   *  flush-and-re-baseline path runs synchronously as it always has. */
  hasHostSave(): boolean {
    return this.provider.requestSave !== undefined;
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
