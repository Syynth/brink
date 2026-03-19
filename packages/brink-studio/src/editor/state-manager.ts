/**
 * EditorStateManager — manages per-file CodeMirror EditorState instances.
 *
 * Owns the Map<path, EditorState> and handles state creation, switching,
 * and snapshotting. Separated from ProjectSession so the project layer
 * stays independent of CodeMirror.
 */

import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { defaultKeymap } from "@codemirror/commands";
import { brinkStudio, type BrinkStudioOptions } from "./extensions.js";
import type { ProjectSession } from "../project-session.js";

export class EditorStateManager {
  private states: Map<string, EditorState> = new Map();
  private studioOptions: BrinkStudioOptions;
  private activeFile: string;
  private project: ProjectSession;
  private view: EditorView | null = null;

  constructor(project: ProjectSession, studioOptions: BrinkStudioOptions) {
    this.project = project;
    this.studioOptions = studioOptions;
    this.activeFile = project.getActiveFile();
  }

  /** Bind the EditorView. Must be called before switchTo/snapshot. */
  setView(view: EditorView): void {
    this.view = view;
    // Capture the initial state for the active file
    this.states.set(this.activeFile, view.state);
  }

  /** Get the EditorView (for external use like triggerCompile). */
  getView(): EditorView {
    if (!this.view) throw new Error("EditorStateManager: view not set");
    return this.view;
  }

  /** Get or create the EditorState for a file. */
  getState(path: string): EditorState {
    let state = this.states.get(path);
    if (!state) {
      const content = this.project.getSession().getFileSource(path) ?? "";
      state = this.createState(content);
      this.states.set(path, state);
    }
    return state;
  }

  /** Switch the view to a different file. */
  async switchTo(path: string): Promise<void> {
    if (path === this.activeFile) return;
    if (!this.view) throw new Error("EditorStateManager: view not set");

    // Flush current content to the wasm session before switching —
    // the compile callback is debounced, so recent edits may not have
    // been persisted to the session yet.
    const currentSource = this.view.state.doc.toString();
    this.project.getSession().updateFile(this.activeFile, currentSource);
    this.project.getSession().setActiveFile(this.activeFile);

    // Snapshot CM state (cursor, scroll, undo history)
    this.states.set(this.activeFile, this.view.state);

    // Update the wasm session's active file (may load from provider)
    await this.project.setActiveFile(path);

    // Get or create the target state
    const state = this.getState(path);

    // Swap
    this.view.setState(state);
    this.activeFile = path;
  }

  /** Create a new file, add to project and provider. */
  async addFile(path: string, content: string = ""): Promise<void> {
    await this.project.addFile(path, content);
    // State created lazily on switchTo
  }

  /** Close a file. Switches to an adjacent tab if closing the active file.
   *  Returns false if the file cannot be closed (last file). */
  async closeFile(path: string): Promise<boolean> {
    const files = this.files();
    if (files.length <= 1) return false;

    if (path === this.activeFile) {
      const idx = files.indexOf(path);
      const nextPath = files[idx + 1] ?? files[idx - 1]!;
      await this.switchTo(nextPath);
    }

    this.states.delete(path);
    this.project.closeFile(path);
    return true;
  }

  /** Save current view state back into the map. */
  snapshot(): void {
    if (this.view) {
      this.states.set(this.activeFile, this.view.state);
    }
  }

  /** List all files in the project. */
  files(): string[] {
    return this.project.getSession().listFiles().map((f) => f.path);
  }

  /** Which file is currently active. */
  active(): string {
    return this.activeFile;
  }

  /** Access the underlying ProjectSession. */
  getProject(): ProjectSession {
    return this.project;
  }

  /** Create a fresh EditorState with the shared extensions. */
  private createState(content: string): EditorState {
    return EditorState.create({
      doc: content,
      extensions: this.createExtensions(),
    });
  }

  private createExtensions(): Extension[] {
    return [
      brinkStudio(this.studioOptions),
      basicSetup,
      keymap.of(defaultKeymap),
    ];
  }
}
