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
   *  Content is null when the file was deleted. */
  onExternalChange?(callback: (path: string, content: string | null) => void): void;

  /** Request save of the current project state. */
  requestSave?(): Promise<void>;
}

/**
 * In-memory file provider — stores files in a Map.
 * Useful for the web playground where there is no real filesystem.
 */
export class InMemoryFileProvider implements FileProvider {
  private files: Map<string, string>;

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

  onFileChanged(path: string, content: string): void {
    this.files.set(path, content);
  }
}
