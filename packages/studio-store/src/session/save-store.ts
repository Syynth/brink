/**
 * Save stores (W14/#3307, spec §F17 RULED) — where runtime checkpoints
 * live. Two stores are first-class and always visible: **project**
 * (shareable, in the project tree on hosts with a filesystem) and
 * **local** ("this computer", private per-project app data). The studio
 * web embed backs both with `localStorage`; a desktop/embedder host
 * replaces either via `mountStudio`'s `saveStores` option (Tauri file
 * callbacks) — the async interface is the seam.
 *
 * The PAYLOAD is the runtime's existing `SaveState` boundary — durable
 * game state (globals, visits, turn index, rng), never execution
 * position; the slot's `knotPath` records where the author was, and
 * loading diverts there (knot-entry granularity, the honest v1).
 */

import type { SaveState, StructuralTranscript } from "@brink/wasm-types";

/** One checkpoint's listing row. */
export interface SaveSlotMeta {
  /** Stable slot id within its store. */
  id: string;
  /** Author-facing name ("Save 3", renameable later). */
  name: string;
  /** `DebugState.turn_index` at save time — the TURN chip. */
  turn: number;
  /** Nearest named container at save time — the load divert target and
   * the row's context; `null` when the runtime couldn't say. */
  knotPath: string | null;
  /** The program checksum the save was captured against — an `OLD` chip
   * when it no longer matches the current compile. */
  checksum: string | null;
  /** Unix ms. */
  savedAt: number;
}

export interface SavePayload {
  meta: SaveSlotMeta;
  state: SaveState;
  /** The story-so-far at save time, in STRUCTURAL form (RULED
   * 2026-08-30): the runtime's part stream as JSON, re-rendered against
   * the CURRENT compile on load — so a save restored after an edit shows
   * the edited prose. Absent on saves from before the ruling. */
  transcript?: StructuralTranscript;
}

/** A checkpoint store. All operations async — the desktop host's file
 * callbacks are; the localStorage default resolves immediately. */
export interface SaveStore {
  list(): Promise<SaveSlotMeta[]>;
  read(id: string): Promise<SavePayload | null>;
  /** Write a payload. `id: null` allocates a NEW slot (fork semantics);
   * an existing id overwrites (load-attachment write-back). Returns the
   * stored meta (with the allocated id). */
  write(id: string | null, payload: Omit<SavePayload, "meta"> & { meta: Omit<SaveSlotMeta, "id"> }): Promise<SaveSlotMeta>;
  remove(id: string): Promise<void>;
}

interface StoredBlob {
  version: 1;
  slots: SavePayload[];
}

/**
 * The default web-embed store: one localStorage key holding every slot.
 * Per-project via the key's scope suffix (the same discipline as the
 * breakpoint store). Storage failures degrade to in-session.
 */
export function localStorageSaveStore(
  storage: Pick<Storage, "getItem" | "setItem">,
  key: string,
): SaveStore {
  const load = (): StoredBlob => {
    try {
      const raw = storage.getItem(key);
      if (raw === null || raw === "") return { version: 1, slots: [] };
      const parsed = JSON.parse(raw) as StoredBlob;
      return Array.isArray(parsed?.slots) ? parsed : { version: 1, slots: [] };
    } catch {
      return { version: 1, slots: [] };
    }
  };
  const persist = (blob: StoredBlob): void => {
    try {
      storage.setItem(key, JSON.stringify(blob));
    } catch {
      // Quota/denied — the in-memory listing the caller holds still works.
    }
  };

  return {
    list: () => Promise.resolve(load().slots.map((s) => s.meta)),
    read: (id) =>
      Promise.resolve(load().slots.find((s) => s.meta.id === id) ?? null),
    write: (id, payload) => {
      const blob = load();
      const slotId =
        id ??
        `s${(blob.slots.reduce((max, s) => {
          const n = Number(s.meta.id.replace(/^s/, ""));
          return Number.isFinite(n) ? Math.max(max, n) : max;
        }, 0) + 1).toString()}`;
      const meta: SaveSlotMeta = { ...payload.meta, id: slotId };
      const next = blob.slots.filter((s) => s.meta.id !== slotId);
      next.push({ meta, state: payload.state, transcript: payload.transcript });
      persist({ version: 1, slots: next });
      return Promise.resolve(meta);
    },
    remove: (id) => {
      const blob = load();
      persist({ version: 1, slots: blob.slots.filter((s) => s.meta.id !== id) });
      return Promise.resolve();
    },
  };
}
