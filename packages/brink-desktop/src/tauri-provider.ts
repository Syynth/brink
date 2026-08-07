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
 *   operations and write through to disk at once.
 * - **Content edits stage** (`onFileChanged` → `staged`); the #154 egress
 *   feeds the bounded **backup ring** (`ringBackups` → `append_backups`),
 *   which is crash protection, orthogonal to dirty.
 * - **`requestSave` is THE canonical write**: the save commands await it
 *   (⌘S narrowed to the focused path, saveAll/autosave unnarrowed) and
 *   only re-baseline on success — a rejected write stays staged and dirty
 *   for retry.
 * - **`onExternalChange` is a real fs watcher** (shell `start_watch`,
 *   debounced + filtered): events forward into `ProjectSession`'s #320
 *   never-clobber machinery, with self-write suppression so our own
 *   canonical writes never masquerade as conflicts.
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
    this.selfWrites.set(path, content);
    await invoke("write_file", { root: this.root, rel: path, content });
  }

  async deleteFile(path: string): Promise<void> {
    this.staged.delete(path);
    await invoke("delete_file", { root: this.root, rel: path });
  }

  async renameFile(oldPath: string, newPath: string): Promise<void> {
    const stagedContent = this.staged.get(oldPath);
    if (stagedContent !== undefined) {
      this.staged.delete(oldPath);
      this.staged.set(newPath, stagedContent);
    }
    await invoke("rename_file", { root: this.root, from: oldPath, to: newPath });
  }

  /**
   * THE canonical write under the overlay contract (D2): the save commands
   * await this and only re-baseline on success. `paths` narrows the write
   * (`file.save` passes the focused path); absent saves everything staged.
   * A staged entry is only dropped once its write succeeded — a rejected
   * write stays staged, the command reports the error, and the file stays
   * dirty for retry.
   */
  async requestSave(paths?: string[]): Promise<void> {
    const wanted = paths === undefined ? null : new Set(paths);
    const pending = [...this.staged.entries()].filter(
      ([rel]) => wanted === null || wanted.has(rel),
    );
    for (const [rel, content] of pending) {
      this.selfWrites.set(rel, content);
      await invoke("write_file", { root: this.root, rel, content });
      this.staged.delete(rel);
    }
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
        if (content !== null && this.selfWrites.get(path) === content) {
          this.selfWrites.delete(path);
          return; // our own canonical write echoing back
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

/** Open the native folder picker; null when the user cancels. */
export async function pickProjectFolder(): Promise<string | null> {
  return invoke<string | null>("pick_project_folder");
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
 * Whether a project root still exists as a directory (#2394 review). Gates
 * lazy pruning: `openProject` failing does not by itself mean the folder is
 * gone — a transient `mountStudio` failure, a permission error, or a file
 * deleted mid-listing must never be conflated with a genuinely missing
 * project root, or a valid entry gets silently deleted from `recents.json`
 * and the native Open Recent submenu over a recoverable error.
 */
export async function projectRootExists(root: string): Promise<boolean> {
  return invoke<boolean>("project_root_exists", { path: root });
}
