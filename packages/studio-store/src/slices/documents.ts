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
  /** docKey ("main.ink" / "main.ink::start") of the focused group's active
   *  ink document; "" when none. */
  activeDocKey: string;
  /** Count of files whose session content diverges from the last-saved /
   *  last-notified baseline (mirrored from the project's FileChangeHub by
   *  a mount.tsx listener; feeds StudioPublicState.dirtyFiles). */
  dirtyFiles: number;
  /** Injected opener (main.tsx → editor-groups store); null until bound. */
  _openTarget: ((target: TabTarget, pinned: boolean) => void) | null;

  /** Open an ink document (pinned, or as the group's preview tab). */
  openTarget(target: TabTarget, pinned: boolean): void;
  /** Create a new file in the project and open it pinned. */
  addFile(name: string): Promise<void>;
  /** Bind the shell opener bridge (main.tsx, at bootstrap). */
  setDocumentOpener(open: (target: TabTarget, pinned: boolean) => void): void;
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
  dirtyFiles: 0,
  _openTarget: null,

  openTarget(target, pinned) {
    get()._openTarget?.(target, pinned);
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

  setActiveDocKey(key) {
    if (get().activeDocKey !== key) set({ activeDocKey: key });
  },

  setDirtyFiles(count) {
    if (get().dirtyFiles !== count) set({ dirtyFiles: count });
  },
});
