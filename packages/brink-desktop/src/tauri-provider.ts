/**
 * TauriFileProvider — the desktop shell's `FileProvider` implementation
 * (docs/desktop-shell-spec.md, "The one new component").
 *
 * All I/O goes through custom Tauri commands in `src-tauri/src/lib.rs`
 * rather than the fs plugin: the shell owns filesystem policy (the
 * stay-inside-the-project-root guard lives in Rust, next to the I/O it
 * guards), and no plugin scope wrangling is needed for a user-picked
 * folder. Provider keys are project-relative `/`-separated paths — the
 * studio's convention; absolute OS paths never leak into the session.
 *
 * ## Persistence model (D2 — the overlay contract, 2026-08-07 ruling)
 *
 * - **Structural ops are disk-immediate**: `createFile` / `deleteFile` /
 *   `renameFile` are called directly by `ProjectSession` on binder
 *   operations and write through to disk at once. For `renameFile` that
 *   means two steps, not one: the native atomic move, then a write of the
 *   moved file's own `INCLUDE`-rewritten source at its new path (#2425) —
 *   the move carries bytes that a cross-directory rename has already made
 *   stale.
 * - **Content edits stage** (`onFileChanged` → `staged`); the #154 egress
 *   feeds the bounded **backup ring** (`ringBackups` → `append_backups`),
 *   which is crash protection, orthogonal to dirty.
 * - **`requestSave` is THE canonical write**: the save commands await it
 *   (⌘S narrowed to the focused path, saveAll/autosave unnarrowed) and
 *   only re-baseline on success — a rejected write stays staged and dirty
 *   for retry. Calls are serialized (#2403) so an autosave tick and a
 *   saveAll (including the quit-time call) queue behind each other instead
 *   of racing writes to the same file.
 * - **`onExternalChange` is a real fs watcher** (shell `start_watch`,
 *   debounced + filtered): events forward into `ProjectSession`'s #320
 *   never-clobber machinery, with self-write, self-delete (#2404), and
 *   self-rename (#2416) suppression so our own canonical writes, deletions,
 *   and renames never masquerade as external changes (which, for a deletion
 *   or a rename's old-path echo, would also wrongly drop the pending
 *   "deleted"/"created" egress record a host mirror relies on). At most one
 *   of the three markers is armed per path: every arming site reconciles the
 *   other two kinds (#2424, see {@link TauriFileProvider.armSelfWrite}), so a
 *   marker the watcher's coalescing left unconsumed can never survive to
 *   swallow a genuinely external change.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { FileChange, FileProvider } from "@brink-lang/editor";

export class TauriFileProvider implements FileProvider {
  /** Latest editor content per path, staged by `onFileChanged`. */
  private staged = new Map<string, string>();
  /**
   * Content of our own recent canonical writes, for watcher self-write
   * suppression: a save triggers an fs event for the file we just wrote,
   * and without suppression a buffer that moved on during the write's
   * flight would false-positive the #320 conflict surface against our own
   * save. An event whose content matches the recorded self-write is
   * swallowed; anything else is genuinely external.
   */
  private selfWrites = new Map<string, string>();
  /**
   * Paths this provider itself just deleted, for watcher self-delete
   * suppression (#2404): `deleteFile` writes through to disk, which triggers
   * an fs event for the same deletion. Unlike a self-write there is no
   * content to compare, so a path is enough — the first echoed deletion for
   * a path we just deleted is ours; anything else is genuinely external. A
   * path is consumed (removed) the moment its echo is swallowed, so a later,
   * independent external deletion of the same path is not mistaken for a
   * stale marker.
   */
  private selfDeletes = new Set<string>();
  /**
   * Paths this provider itself just created as the destination of a
   * `renameFile` (#2416): the native `rename_file` command is atomic and
   * carries no content, so unlike `selfWrites` there is nothing to compare
   * against — a path is enough. The rename's watcher echo shows up as a
   * creation event for `newPath` (mirroring `selfDeletes`'s reasoning for
   * `oldPath`'s deletion echo), and without this marker `onExternalChange`
   * would call `applyExternal(newPath, content)`, wiping the pending
   * "created" egress record `renameFile`'s own `changes.record` just staged.
   * Consumed the moment its echo is swallowed, exactly like `selfDeletes`.
   */
  private selfCreates = new Set<string>();
  /**
   * Serializes `requestSave` (#2403): without this, the 2-minute autosave
   * ticker and a `saveAll` (including the quit-time call) can overlap on
   * `staged` and race each other's writes to the same file — one call's
   * snapshot-then-clear of `staged` interleaving with another's. Mirrors
   * `OverlayPersistence`'s `enqueue` (`packages/ink-editor/src/persistence.ts`):
   * each call chains behind the last so overlapping callers queue instead of
   * racing. Desktop doesn't route through `OverlayPersistence` itself — saves
   * are driven by the studio's `file.save`/`file.saveAll` commands calling
   * `provider.requestSave` directly (see `main.tsx`), so the minimal fix is
   * the same serialization primitive applied at this provider's own save
   * entry point, rather than restructuring desktop's save wiring around a
   * coordinator it doesn't otherwise use.
   */
  private saving: Promise<unknown> = Promise.resolve();

  constructor(private readonly root: string) {}

  async listFiles(): Promise<string[]> {
    return invoke<string[]>("list_files", { root: this.root });
  }

  async readFile(path: string): Promise<string> {
    return invoke<string>("read_file", { root: this.root, rel: path });
  }

  async requestFile(path: string): Promise<string | null> {
    try {
      return await this.readFile(path);
    } catch {
      return null;
    }
  }

  onFileChanged(path: string, content: string): void {
    this.staged.set(path, content);
  }

  async createFile(path: string, content: string): Promise<void> {
    this.armSelfWrite(path, content);
    try {
      await invoke("write_file", { root: this.root, rel: path, content });
    } catch (e) {
      this.disarmSelfWrite(path, content);
      throw e;
    }
  }

  async deleteFile(path: string): Promise<void> {
    this.staged.delete(path);
    this.armSelfDelete(path);
    try {
      await invoke("delete_file", { root: this.root, rel: path });
    } catch (e) {
      // A failed delete must not leave a permanently armed marker that
      // silently swallows the next genuine external deletion of this path
      // (#2404 review).
      this.selfDeletes.delete(path);
      throw e;
    }
  }

  /**
   * Move `oldPath` to `newPath` through the native (atomic) `rename_file`,
   * then persist `newContent` there.
   *
   * The second half is not redundant (#2425): `rename_file` moves BYTES, but
   * `ProjectSession.renameFile`'s op folds the moved file's own outbound
   * `INCLUDE` rewrites into the source it hands over here (`new_source`,
   * `crates/brink-web/src/editor/refactor.rs`), so for any move that changes
   * the file's directory the moved bytes are already stale the moment they
   * land. Staging alone would not do: under the D2 contract structural ops
   * are disk-immediate, and until some later edit dirtied the file a
   * `brink compile`/CLI reader going straight to disk would compile the
   * moved file's own pre-rewrite `INCLUDE` paths. This write closes that gap
   * only for the moved file's own content — disk at `newPath` now agrees
   * with the session. A referrer's rewritten `INCLUDE` (a file that pointed
   * at `oldPath`) is a separate, ordinary edit that goes through
   * `ProjectSession.applyEdit` → `onFileChanged` → {@link staged} and lands
   * on disk only at the next `requestSave`, same as any other edit — so disk
   * can still disagree with the session for those referrer files in the
   * meantime. The write goes through the same staged-write path as a save,
   * so it queues behind any in-flight `requestSave` rather than racing it
   * (#2403) and, if it fails, the content stays in {@link staged} for the
   * next save to retry (see that catch block below for what it does NOT
   * change: this failure has no effect on studio dirty state, which was
   * already true the moment `ProjectSession.renameFile` recorded the move).
   *
   * `newContent` is optional purely for signature compatibility with the
   * `FileProvider` contract; `ProjectSession` always supplies it.
   */
  async renameFile(oldPath: string, newPath: string, newContent?: string): Promise<void> {
    const stagedContent = this.staged.get(oldPath);
    if (stagedContent !== undefined) {
      this.staged.delete(oldPath);
      this.staged.set(newPath, stagedContent);
    }
    // Arm markers for BOTH sides before the invoke (#2416, the same
    // disarm-on-rejection discipline PR #2412 added for `deleteFile`): the
    // native rename is atomic and produces two watcher echoes — a deletion
    // for `oldPath` and a creation for `newPath` — either of which would
    // otherwise reach `onExternalChange` and wipe the "deleted"/"created"
    // egress records `ProjectSession.renameFile` is about to record.
    this.armSelfDelete(oldPath);
    this.armSelfCreate(newPath);
    try {
      await invoke("rename_file", { root: this.root, from: oldPath, to: newPath });
    } catch (e) {
      // A failed rename must not leave either marker permanently armed,
      // silently swallowing the next genuine external change to either path.
      this.selfDeletes.delete(oldPath);
      this.selfCreates.delete(newPath);
      throw e;
    }
    if (newContent === undefined) return;
    // Written unconditionally rather than only when the source actually
    // changed: the provider cannot tell a rewritten source from an unchanged
    // one without re-reading the moved bytes, and "after a rename, disk at
    // the new path matches the session" is the simpler invariant to keep.
    // The re-keyed staged entry above is pre-rewrite text, so this overwrite
    // is also what stops a later save from putting it back.
    this.staged.set(newPath, newContent);
    try {
      await this.enqueueSave(() => this.writeStaged([newPath]));
    } catch (e) {
      // The move itself succeeded and the session already reflects it, so
      // rejecting here would leave `ProjectSession.renameFile` throwing past
      // its own "created"/"deleted" egress records for a rename that really
      // did happen. The content stays in `staged` instead, retried by the
      // next `requestSave`, exactly as a rejected save is — this failure
      // does not itself make the file "dirty": `ProjectSession.renameFile`'s
      // `record(newPath, "created")` already set studio dirty state the
      // moment the session updated, independent of whether this follow-up
      // write ever lands. And "the next `requestSave`" means the next
      // UNNARROWED one (the autosave ticker, `saveAll`) — a `file.save`
      // narrowed to a different, currently-focused path leaves this entry
      // in `staged` untouched, since `writeStaged` only writes the paths
      // it's given.
      //
      // `writeStaged`'s own failure path already disarmed the self-write
      // marker it armed (`disarmSelfWrite`), but the native rename already
      // landed and its creation echo for `newPath` is still outstanding —
      // carrying the pre-rewrite bytes the move put on disk, since this
      // write never reached it. Re-arm the self-create marker so that echo
      // is still suppressed instead of reaching `onExternalChange` and
      // wiping the pending "created" egress record for a rename that really
      // did happen (#2438 review).
      this.armSelfCreate(newPath);
      console.error("[brink-desktop] rename content write failed", e);
    }
  }

  /**
   * Arm the self-write marker for `path`, clearing any still-armed marker of
   * a DIFFERENT kind for the same path first (#2424).
   *
   * At most ONE self-echo marker stays armed per path. The shell's watcher
   * accumulates paths and flushes after a quiet window, reading content once
   * at flush time — at most one event per path per window — so a second
   * marker armed before the first echoed back can never be consumed. An
   * unconsumed marker is not inert: a leftover `selfDeletes` goes on to
   * swallow a genuinely external deletion of that path. Reconciling here, at
   * ARMING time, means the outcome no longer depends on which branch of
   * {@link onExternalChange} happens to check first.
   */
  private armSelfWrite(path: string, content: string): void {
    this.selfDeletes.delete(path);
    this.selfCreates.delete(path);
    this.selfWrites.set(path, content);
  }

  /**
   * Drop a self-write marker whose write never happened, so it cannot
   * swallow a later external change carrying that exact content (the
   * discipline #2412 gave `deleteFile`). Only clears the marker if it is
   * still the one this call armed — an operation that ran during the failed
   * write's flight may have armed a newer one for the same path.
   */
  private disarmSelfWrite(path: string, content: string): void {
    if (this.selfWrites.get(path) === content) {
      this.selfWrites.delete(path);
    }
  }

  /** Arm the self-delete marker for `path`. See {@link armSelfWrite} for why
   *  the other two kinds are cleared rather than left to check order. */
  private armSelfDelete(path: string): void {
    this.selfWrites.delete(path);
    this.selfCreates.delete(path);
    this.selfDeletes.add(path);
  }

  /** Arm the self-create marker for `path`. See {@link armSelfWrite} for why
   *  the other two kinds are cleared rather than left to check order. */
  private armSelfCreate(path: string): void {
    this.selfWrites.delete(path);
    this.selfDeletes.delete(path);
    this.selfCreates.add(path);
  }

  /**
   * THE canonical write under the overlay contract (D2): the save commands
   * await this and only re-baseline on success. `paths` narrows the write
   * (`file.save` passes the focused path); absent saves everything staged.
   * A staged entry is only dropped once its write succeeded — a rejected
   * write stays staged, the command reports the error, and the file stays
   * dirty for retry.
   *
   * Serialized (#2403): the 2-minute autosave ticker and a `saveAll`
   * (including the quit-time call, PR #2382) both call this, and without
   * serialization their snapshot-then-clear of `staged` can interleave and
   * race each other's writes to the same file. Each call is queued behind
   * the previous one via {@link enqueueSave}, exactly like
   * `OverlayPersistence.enqueue` — so overlapping callers run one after the
   * other against a consistent `staged` snapshot instead of racing.
   */
  async requestSave(paths?: string[]): Promise<void> {
    return this.enqueueSave(() => this.writeStaged(paths));
  }

  private async writeStaged(paths?: string[]): Promise<void> {
    const wanted = paths === undefined ? null : new Set(paths);
    const pending = [...this.staged.entries()].filter(
      ([rel]) => wanted === null || wanted.has(rel),
    );
    for (const [rel, content] of pending) {
      this.armSelfWrite(rel, content);
      try {
        await invoke("write_file", { root: this.root, rel, content });
      } catch (e) {
        this.disarmSelfWrite(rel, content);
        throw e;
      }
      // Only drop the staged entry if it still matches what we just wrote —
      // an edit staged while this write was in flight must survive so the
      // next requestSave() picks it up (#2403 review).
      if (this.staged.get(rel) === content) {
        this.staged.delete(rel);
      }
    }
  }

  /**
   * Chain `work` behind whatever `requestSave` call (if any) is already in
   * flight, so overlapping callers queue rather than race. Mirrors
   * `OverlayPersistence.enqueue` (`packages/ink-editor/src/persistence.ts`):
   * a rejected save must not wedge the queue for the next caller, so the
   * chain link swallows its own rejection while still propagating it to the
   * caller that issued it.
   */
  private enqueueSave<T>(work: () => Promise<T>): Promise<T> {
    const next = this.saving.then(work, work);
    this.saving = next.catch(() => undefined);
    return next;
  }

  /**
   * Watch the project root (D2): shell fs events arrive as
   * `fs:external-change` `{ path, content|null }`, debounced and filtered
   * shell-side. Self-writes are swallowed here; everything else forwards
   * into `ProjectSession`'s #320 never-clobber machinery. The returned
   * unsubscribe stops the watch (the contract requires calling it on
   * teardown so a late event can't fire into a freed session).
   */
  onExternalChange(callback: (path: string, content: string | null) => void): () => void {
    let live = true;
    const unlistenPromise = listen<{ path: string; content: string | null }>(
      "fs:external-change",
      (event) => {
        if (!live) return;
        const { path, content } = event.payload;
        let suppressed = false;
        if (content === null) {
          if (this.selfDeletes.delete(path)) {
            suppressed = true; // our own deleteFile() or renameFile()'s old-path echoing back (#2404/#2416)
          }
        } else if (this.selfWrites.get(path) === content) {
          this.selfWrites.delete(path);
          suppressed = true; // our own canonical write echoing back
        } else if (this.selfCreates.delete(path)) {
          suppressed = true; // our own renameFile()'s new-path creation echoing back (#2416)
        }
        if (suppressed) {
          // The shell coalesces multiple events for the same path within one
          // debounce window into a single flushed event, so a path can pick
          // up a second marker before the first echoes back (e.g. rename A→B
          // then requestSave(B) inside the same window arms both selfCreates
          // and selfWrites for B, but only one B event ever arrives). Clear
          // all three markers for this path once any one of them consumes
          // the event, so a now-stale leftover marker can't later swallow a
          // genuinely external change at this path (#2421 review; a no-op
          // for the common, non-coalesced case since the other markers are
          // already unset).
          this.selfWrites.delete(path);
          this.selfDeletes.delete(path);
          this.selfCreates.delete(path);
          return;
        }
        callback(path, content);
      },
    );
    void invoke("start_watch", { root: this.root }).catch((e: unknown) => {
      console.error("[brink-desktop] start_watch failed", e);
    });
    return () => {
      live = false;
      void unlistenPromise.then((unlisten) => unlisten());
      void invoke("stop_watch").catch(() => {});
    };
  }

  /**
   * Feed a #154 egress batch to the BACKUP RING (D2 overlay model — the
   * egress is crash protection, not canonical persistence; canonical
   * writes happen in {@link requestSave}). Ring bounds are enforced in the
   * shell command, next to the storage.
   */
  async ringBackups(changes: FileChange[]): Promise<void> {
    const at = Date.now();
    const entries = changes
      .filter((c) => c.type !== "deleted" && c.content !== undefined)
      .map((c) => ({ path: c.path, content: c.content ?? "", at }));
    if (entries.length === 0) return;
    await invoke("append_backups", { root: this.root, entries });
  }
}

/** Open the native folder picker; null when the user cancels. Still used
 *  by the New Project dialog's Choose… (a new project genuinely starts
 *  from a folder — the dialog then creates the file anchors in it). */
export async function pickProjectFolder(): Promise<string | null> {
  return invoke<string | null>("pick_project_folder");
}

/** Open the native story-file picker (the Open… door, #3021): a `.ink`
 *  story or a `brink.toml`. Null when the user cancels. */
export async function pickProjectFile(): Promise<string | null> {
  return invoke<string | null>("pick_project_file");
}

/**
 * The governing `brink.toml` for an explicitly opened story file, found by
 * the compiler's own bounded walk-up (`brink-project-config` via the shell
 * command — never a same-directory approximation). Null when nothing
 * governs. See `DiscoveredProjectConfig`'s Rust twin in
 * `src-tauri/src/lib.rs` for field semantics.
 */
export interface DiscoveredProjectConfig {
  configPath: string;
  entry: string | null;
  openedIsEntry: boolean;
  walked: string[];
  warnings: string[];
}

export async function discoverProjectConfig(
  path: string,
): Promise<DiscoveredProjectConfig | null> {
  return invoke<DiscoveredProjectConfig | null>("discover_project_config", { path });
}

/**
 * Create a new project (#3012): the starter story at `entry` plus a
 * `brink.toml` naming it, in the EXISTING directory `dir`. Refuses to
 * overwrite. Resolves to the absolute path of the created `brink.toml` —
 * the anchor the caller opens, on the toml door.
 */
export async function createProject(dir: string, entry: string): Promise<string> {
  return invoke<string>("create_project", { dir, entry });
}

/**
 * Export Story (.inkb) (D3 slice 1, #2391): write already-compiled bytes
 * through a native save dialog. Returns the chosen path, or null if the
 * user cancelled. `bytes` crosses the IPC boundary as a plain number array
 * — the same encoding `CompileResult.story_bytes` already uses coming the
 * other way out of wasm, so no new (de)serialization convention is
 * introduced.
 */
export async function saveBytesDialog(
  defaultName: string,
  bytes: Uint8Array,
): Promise<string | null> {
  return invoke<string | null>("save_bytes_dialog", {
    defaultName,
    bytes: Array.from(bytes),
  });
}

/**
 * Recent projects (#2394, `docs/desktop-shell-spec.md` D2): a persisted,
 * most-recent-first, capped, deduplicated-by-path list backed by
 * `recents.json` in app-data. All three commands return the resulting
 * list so the caller can re-render without a second round trip; the shell
 * also keeps the native File → Open Recent submenu in sync with the same
 * list on every push/prune (see `rebuild_menu` in `src-tauri/src/lib.rs`).
 */
export async function readRecents(): Promise<string[]> {
  return invoke<string[]>("read_recents");
}

/** Record a successfully-opened project root. Call after every successful open. */
export async function pushRecent(root: string): Promise<string[]> {
  return invoke<string[]>("push_recent", { path: root });
}

/**
 * Lazily drop one path from the recents list. Call only when opening a
 * recent entry actually failed (e.g. its folder was deleted or moved) —
 * never as a proactive existence sweep.
 */
export async function pruneRecent(root: string): Promise<string[]> {
  return invoke<string[]>("prune_recent", { path: root });
}

/**
 * Whether a project ANCHOR still exists — a directory for a legacy folder
 * recent, a file for the two file doors (#3021). Gates lazy pruning
 * (#2394 review): `openProject` failing does not by itself mean the anchor
 * is gone — a transient `mountStudio` failure, a permission error, or a
 * file deleted mid-listing must never be conflated with a genuinely
 * missing anchor, or a valid entry gets silently deleted from
 * `recents.json` and the native Open Recent submenu over a recoverable
 * error.
 */
export async function projectAnchorExists(path: string): Promise<boolean> {
  return invoke<boolean>("project_anchor_exists", { path });
}
