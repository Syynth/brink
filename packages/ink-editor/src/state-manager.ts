/**
 * EditorStateManager — manages per-tab CodeMirror EditorState instances.
 *
 * Supports both full-file tabs and focused symbol tabs (knot/stitch).
 * Handles pinned/unpinned tab semantics: at most one unpinned tab,
 * which gets replaced on the next single-click navigation.
 *
 * Focused symbol tabs use the wasm session's native view context —
 * `update_source(fragment)` splices into the full file internally,
 * and IDE responses return view-relative offsets.
 */

import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { defaultKeymap } from "@codemirror/commands";
import { brinkStudio, type BrinkStudioOptions } from "./extensions.js";
import type { ProjectSession } from "./project-session.js";

// ── Public types ───────────────────────────────────────────────────

export type TabTarget =
  | { kind: "file"; path: string }
  | { kind: "symbol"; path: string; name: string; start: number; end: number };

export interface TabInfo {
  /** Unique key for state map (e.g. "main.ink" or "main.ink::start"). */
  id: string;
  target: TabTarget;
  pinned: boolean;
  /** Display name for the tab. */
  label: string;
}

// ── Helpers ────────────────────────────────────────────────────────

function tabId(target: TabTarget): string {
  if (target.kind === "file") return target.path;
  return `${target.path}::${target.name}`;
}

function tabLabel(target: TabTarget): string {
  if (target.kind === "file") {
    const slash = target.path.lastIndexOf("/");
    return slash >= 0 ? target.path.substring(slash + 1) : target.path;
  }
  const slash = target.path.lastIndexOf("/");
  const fileName = slash >= 0 ? target.path.substring(slash + 1) : target.path;
  return `${target.name} (${fileName})`;
}

// ── Manager ────────────────────────────────────────────────────────

export class EditorStateManager {
  private states: Map<string, EditorState> = new Map();
  private studioOptions: BrinkStudioOptions;
  private project: ProjectSession;
  private view: EditorView | null = null;

  /** Ordered list of open tabs. */
  private _tabs: TabInfo[] = [];
  private activeTabId: string;

  constructor(project: ProjectSession, studioOptions: BrinkStudioOptions) {
    this.project = project;
    this.studioOptions = studioOptions;

    // Start with a single pinned file tab for the entry file
    const entryPath = project.getActiveFile();
    const target: TabTarget = { kind: "file", path: entryPath };
    const id = tabId(target);
    this._tabs.push({ id, target, pinned: true, label: tabLabel(target) });
    this.activeTabId = id;
  }

  // ── View binding ───────────────────────────────────────────────

  /** Bind the EditorView. Must be called before switchTo/snapshot. */
  setView(view: EditorView): void {
    this.view = view;
    this.states.set(this.activeTabId, view.state);
  }

  /** Get the EditorView (for external use like triggerCompile). */
  getView(): EditorView {
    if (!this.view) throw new Error("EditorStateManager: view not set");
    return this.view;
  }

  // ── Tab accessors ──────────────────────────────────────────────

  getTabs(): readonly TabInfo[] {
    return this._tabs;
  }

  getActiveTab(): TabInfo {
    return this._tabs.find((t) => t.id === this.activeTabId)!;
  }

  /** Which file path is currently active (for the underlying wasm session). */
  active(): string {
    const tab = this.getActiveTab();
    return tab.target.path;
  }

  /** List all open file paths (unique, for compatibility). */
  files(): string[] {
    return this.project.getSession().listFiles().map((f) => f.path);
  }

  /** Access the underlying ProjectSession. */
  getProject(): ProjectSession {
    return this.project;
  }

  // ── Tab operations ─────────────────────────────────────────────

  /**
   * Open a tab for the given target.
   *
   * - If pinned=false: reuse the existing unpinned tab (replacing its content),
   *   or create a new unpinned tab. At most one unpinned tab at a time.
   * - If pinned=true: pin an existing tab with the same ID, or create new pinned.
   */
  async openTab(target: TabTarget, pinned: boolean): Promise<void> {
    const id = tabId(target);

    // Already open — just switch to it (and optionally pin)
    const existing = this._tabs.find((t) => t.id === id);
    if (existing) {
      if (pinned && !existing.pinned) existing.pinned = true;
      // Update range for symbol tabs (outline may have changed)
      if (target.kind === "symbol" && existing.target.kind === "symbol") {
        existing.target.start = target.start;
        existing.target.end = target.end;
      }
      await this.switchToTab(id);
      return;
    }

    if (!pinned) {
      // Find existing unpinned tab and replace it
      const unpinnedIdx = this._tabs.findIndex((t) => !t.pinned);
      if (unpinnedIdx >= 0) {
        const oldTab = this._tabs[unpinnedIdx]!;
        // Flush current tab BEFORE replacing (the old tab may be active)
        if (this.view && oldTab.id === this.activeTabId) {
          this.flushCurrentTab();
        }
        // Remove old state
        this.states.delete(oldTab.id);
        // Replace in-place
        const newTab: TabInfo = { id, target, pinned: false, label: tabLabel(target) };
        this._tabs[unpinnedIdx] = newTab;
        await this.switchToTab(id);
        return;
      }
    }

    // Create a new tab
    const newTab: TabInfo = { id, target, pinned, label: tabLabel(target) };
    this._tabs.push(newTab);
    await this.switchToTab(id);
  }

  /** Pin an unpinned tab. */
  pinTab(id: string): void {
    const tab = this._tabs.find((t) => t.id === id);
    if (tab) tab.pinned = true;
  }

  /** Pin the currently active tab (used by auto-pin on edit). */
  pinActiveTab(): void {
    this.pinTab(this.activeTabId);
  }

  /** Close a tab. Returns false if it's the last tab. */
  async closeTab(id: string): Promise<boolean> {
    if (this._tabs.length <= 1) return false;

    const idx = this._tabs.findIndex((t) => t.id === id);
    if (idx < 0) return false;

    // If closing the active tab, switch to an adjacent one
    if (id === this.activeTabId) {
      const nextTab = this._tabs[idx + 1] ?? this._tabs[idx - 1]!;
      await this.switchToTab(nextTab.id);
    }

    this._tabs.splice(idx, 1);
    this.states.delete(id);
    return true;
  }

  // ── State management ───────────────────────────────────────────

  /** Get or create the EditorState for a tab. */
  getState(tabIdOrPath: string): EditorState {
    let state = this.states.get(tabIdOrPath);
    if (!state) {
      const tab = this._tabs.find((t) => t.id === tabIdOrPath);
      const content = tab ? this.getTabContent(tab) : (this.project.getSession().getFileSource(tabIdOrPath) ?? "");
      state = this.createState(content);
      this.states.set(tabIdOrPath, state);
    }
    return state;
  }

  /** Add a new file to the project. Opens as a pinned tab. */
  async addFile(path: string, content: string = ""): Promise<void> {
    await this.project.addFile(path, content);
    await this.openTab({ kind: "file", path }, true);
  }

  /** Close a file (legacy compat — closes its tab). */
  async closeFile(path: string): Promise<boolean> {
    const tab = this._tabs.find((t) => t.target.path === path && t.target.kind === "file");
    if (!tab) return false;
    return this.closeTab(tab.id);
  }

  /** Save current view state back into the map. */
  snapshot(): void {
    if (this.view) {
      this.states.set(this.activeTabId, this.view.state);
    }
  }

  // ── Auto-pin extension ─────────────────────────────────────────

  autoPinExtension(): Extension {
    const self = this;
    return EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        const tab = self.getActiveTab();
        if (!tab.pinned) {
          tab.pinned = true;
          self.view?.dom.dispatchEvent(new CustomEvent("brink-tab-pinned"));
        }
      }
    });
  }

  // ── Private ────────────────────────────────────────────────────

  /** Get the text content to display for a tab. */
  private getTabContent(tab: TabInfo): string {
    const session = this.project.getSession();
    if (tab.target.kind === "file") {
      session.clearViewContext();
      return session.getFileSource(tab.target.path) ?? "";
    }
    // Symbol tab — set view context and get the fragment
    session.setViewContext(tab.target.start, tab.target.end);
    return session.getViewSource() ?? "";
  }

  private async switchToTab(id: string): Promise<void> {
    if (id === this.activeTabId && this.states.has(id)) return;
    if (!this.view) throw new Error("EditorStateManager: view not set");

    const tab = this._tabs.find((t) => t.id === id);
    if (!tab) throw new Error(`No tab with id: ${id}`);

    // Flush current content to the wasm session before switching
    this.flushCurrentTab();

    // Snapshot current CM state
    this.states.set(this.activeTabId, this.view.state);

    // Ensure the wasm session knows about the target file.
    // setActiveFile clears the view context, so we must set the view
    // context AFTER this call and BEFORE creating the EditorState
    // (state creation triggers computeLineInfos → updateSource).
    await this.project.setActiveFile(tab.target.path);

    // Set or clear the view context BEFORE getState (which creates
    // the EditorState whose StateField calls updateSource)
    if (tab.target.kind === "symbol") {
      this.project.getSession().setViewContext(tab.target.start, tab.target.end);
    } else {
      this.project.getSession().clearViewContext();
    }

    // Get or create state for target tab
    const state = this.getState(id);

    this.view.setState(state);
    this.activeTabId = id;
  }

  /** Flush current editor content to the wasm session. */
  private flushCurrentTab(): void {
    if (!this.view) return;
    const tab = this.getActiveTab();
    if (!tab) return;
    const source = this.view.state.doc.toString();

    // updateSource handles splicing natively when a view context is active
    this.project.getSession().updateSource(source);

    if (tab.target.kind === "symbol") {
      tab.target.end = tab.target.start + source.length;
      this.states.delete(tab.target.path);
    }
  }

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
      EditorView.lineWrapping,
      this.autoPinExtension(),
    ];
  }
}
