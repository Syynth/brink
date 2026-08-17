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
import { scheduleIdleWork, cancelIdleWork, type IdleHandle } from "./idle-schedule.js";

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
  /**
   * The project's entry file, used to seed `brink.toml` discovery (its
   * walk-up starts at this path's directory) and as the compile/initial-tab
   * entry UNTIL/UNLESS discovery finds a `brink.toml` naming a valid
   * `[project] entry` (issue #2331, ruled 2026-08-07 "`[project] entry`
   * beats `mountStudio`'s `entryFile`") — see {@link ProjectSession.getEntryFile}.
   * This argument is the fallback for a configless project; it is never
   * consulted again once a config-named entry supersedes it.
   */
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
  /**
   * The host's constructor-time `entryFile` argument — the fallback for a
   * configless project, and the seed for `discoverProjectConfig`'s walk-up.
   * Set once in the constructor and never mutated afterward (issue #2331
   * review finding): `applyProjectConfig` used to overwrite the equivalent
   * field with a config-named entry, which both defeated the "config wins
   * only while it says so" contract (deleting `entry` from `brink.toml` had
   * no way back to the host's own choice) and shifted the discovery seed
   * itself out from under the host on every subsequent re-discovery.
   */
  private readonly hostEntryFile: string;
  /**
   * The most recently discovered `[project] entry`, when it resolves to a
   * real file in this session — `null` when no `brink.toml` was found, it
   * doesn't set `entry`, or the named entry doesn't resolve. Wholesale
   * replaced on every `applyProjectConfig` call (never merged with the
   * previous value), so removing `entry` from `brink.toml` genuinely clears
   * it and {@link getEntryFile} falls back to {@link hostEntryFile} again.
   */
  private configuredEntry: string | null = null;
  private session: EditorSessionHandle;
  private readonly changes: FileChangeHub;
  private onExternalFileChange?: (path: string, content: string | null) => void;
  private onFileConflict?: (conflict: FileConflict) => void;
  private onProjectConfigWarnings?: (warnings: string[]) => void;
  private onProjectConfigError?: (message: string) => void;
  private unsubscribeExternal?: () => void;
  private destroyed = false;
  private lastCompile: { generation: number; result: CompileResult } | null = null;
  /**
   * Idle handles this class has scheduled via {@link deferForGatedCall} —
   * today, `renameFile`'s yield below — that have not yet settled, mapped to
   * the `reject` function that lets {@link destroy} abort them (issue #2794).
   * The same freed-wasm discipline this class already applies elsewhere
   * (`destroy()`'s listener detach, `FileChangeHub.getContent`'s `destroyed`
   * check): one guard, meant to cover every current and future gated call
   * this class defers via `scheduleIdleWork`, not a `renameFile`-specific
   * patch. Before this existed, an unmount landing inside the ≤300ms idle
   * window left the scheduled callback to fire anyway and go on to call
   * `this.session.*` on a handle `destroy()` had already freed.
   */
  private readonly pendingIdleWork = new Map<IdleHandle, (reason: Error) => void>();

  constructor(options: ProjectSessionOptions) {
    this.provider = options.provider;
    this.hostEntryFile = options.entryFile;
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
   *
   * Also owns `[project] entry` precedence (issue #2331, ruled 2026-08-07
   * "`[project] entry` beats `mountStudio`'s `entryFile`"): a discovered
   * `brink.toml` naming an `entry` that resolves to a real file in this
   * session supersedes {@link hostEntryFile} — the host's constructor-time
   * `entryFile` argument is only the fallback for a configless project (no
   * `brink.toml`, or one that doesn't set `entry`). {@link configuredEntry}
   * is wholesale-replaced (not merged) on every call, so a `brink.toml` edit
   * that removes `entry` genuinely clears the supersession. This is the one
   * place that reconciles the two, so every caller of `getEntryFile()`/
   * `compileProject()` — and `mountStudio`'s initial-tab open, which reads
   * `getEntryFile()` after `initialize()` — automatically sees whichever
   * one wins. A config-named entry that does NOT resolve to a real project
   * file never supersedes anything ({@link configuredEntry} is cleared to
   * `null`) and is reported through the same
   * {@link ProjectSessionOptions.onProjectConfigWarnings} channel as every
   * other `brink.toml` misconfiguration — no new warning channel for this
   * one case.
   */
  private applyProjectConfig(): void {
    let warnings: string[];
    try {
      warnings = this.session.discoverProjectConfig(this.hostEntryFile);
    } catch (err) {
      this.onProjectConfigError?.(err instanceof Error ? err.message : String(err));
      return;
    }
    const configuredEntry = this.sessionConfiguredEntry();
    if (configuredEntry !== null && this.session.getFileSource(configuredEntry) !== null) {
      this.configuredEntry = configuredEntry;
    } else {
      if (configuredEntry !== null) {
        warnings = [
          ...warnings,
          `project.entry \`${configuredEntry}\` in brink.toml does not resolve to a project file (ignored)`,
        ];
      }
      this.configuredEntry = null;
    }
    this.onProjectConfigWarnings?.(warnings);
  }

  /**
   * Feature-detected wrapper around `session.getConfiguredEntry()` (issue
   * #2331 review finding): `session` is a public injection seam
   * (`ProjectSessionOptions.session`), and a pre-#2331 stub/handle has no
   * such method — calling it unguarded would throw out of `initialize()`
   * for any host stub that predates this feature. Same pattern as
   * {@link sessionIsReadOnly}.
   */
  private sessionConfiguredEntry(): string | null {
    return typeof this.session.getConfiguredEntry === "function"
      ? this.session.getConfiguredEntry()
      : null;
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

  /**
   * Yield to the next idle slot ahead of a gated wasm call this class
   * defers (today: {@link renameFile}'s call to `this.session.renameFile`),
   * the way every future gated call this class defers should too (issue
   * #2794). Unlike a bare `scheduleIdleWork` await, this:
   *
   *  - Rejects immediately, without scheduling anything, if the session is
   *    already destroyed when called (a caller invoking a gated method after
   *    `destroy()` — a caller bug, but one that must not reach a freed
   *    handle either).
   *  - Tracks the idle handle in {@link pendingIdleWork} so {@link destroy}
   *    can `cancelIdleWork` it — otherwise the browser/timer callback fires
   *    into a session that has already freed its wasm handle.
   *  - Rejects (rather than leaving the promise to hang forever) if
   *    `destroy()` runs while this is still waiting: the caller's `await`
   *    throws, so the code that would call `this.session.*` after the yield
   *    never runs. This mirrors `applyRename`'s existing `try`/`finally` in
   *    `binder.ts`, which already restores its own local state on any
   *    rejection — a caller that swallows this rejection with no
   *    catch/finally still won't touch the freed session, since the
   *    rejection prevents its own continuation from ever executing.
   */
  private deferForGatedCall(): Promise<void> {
    if (this.destroyed) {
      return Promise.reject(
        new Error("ProjectSession destroyed before a deferred gated call ran"),
      );
    }
    return new Promise<void>((resolve, reject) => {
      const handle = scheduleIdleWork(() => {
        this.pendingIdleWork.delete(handle);
        resolve();
      });
      this.pendingIdleWork.set(handle, reject);
    });
  }

  /**
   * The project's entry file — for compilation, and (via `mountStudio`,
   * read after `initialize()`) the initial tab. This is the constructor's
   * `entryFile` option ({@link hostEntryFile}) UNLESS `applyProjectConfig`
   * found a `brink.toml` naming a valid `[project] entry` (issue #2331,
   * ruled 2026-08-07), which supersedes it; see that method's doc for the
   * full precedence rule. Never sticky past the config that set it — see
   * {@link configuredEntry}.
   */
  getEntryFile(): string {
    return this.configuredEntry ?? this.hostEntryFile;
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

  /**
   * Delete a file: remove it from the provider and the wasm session, and
   * record a "deleted" change so the host's mirror drops it too. Unlike
   * {@link closeFile} (session-only eviction), this is a real deletion. The
   * caller is responsible for snapshotting content first if undo is wanted
   * and for closing any open views (see the store's `deleteFile`).
   *
   * Refuses (no provider write, no session mutation) when `path` currently
   * resolves to a mounted stdlib copy (issue #2306/#2343): the Binder's
   * Library section offers no delete affordance, but `list_files` now
   * lists mounted files (the exact route this guard closes — a caller
   * reaching a mounted path outside the Binder's own gating must not
   * delete the mount, and definitely must not have the provider write a
   * "deletion" of a file it never wrote in the first place). Returns `false`
   * on refusal rather than throwing, matching {@link applyEdit}'s and
   * `EditorSession::remove_file`'s sibling contract — the store's
   * `deleteFilesWithUndo` awaits this with no try/catch, so a throw here
   * would leave a tab already closed by the caller with nothing telling the
   * user why the delete silently vanished (issue #2343 review finding).
   */
  async deleteFile(path: string): Promise<boolean> {
    if (this.sessionIsReadOnly(path)) {
      return false;
    }
    await this.provider.deleteFile?.(path);
    this.session.removeFile(path);
    this.changes.record(path, "deleted");
    // A deleted `brink.toml` (issue #2324) may uncover an ancestor
    // `brink.toml` discovery previously stopped short of (or find none,
    // which is not an error — see `applyProjectConfig`'s doc comment).
    if (isProjectConfigPath(path)) this.applyProjectConfig();
    return true;
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
   *
   * Off the paint path (#2776, generalizing #2767's `runGatedStructuralOp`
   * remedy — spec §7.7.4): `rename_file` runs the same op-agnostic breakage
   * gate as `moveStitch`/`promoteStitch`/`demoteKnot` (`gate_with_source`,
   * `crates/internal/brink-ide/src/file_rename.rs`) — an overlay re-analysis
   * of the whole project — so the wasm call below is deferred to the next
   * idle slot via `scheduleIdleWork` rather than run inline. This method
   * stays async either way, so every existing caller gets the deferral for
   * free; the synchronous busy-state commit a caller needs to paint BEFORE
   * this yields lives one layer up, in the caller that has store access
   * (`applyRename`, `packages/studio-store/src/slices/binder.ts`) — this
   * class has no UI-state concept of its own to commit one.
   */
  async renameFile(oldPath: string, newPath: string): Promise<string[]> {
    if (oldPath === newPath) return [];
    await this.deferForGatedCall();
    // PAINT-PATH-DEFERRED rename-file: gated (structural_result::gate_with_source
    // via crates/internal/brink-ide/src/file_rename.rs) — deferred by the
    // deferForGatedCall yield immediately above (#2776; destroy()-safe since
    // #2794 — see that method's doc comment).
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

    // Provider: atomic rename, or create-new + delete-old fallback. Both
    // branches hand over `newSource` — an atomic rename moves the file's
    // PRE-rewrite bytes, so a host that persisted only those would keep
    // stale outbound `INCLUDE` paths for any move that crossed a directory
    // boundary (#2425), while the fallback branch already wrote the
    // rewritten source through `createFile`.
    if (this.provider.renameFile) {
      await this.provider.renameFile(oldPath, newPath, newSource);
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
    const result = this.session.compileProject(this.getEntryFile());
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
  /** See the feature-detection note at the first call site. */
  private sessionIsReadOnly(path: string): boolean {
    return typeof this.session.isReadOnly === "function" && this.session.isReadOnly(path);
  }

  /**
   * Whether `path` currently resolves to a mounted stdlib copy (issue
   * #2306/#2343) — the public wrapper `DocumentSessions` reads to put a
   * mounted file's CM6 view into `EditorState.readOnly` (`document-sessions.ts`
   * `slotExtensions`), so a keystroke over the Binder's Library section
   * genuinely can't type rather than silently no-oping at the wasm layer.
   * Same feature-detected fallback as {@link sessionIsReadOnly}: `false` for
   * an injected session/stub that predates #2306.
   */
  isReadOnly(path: string): boolean {
    return this.sessionIsReadOnly(path);
  }

  notifyFileChanged(path: string): void {
    // Session-level read-only enforcement (issue #2306, ruled 2026-08-06
    // "Mounted stdlib presents as a read-only library node"): a still-
    // mounted path has no host baseline to diff against, so egressing it
    // here would persist the library's content into the host provider
    // (`InMemoryFileProvider.onFileChanged`) and record a false "modified"
    // change — forking the mount into the user's project with no actual
    // edit having been legitimately applied. The legitimate shadow path
    // (a real file replacing a mount) calls `session.updateFile` first,
    // which un-mounts the id before this method is ever reached for it.
    //
    // Feature-detected: `session` is a public injection seam
    // (`ProjectSessionOptions.session`) and pre-#2306 stubs/handles have
    // no `isReadOnly` — absent means "nothing is read-only", which is
    // exactly their world (only the real wasm handle mounts a stdlib).
    if (this.sessionIsReadOnly(path)) return;
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
   *
   * Session-level read-only enforcement (issue #2306, ruled 2026-08-06
   * "Mounted stdlib presents as a read-only library node"): refuses (no
   * write, no notify) when `path` currently resolves to a mounted stdlib
   * copy — the by-id route named in that ruling (project-wide search/
   * replace, or any future bulk caller not gated by `listFiles`) must not
   * be able to silently fork the library into the project. Returns whether
   * the edit actually applied, so a caller can surface the refusal instead
   * of assuming success.
   *
   * Deliberately NOT applied to `initialize()`/`addFile()`/the external-
   * change handler above, which call `session.updateFile` directly: those
   * are the host seeding real project content, including the legal case of
   * a real file deliberately shadowing a mounted stdlib key (see
   * `EditorSession::new`'s doc in `crates/brink-web/src/editor/mod.rs`) —
   * that must keep winning by construction-time ordering, not be rejected
   * because the id is still (momentarily) mounted at call time.
   */
  applyEdit(path: string, newSource: string): boolean {
    if (this.sessionIsReadOnly(path)) return false;
    this.session.updateFile(path, newSource);
    this.notifyFileChanged(path);
    return true;
  }

  // ── Host egress (issue #154) ─────────────────────────────────────

  /** Deliver pending change notifications to the host now (save commands,
   *  unmount) instead of waiting for the debounce. */
  flushFileChanges(): FileChange[] {
    return this.changes.flush();
  }

  /** Re-baseline `paths` to their current content (explicit save).
   *
   *  ⚠ Callers must read the content that CONFIRMS what the write persisted
   *  and call this in ONE synchronous step — no `await` between them, or an
   *  edit landing in that window is retired without ever having been
   *  written (docs/embedder-api.md "Dirty state", "Confirm and retire in
   *  ONE synchronous step"; pinned for every save path by
   *  packages/brink-studio/src/__tests__/save-retire-invariant.test.ts). */
  markFilesSaved(paths: Iterable<string>): void {
    this.changes.markSaved(paths);
  }

  /** Re-baseline every session file (file.saveAll). Excludes mounted stdlib
   *  files (issue #2306/#2343): the Library section has no save affordance
   *  and a mounted path never gets a dirty baseline in the first place
   *  (`notifyFileChanged`/`applyEdit` refuse it), but `listFiles()` now
   *  lists it alongside real files (#2343's flag flip) — filtering here
   *  keeps this method's own contract ("re-baseline every session file")
   *  from silently growing to include files that were never dirty.
   *
   *  ⚠ Unconditional re-baseline — no confirming read at all, so it is MORE
   *  dangerous than `markFilesSaved` if a future caller ever reaches it
   *  after an `await`. Only safe today because its one caller
   *  (`file-commands.ts`'s no-host-save branch) is fully synchronous — there
   *  is no write to await, so nothing can move on first. A new async save
   *  path must not call this directly; it needs the same confirm-then-retire
   *  discipline as `markFilesSaved` (docs/embedder-api.md "Dirty state",
   *  "Confirm and retire in ONE synchronous step"; pinned for every save
   *  path by
   *  packages/brink-studio/src/__tests__/save-retire-invariant.test.ts). */
  markAllSaved(): void {
    this.changes.markSaved(
      this.session
        .listFiles()
        .filter((f) => !f.mounted)
        .map((f) => f.path),
    );
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

  /** Whether `path` was deleted externally while a kept editor buffer for it
   *  survives, not yet recreated by a save or an external re-creation (issue
   *  #2371, "External deletion of an open file: keep the view, mark
   *  orphaned"). */
  isOrphaned(path: string): boolean {
    return this.changes.isOrphaned(path);
  }

  /** Sorted paths flagged orphaned (issue #2371) — for tab badging. */
  orphanedPaths(): string[] {
    return this.changes.orphanedPaths();
  }

  /**
   * Recreate `path` in the wasm session from a kept editor buffer after an
   * external deletion (issue #2371) — `DocumentSessions.markOrphaned`'s only
   * call site, and the point at which a kept buffer is first confirmed to
   * survive. Unlike {@link applyEdit}, this deliberately does NOT notify the
   * provider yet, and does NOT go through `record()`/`notifyFileChanged`:
   * `changes.noteOrphanRecreated` flags the path orphaned (no earlier call
   * site knows a buffer exists) and marks it dirty (no baseline — the
   * existing `FileChangeHub` rule) so the badge, dirty indicator, and IDE
   * queries are all correct immediately, WITHOUT enqueuing a pending change
   * or arming the flush debounce — a debounced delivery here would
   * "save" the recreated buffer on a timer under a write-through contract,
   * not on an actual ⌘S. `provider.onFileChanged` — the step that actually
   * stages/persists content, depending on the provider — fires only from the
   * next real `notifyFileChanged`, which a save always triggers
   * (`DocumentSessions.flushSlot` calls it unconditionally, whether or not
   * the buffer was edited since the deletion). That keeps "⌘S recreates the
   * file" literally true even for a provider whose `onFileChanged` IS its
   * persistence (`InMemoryFileProvider`'s playground contract) — recreating
   * eagerly here would resurrect the file the moment the deletion is
   * detected, before any save.
   */
  recreateOrphaned(path: string, content: string): void {
    if (this.sessionIsReadOnly(path)) return;
    this.session.updateFile(path, content);
    this.changes.noteOrphanRecreated(path);
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

  /**
   * Read `path` straight from the provider, bypassing session state
   * entirely — the provider's own account of what is actually persisted
   * (disk, for a host-save provider). Existing {@link FileProvider.readFile}
   * plumbing; this just exposes it past `ProjectSession`.
   *
   * The save commands use this to confirm what a host write actually wrote
   * when a path's content no longer matches the snapshot taken before the
   * save started (issue #2435): with `requestSave` calls serialized
   * (`TauriFileProvider`, #2403), a write queued behind another in-flight
   * one can legitimately pick up a later edit by the time it actually runs
   * and persist content newer than that snapshot — a case this lets the
   * caller tell apart from a genuine mid-write divergence (issue #2426)
   * without weakening that guard: a divergence still fails this check,
   * since disk keeps the pre-race content the write actually persisted.
   * Rejects like {@link FileProvider.readFile} itself (e.g. a vanished path).
   *
   * This confirmation is only meaningful if the underlying
   * {@link FileProvider.readFile} reports PERSISTED content, never content a
   * `requestSave` merely staged — a provider whose `readFile` mirrors
   * in-flight edits (see that method's doc) makes every call here vacuously
   * match, silently turning the #2426 guard into a no-op.
   */
  async readProviderFile(path: string): Promise<string> {
    return this.provider.readFile(path);
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
   *  (mountStudio's unmount does) — destroy only cancels.
   *
   *  Also aborts every gated call still waiting on its {@link
   *  deferForGatedCall} yield (issue #2794): each pending idle handle is
   *  cancelled (so the browser/timer callback never fires against this
   *  now-freed session) and its caller's `await` is rejected (so the code
   *  that would call `this.session.*` after the yield never runs) — before
   *  `this.session.free()` below. */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.changes.dispose();
    this.unsubscribeExternal?.();
    this.unsubscribeExternal = undefined;
    for (const [handle, reject] of this.pendingIdleWork) {
      cancelIdleWork(handle);
      reject(new Error("ProjectSession destroyed while a gated call was deferred"));
    }
    this.pendingIdleWork.clear();
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
