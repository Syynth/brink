/**
 * FileProvider — async host-owned file I/O interface.
 *
 * Different environments (web/localStorage, Tauri/FS, s92-studio) implement
 * this interface to plug in file management through a single async abstraction.
 */

export interface FileProvider {
  /** List all files known to the provider. */
  listFiles(): Promise<string[]>;

  /** Read a file's content by path. Throws if the file does not exist. */
  readFile(path: string): Promise<string>;

  /** Request a file that is not yet loaded (e.g. discovered via INCLUDE).
   *  Returns the content if the provider can supply it, or null otherwise. */
  requestFile(path: string): Promise<string | null>;

  /** Called when the editor changes a file's content. */
  onFileChanged?(path: string, content: string): void;

  /** Register a callback for external file changes (e.g. filesystem watcher).
   *  Content is null when the file was deleted. Returns an unsubscribe function
   *  the consumer MUST call on teardown, so the callback can't fire into a
   *  freed session. */
  onExternalChange?(callback: (path: string, content: string | null) => void): () => void;

  /** Create a new file at the given path. */
  createFile(path: string, content: string): Promise<void>;

  /** Delete a file. Optional — hosts that can't remove files (or don't want
   *  the studio to) simply omit it, and the studio hides its delete UI. Kept
   *  optional (unlike `createFile`) so adding delete doesn't break existing
   *  provider implementations. */
  deleteFile?(path: string): Promise<void>;

  /** Rename/move a file (its key changes; content is supplied separately via
   *  the session). Optional — when absent, ProjectSession falls back to
   *  `createFile(new)` + `deleteFile(old)`. A host with an atomic rename (or
   *  that must preserve history) implements this instead. */
  renameFile?(oldPath: string, newPath: string): Promise<void>;

  /** Request a canonical save of the current project state. When `paths`
   *  is given, the host may narrow the write to those files (the
   *  `file.save` single-file command passes the focused path); absent
   *  means save everything outstanding (`file.saveAll`, autosave). Under
   *  the overlay contract (`egressPersists: false`) this is THE canonical
   *  write — the save commands await it and only re-baseline on success,
   *  so a rejection keeps the files dirty for retry. */
  requestSave?(paths?: string[]): Promise<void>;
}

/**
 * In-memory file provider — stores files in a Map.
 * Useful for the web playground where there is no real filesystem.
 */
export class InMemoryFileProvider implements FileProvider {
  private files: Map<string, string>;
  /** External-change subscribers (filesystem-watcher analogue). */
  private externalListeners = new Set<(path: string, content: string | null) => void>();

  constructor(initialFiles?: Record<string, string>) {
    this.files = new Map(
      initialFiles ? Object.entries(initialFiles) : [],
    );
  }

  async listFiles(): Promise<string[]> {
    return [...this.files.keys()];
  }

  async readFile(path: string): Promise<string> {
    const content = this.files.get(path);
    if (content === undefined) {
      throw new Error(`File not found: ${path}`);
    }
    return content;
  }

  async requestFile(_path: string): Promise<string | null> {
    return this.files.get(_path) ?? null;
  }

  async createFile(path: string, content: string): Promise<void> {
    this.files.set(path, content);
  }

  async deleteFile(path: string): Promise<void> {
    this.files.delete(path);
  }

  async renameFile(oldPath: string, newPath: string): Promise<void> {
    const content = this.files.get(oldPath);
    if (content !== undefined) {
      this.files.set(newPath, content);
      this.files.delete(oldPath);
    }
  }

  onFileChanged(path: string, content: string): void {
    this.files.set(path, content);
  }

  onExternalChange(
    callback: (path: string, content: string | null) => void,
  ): () => void {
    this.externalListeners.add(callback);
    return () => this.externalListeners.delete(callback);
  }

  /**
   * Simulate an external (on-disk) change to `path`, as a filesystem watcher
   * would report it — updates the backing store and notifies subscribers
   * (content `null` deletes). The studio's external-change handler then runs
   * its conflict detection (issue #320). Exercised by the playground's
   * dev/e2e hook to drive the Track V merge view without a real filesystem.
   */
  pushExternalChange(path: string, content: string | null): void {
    if (content === null) {
      this.files.delete(path);
    } else {
      this.files.set(path, content);
    }
    for (const listener of this.externalListeners) listener(path, content);
  }
}
