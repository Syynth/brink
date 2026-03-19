/**
 * ProjectSession — bridges a FileProvider with an EditorSession.
 *
 * Handles multi-file loading, INCLUDE resolution, active file switching,
 * and generates editor options that wire everything together.
 */

import type { FileProvider } from "./provider.js";
import { EditorSessionHandle, getTokenTypeNames } from "./wasm.js";
import type { BrinkEditorOptions } from "./editor/index.js";

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
  private activeFile: string;
  private onExternalFileChange?: (path: string, content: string | null) => void;

  constructor(options: ProjectSessionOptions) {
    this.provider = options.provider;
    this.entryFile = options.entryFile;
    this.session = options.session ?? new EditorSessionHandle();
    this.activeFile = options.entryFile;
    this.onExternalFileChange = options.onExternalFileChange;
  }

  /** Load all files from provider, resolve INCLUDEs, set active file. */
  async initialize(): Promise<void> {
    const files = await this.provider.listFiles();
    for (const file of files) {
      const content = await this.provider.readFile(file);
      this.session.updateFile(file, content);
    }

    await this.resolveIncludes();

    this.session.setActiveFile(this.entryFile);
    this.activeFile = this.entryFile;

    // Register external change callback if the provider supports it
    this.provider.onExternalChange?.((path, content) => {
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

  /** Current active file. */
  getActiveFile(): string {
    return this.activeFile;
  }

  /** Switch active file. Loads from provider if not yet in session. */
  async setActiveFile(path: string): Promise<string> {
    // Try to set directly — file may already be loaded
    if (!this.session.setActiveFile(path)) {
      // Not loaded yet — try to get it from the provider
      const content = await this.provider.requestFile(path);
      if (content !== null) {
        this.session.updateFile(path, content);
        this.session.setActiveFile(path);
      } else {
        throw new Error(`File not available: ${path}`);
      }
    }
    this.activeFile = path;
    return this.session.getFileSource(path) ?? "";
  }

  /** Generate BrinkEditorOptions for createBrinkEditor. */
  createEditorOptions(): BrinkEditorOptions {
    const session = this.session;
    const provider = this.provider;
    const self = this;

    return {
      initialContent: session.getFileSource(this.activeFile) ?? "",
      compile: (source: string) => {
        session.updateFile(self.activeFile, source);
        provider.onFileChanged?.(self.activeFile, source);
        // Kick off async INCLUDE resolution — next compile picks up new files
        void self.resolveIncludes();
        return session.compileProject(self.entryFile);
      },
      getSemanticTokens: (source: string) => {
        session.updateSource(source);
        return session.getSemanticTokens();
      },
      getTokenTypeNames,
      session,
      getCompletions: (_source: string, offset: number) => session.getCompletions(offset),
      getHover: (_source: string, offset: number) => session.getHover(offset),
      gotoDefinition: (_source: string, offset: number) => session.gotoDefinition(offset),
      findReferences: (_source: string, offset: number) => session.findReferences(offset),
      prepareRename: (_source: string, offset: number) => session.prepareRename(offset),
      doRename: (_source: string, offset: number, newName: string) => session.doRename(offset, newName),
      getCodeActions: (_source: string, offset: number) => session.getCodeActions(offset),
      getInlayHints: (_source: string, start: number, end: number) => session.getInlayHints(start, end),
      getSignatureHelp: (_source: string, offset: number) => session.getSignatureHelp(offset),
      getFoldingRanges: () => session.getFoldingRanges(),
    };
  }

  /** Request save via provider. */
  async save(): Promise<void> {
    await this.provider.requestSave?.();
  }

  /** Tear down. */
  destroy(): void {
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
