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
 * ## Persistence model (D1)
 *
 * - **Structural ops are disk-immediate**: `createFile` / `deleteFile` /
 *   `renameFile` are called directly by `ProjectSession` on binder
 *   operations and write through to disk at once.
 * - **Content edits persist via the #154 egress**: `mountStudio`'s
 *   `onFilesChanged` delivers full-content batches (debounced ~500 ms,
 *   flushed immediately on `file.save`/`file.saveAll` and on unmount) and
 *   the shell writes each batch to disk (`writeChanges`). This is
 *   write-through-with-debounce rather than the spec's strict
 *   explicit-save model — a deliberate D1 simplification, recorded in the
 *   spec's D-stage notes: the egress is the one delivery channel that is
 *   reliable today, and writing it eagerly can never lose work. D2
 *   revisits explicit-save once the shell owns dirty-state UI.
 * - `requestSave` flushes anything staged through `onFileChanged` as a
 *   belt-and-braces path; with the egress wired it is normally a no-op.
 *
 * `onExternalChange` is deliberately absent in D1 (no fs watcher yet) —
 * `ProjectSession` treats an absent subscription as "no external changes",
 * and the #320 conflict surface stays dormant until D2 wires the watcher.
 */

import { invoke } from "@tauri-apps/api/core";
import type { FileChange, FileProvider } from "@brink-lang/editor";

export class TauriFileProvider implements FileProvider {
  /** Latest editor content per path, staged by `onFileChanged`. */
  private staged = new Map<string, string>();

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
      await invoke("write_file", { root: this.root, rel, content });
      this.staged.delete(rel);
    }
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
