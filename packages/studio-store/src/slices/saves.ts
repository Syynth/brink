/**
 * Runtime save/load slice (W14/#3307, spec §F17 RULED) — checkpoints so
 * the author can get somewhere and keep testing from there.
 *
 * Two stores are first-class (`project` / `local`); the App setting
 * picks only the DEFAULT TARGET for new saves. Load vs Fork (RULED):
 * Load attaches the session to the slot — "Save state" writes back;
 * Fork starts from a copy — unattached, the next save allocates a new
 * slot, checkpoint untouched. A non-clean `LoadReport` surfaces inline
 * (the provider's transcript notice), never silently.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { SaveSlotMeta, SaveStore } from "../session/save-store.js";

export type SaveLocation = "project" | "local";

export interface SavesSlice {
  /** Listings per store, refreshed by `refreshSaves`. */
  saveSlots: Record<SaveLocation, SaveSlotMeta[]>;
  /** The slot a Load attached the session to — "Save state" writes back
   * here; `null` = unattached (fresh runs, forks) → next save allocates
   * a new slot in `saveLocationDefault`. */
  attachedSlot: { location: SaveLocation; id: string } | null;
  /** Default target for NEW saves (App setting; both stores stay
   * visible regardless). */
  saveLocationDefault: SaveLocation;
  setSaveLocationDefault(location: SaveLocation): void;
  /** Wire the stores (app boundary; desktop replaces via mount options). */
  setSaveStores(stores: Record<SaveLocation, SaveStore>): void;
  /** Re-list both stores. */
  refreshSaves(): Promise<void>;
  /** Capture the running session into the attached slot (write-back) or
   * a new slot in the default location. No-op without a live session. */
  saveCurrentState(): Promise<void>;
  /** Load a checkpoint and ATTACH to it. Starts a session if none. */
  loadSave(location: SaveLocation, id: string): Promise<void>;
  /** Load a checkpoint UNATTACHED — the fork gesture. */
  forkSave(location: SaveLocation, id: string): Promise<void>;
  removeSave(location: SaveLocation, id: string): Promise<void>;
  /** Internal. */
  _saveStores: Record<SaveLocation, SaveStore> | null;
}

export const createSavesSlice: StateCreator<StudioState, [], [], SavesSlice> = (set, get) => {
  const loadInto = async (
    location: SaveLocation,
    id: string,
    attach: boolean,
  ): Promise<void> => {
    const stores = get()._saveStores;
    if (!stores) return;
    const payload = await stores[location].read(id);
    if (payload === null) {
      get()._notify?.({
        severity: "warning",
        source: "story",
        message: "That save no longer exists.",
      });
      await get().refreshSaves();
      return;
    }
    // A checkpoint needs a live session at a turn boundary — start one
    // when the Player is idle (the launcher's whole flow).
    if (get().sessionStatus === "none") {
      const bytes = get().storyBytes;
      if (!bytes) {
        get()._notify?.({
          severity: "warning",
          source: "story",
          message: "Compile the story before loading a save.",
        });
        return;
      }
      get().startSession(bytes);
    }
    const provider = get()._provider;
    if (!provider || typeof provider.loadCheckpoint !== "function") return;
    const report = provider.loadCheckpoint(
      payload.state,
      payload.meta.knotPath,
      "Loaded",
      payload.transcript ?? null,
    );
    if (report === null) return;
    set({ attachedSlot: attach ? { location, id } : null });
  };

  return {
    saveSlots: { project: [], local: [] },
    attachedSlot: null,
    saveLocationDefault: "local",
    _saveStores: null,

    setSaveLocationDefault(location) {
      set({ saveLocationDefault: location });
    },

    setSaveStores(stores) {
      set({ _saveStores: stores });
      void get().refreshSaves();
    },

    async refreshSaves() {
      const stores = get()._saveStores;
      if (!stores) return;
      const [project, local] = await Promise.all([
        stores.project.list(),
        stores.local.list(),
      ]);
      const bySavedAt = (a: SaveSlotMeta, b: SaveSlotMeta): number =>
        b.savedAt - a.savedAt;
      set({
        saveSlots: {
          project: [...project].sort(bySavedAt),
          local: [...local].sort(bySavedAt),
        },
      });
    },

    async saveCurrentState() {
      const stores = get()._saveStores;
      const provider = get()._provider;
      if (!stores || !provider || typeof provider.saveState !== "function") return;
      const state = provider.saveState();
      if (state === null) {
        get()._notify?.({
          severity: "warning",
          source: "story",
          message: "Nothing to save — no live session.",
        });
        return;
      }
      const st = get();
      const attached = st.attachedSlot;
      const location = attached?.location ?? st.saveLocationDefault;
      const id = attached?.id ?? null;
      const existing = id !== null ? st.saveSlots[location].find((s) => s.id === id) : undefined;
      const count = st.saveSlots.project.length + st.saveSlots.local.length;
      const meta = {
        name: existing?.name ?? `Save ${count + 1}`,
        turn: st.debugState?.turn_index ?? 0,
        knotPath: st.debugState?.current_location ?? null,
        checksum: st.programChecksum,
        savedAt: Date.now(),
      };
      const written = await stores[location].write(id, {
        meta,
        state,
        // Structural (RULED 2026-08-30): the part stream, not resolved
        // text — a load re-renders it against whatever compile is current.
        transcript: provider.exportTranscript?.() ?? undefined,
      });
      set({ attachedSlot: { location, id: written.id } });
      await get().refreshSaves();
      get()._notify?.({
        severity: "info",
        source: "story",
        message: `Saved "${written.name}" (${location === "local" ? "this computer" : "project"}).`,
      });
    },

    loadSave: (location, id) => loadInto(location, id, true),
    forkSave: (location, id) => loadInto(location, id, false),

    async removeSave(location, id) {
      const stores = get()._saveStores;
      if (!stores) return;
      await stores[location].remove(id);
      const attached = get().attachedSlot;
      if (attached?.location === location && attached.id === id) {
        set({ attachedSlot: null });
      }
      await get().refreshSaves();
    },
  };
};
