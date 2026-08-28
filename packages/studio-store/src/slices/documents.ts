/**
 * Documents slice — the store-side bridge to the shell's editor groups.
 *
 * Tab structure (groups, order, pin state, the active tab) is owned by the
 * shell's editor-groups store (spec §7.8); the studio store no longer mirrors
 * it. What feature components below the shell (Binder, …) need is:
 *
 * - `openTarget` — open an ink file/symbol document. The store sits below
 *   the shell (spec §7.2 layering), so the actual opener is injected by the
 *   app boundary (main.tsx) via `setDocumentOpener`, like `_notify`.
 * - `activeDocKey` — a read-only mirror of the focused group's active ink
 *   document, kept current by a main.tsx subscription (drives the Binder's
 *   active-row highlight).
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { TabTarget } from "../types.js";

export interface DocumentsSlice {
  /**
   * The open Settings section, or null when Settings is closed (#3174).
   *
   * Settings is a MODAL rather than an editor occupant (ruled 2026-08-27):
   * it is consult-and-adjust, so taking over the editor area cost you the
   * file you were looking at for something you leave in seconds.
   *
   * The section id lives here rather than inside the modal so every door
   * into Settings can open it at the right place — the command palette, the
   * Binder's `brink.toml` row, and the Problems panel's "Configure Exxx…"
   * all want different sections.
   */
  settingsSection: string | null;

  /**
   * Out-of-scope banners the author has put away, by path.
   *
   * SESSION-SCOPED and deliberately not persisted (ruled 2026-08-28): the
   * banner states something true about the project right now, so a
   * dismissal that outlived the session would keep quiet about a file the
   * author later un-INCLUDEs on purpose. Coming back on reload is the
   * accepted cost of never going stale.
   *
   * A Set rather than a per-document flag because the banner is rendered by
   * whichever view holds the file, and a tab switch remounts that view —
   * component-local state would forget on every switch, which is the same
   * complaint that opened #3144.
   */
  dismissedScopeBanners: ReadonlySet<string>;
  /** docKey ("main.ink" / "main.ink::start") of the focused group's active
   *  ink document; "" when none. */
  activeDocKey: string;
  /**
   * Bumped on every `openTarget`, including one that names the document
   * already active.
   *
   * Continuous view needs it: there, navigating IS scrolling, and the active
   * document does not change when you jump between two knots in the file you
   * are already in — or click the file you are already in from the Binder.
   * Watching `activeDocKey` alone made both do nothing at all.
   */
  navSeq: number;
  /** Count of files whose session content diverges from the last-saved /
   *  last-notified baseline (mirrored from the project's FileChangeHub by
   *  a mount.tsx listener; feeds StudioPublicState.dirtyFiles). */
  dirtyFiles: number;
  /** Injected opener (main.tsx → editor-groups store); null until bound. */
  _openTarget: ((target: TabTarget, pinned: boolean) => void) | null;
  /** Injected tab-closer (main.tsx → editor-groups store): closes every tab
   *  for a file path (the file doc and any of its `path::symbol` docs) across
   *  all groups. Null until bound. */
  _closeDocsForPath: ((path: string) => void) | null;
  /** Injected tab-renamer (main.tsx): re-keys every tab + view slot for a file
   *  path in place when the file is renamed/moved. Null until bound. */
  _renameDocPath: ((oldPath: string, newPath: string) => void) | null;
  /** Injected symbol-tab-renamer (main.tsx): re-keys the open `path::oldName`
   *  symbol tab + view slot to `path::newName` when a knot/stitch is renamed
   *  (#305). Null until bound. */
  _renameSymbolDoc: ((path: string, oldName: string, newName: string) => void) | null;

  /** Open an ink document (pinned, or as the group's preview tab). */
  openTarget(target: TabTarget, pinned: boolean): void;
  /** Open Settings at `section` (default: the first one), or close it. */
  setSettingsSection(section: string | null): void;
  /** @see dismissedScopeBanners */
  dismissScopeBanner(path: string): void;
  /** Close every open tab for `path` (file + its symbol docs). Used by delete
   *  so the shell tears the views down before the file leaves the session. */
  closeDocsForPath(path: string): void;
  /** Re-key every open tab + view slot for `oldPath` to `newPath` in place
   *  (rename/move), preserving pin/split/selection. */
  renameDocPath(oldPath: string, newPath: string): void;
  /** Re-key the open `path::oldName` symbol tab + view slot to `path::newName`
   *  in place when a knot/stitch is renamed (#305). No-op when no such tab. */
  renameSymbolDocKey(path: string, oldName: string, newName: string): void;
  /** Create a new file in the project and open it pinned. */
  addFile(name: string): Promise<void>;
  /** Bind the shell opener bridge (main.tsx, at bootstrap). */
  setDocumentOpener(open: (target: TabTarget, pinned: boolean) => void): void;
  /** Bind the shell tab-closer bridge (main.tsx, at bootstrap). */
  setDocCloser(close: (path: string) => void): void;
  /** Bind the shell tab-renamer bridge (main.tsx, at bootstrap). */
  setDocRenamer(rename: (oldPath: string, newPath: string) => void): void;
  /** Bind the shell symbol-tab-renamer bridge (main.tsx, at bootstrap). */
  setDocSymbolRenamer(
    rename: (path: string, oldName: string, newName: string) => void,
  ): void;
  /** Update the focused-document mirror (main.tsx subscription). */
  setActiveDocKey(key: string): void;
  /** Update the dirty-file summary (mount.tsx dirty listener). */
  setDirtyFiles(count: number): void;
}

export const createDocumentsSlice: StateCreator<StudioState, [], [], DocumentsSlice> = (
  set,
  get,
) => ({
  activeDocKey: "",
  settingsSection: null,
  dismissedScopeBanners: new Set<string>(),
  navSeq: 0,
  dirtyFiles: 0,
  _openTarget: null,
  _closeDocsForPath: null,
  _renameDocPath: null,
  _renameSymbolDoc: null,

  dismissScopeBanner(path) {
    const next = new Set(get().dismissedScopeBanners);
    next.add(path);
    set({ dismissedScopeBanners: next });
  },

  setSettingsSection(section) {
    set({ settingsSection: section });
  },

  openTarget(target, pinned) {
    set({ navSeq: get().navSeq + 1 });
    get()._openTarget?.(target, pinned);
  },

  closeDocsForPath(path) {
    get()._closeDocsForPath?.(path);
  },

  renameDocPath(oldPath, newPath) {
    get()._renameDocPath?.(oldPath, newPath);
  },

  renameSymbolDocKey(path, oldName, newName) {
    get()._renameSymbolDoc?.(path, oldName, newName);
  },

  async addFile(name) {
    const project = get()._project;
    if (!project) return;
    await project.addFile(name);
    get().openTarget({ kind: "file", path: name }, true);
  },

  setDocumentOpener(open) {
    set({ _openTarget: open });
  },

  setDocCloser(close) {
    set({ _closeDocsForPath: close });
  },

  setDocRenamer(rename) {
    set({ _renameDocPath: rename });
  },

  setDocSymbolRenamer(rename) {
    set({ _renameSymbolDoc: rename });
  },

  setActiveDocKey(key) {
    if (get().activeDocKey !== key) set({ activeDocKey: key });
  },

  setDirtyFiles(count) {
    if (get().dirtyFiles !== count) set({ dirtyFiles: count });
  },
});
