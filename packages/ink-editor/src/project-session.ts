/**
 * ProjectSession — bridges a FileProvider with an EditorSession.
 *
 * Handles multi-file loading, INCLUDE resolution, file creation, provider
 * write-back, and project compilation (cached by the session's mutation
 * generation, so several live views can each ask for "the current compile"
 * without recompiling an unchanged project).
 *
 * Per-view document state (wasm document handles, CM6 states, mirroring)
 * lives in DocumentSessions — this class owns only project-level concerns.
 */

import type { FileProvider } from "./provider.js";
import { EditorSessionHandle } from "@brink/wasm";
import type { CompileResult } from "@brink/wasm-types";

export interface ProjectSessionOptions {
  provider: FileProvider;
  entryFile: string;
  /** Re-use an existing session, or a new one is created. */
  session?: EditorSessionHandle;
  /** Called when an external file change is detected. */
  onExternalFileChange?: (path: string, content: string | null) => void;
}

export class ProjectSession {
  private provider: FileProvider;
  private entryFile: string;
  private session: EditorSessionHandle;
  private onExternalFileChange?: (path: string, content: string | null) => void;
  private unsubscribeExternal?: () => void;
  private destroyed = false;
  private lastCompile: { generation: number; result: CompileResult } | null = null;

  constructor(options: ProjectSessionOptions) {
    this.provider = options.provider;
    this.entryFile = options.entryFile;
    this.session = options.session ?? new EditorSessionHandle();
    this.onExternalFileChange = options.onExternalFileChange;
  }

  /** Load all files from provider and resolve INCLUDEs. */
  async initialize(): Promise<void> {
    const files = await this.provider.listFiles();
    for (const file of files) {
      const content = await this.provider.readFile(file);
      this.session.updateFile(file, content);
    }

    await this.resolveIncludes();

    // Register external change callback if the provider supports it. Keep the
    // unsubscribe so destroy() can detach it — otherwise a later external change
    // would call into a freed wasm session (use-after-free).
    this.unsubscribeExternal = this.provider.onExternalChange?.((path, content) => {
      if (this.destroyed) return;
      if (content === null) {
        this.session.removeFile(path);
      } else {
        this.session.updateFile(path, content);
      }
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

  /** Create a new file and add it to the session. */
  async addFile(path: string, content: string = ""): Promise<void> {
    await this.provider.createFile(path, content);
    this.session.updateFile(path, content);
  }

  /** Remove a file from the wasm session (does not delete from provider). */
  closeFile(path: string): void {
    this.session.removeFile(path);
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

  /** Write a file's current session content back to the provider. */
  notifyFileChanged(path: string): void {
    const source = this.session.getFileSource(path);
    if (source !== null) {
      this.provider.onFileChanged?.(path, source);
    }
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
    }
    return content;
  }

  /** Tear down. Detaches the external-change listener before freeing the
   *  session so a late callback can't touch freed wasm memory. */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
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
          queue.push(inc.resolved);
        }
      }
    }
  }
}
