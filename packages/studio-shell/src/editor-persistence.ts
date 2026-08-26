/**
 * @brink/studio-shell — editor-state persistence (docs/studio-shell-spec.md §7.8).
 *
 * The durable half of the editor-groups store — the group structure, each
 * group's tab list with pin state and active tab, the focused group, splitter
 * sizes — plus each open document's cursor + scroll, round-tripped through a
 * versioned localStorage key.
 *
 * Unlike every other persisted key in the shell (layout, theme, keymap,
 * editor settings), this one is SCOPED PER PROJECT: a tab list only means
 * anything relative to the project whose files it names (decision log
 * 2026-08-26, "Restored editor tabs are scoped per project, with an LRU
 * cap"). One payload holds an entry per scope, most-recently-written first,
 * truncated at `MAX_SCOPES` — a single read/write keeps the eviction order
 * honest and leaves no orphan keys behind, which per-scope keys would.
 *
 * Loading is lenient in the same way `layout-persistence` is: a corrupt,
 * foreign-version or partially-invalid payload yields null and the defaults
 * win. What loading does NOT do is decide whether a tab's document still
 * exists — the shell has no file list. The host filters the restored tabs
 * (`applyEditorSnapshot`'s `isKnown`) because only it knows the project.
 */

import type { DocumentRef } from "./document.js";
import { documentKey } from "./document.js";
import type { EditorGroup, EditorGroupsState, EditorGroupsStore } from "./editor-groups.js";

export const EDITOR_STORAGE_KEY = "brink-studio.editors.v1";

const SNAPSHOT_VERSION = 1;

/**
 * How many projects keep a remembered editor layout. Past this the
 * least-recently-written entry is dropped — the cap is the whole reason
 * keying by project is safe (see the decision-log entry above).
 */
export const MAX_SCOPES = 20;

/** Debounce for writes — tab drags and splitter moves come in bursts. */
const WRITE_DEBOUNCE_MS = 250;

/**
 * A view's cursor + scroll, structurally identical to `ink-editor`'s
 * `ViewStateSnapshot`. Redeclared rather than imported: the shell does not
 * depend on the editor package (spec §7.2), and this is a plain data shape.
 */
export interface PersistedViewState {
  anchor: number;
  head: number;
  scrollTop: number;
}

export interface EditorSnapshot {
  groups: EditorGroup[];
  focusedGroupId: string;
  groupSizes: Record<string, number>;
  /** Keyed by `documentKey(ref)`, for open tabs only. */
  viewStates: Record<string, PersistedViewState>;
}

type StorageLike = Pick<Storage, "getItem" | "setItem">;

/**
 * The persisted subset of the current state. `maximizedGroupId` is
 * deliberately absent: a maximized group is a momentary "show me just this"
 * gesture, and coming back to a window that silently hides the other groups
 * reads as a bug rather than as restored state.
 */
export function snapshotEditorGroups(
  state: EditorGroupsState,
  viewStates: Record<string, PersistedViewState>,
): EditorSnapshot {
  return {
    groups: state.groups,
    focusedGroupId: state.focusedGroupId,
    groupSizes: state.groupSizes,
    viewStates,
  };
}

/**
 * Load the snapshot stored for `scope`. Never throws; anything unparseable,
 * foreign-versioned or structurally wrong yields null.
 */
export function loadEditorSnapshot(
  storage: Pick<Storage, "getItem">,
  scope: string,
): EditorSnapshot | null {
  const entries = readEntries(storage);
  if (entries === null) return null;
  const entry = entries.find((e) => e.scope === scope);
  return entry?.snapshot ?? null;
}

/**
 * Subscribe the store to storage: debounced writes of the durable subset for
 * `scope`, flushed on pagehide so quick closes don't lose the last change.
 * Writing promotes `scope` to the front of the entry list, which is what
 * makes eviction least-recently-used. Returns a detach function (also
 * flushes).
 *
 * `viewStates` is a callback rather than a value because the cursor and
 * scroll of an open document live in the editor package, not here — the host
 * reads them at write time so a snapshot is never stale.
 */
export function attachEditorPersistence(
  store: EditorGroupsStore,
  storage: StorageLike,
  options: {
    scope: string;
    viewStates: () => Record<string, PersistedViewState>;
  },
  target: Pick<Window, "addEventListener" | "removeEventListener"> | null =
    typeof window === "undefined" ? null : window,
): () => void {
  const { scope, viewStates } = options;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const write = (): void => {
    timer = null;
    const snapshot = snapshotEditorGroups(store.getState(), viewStates());
    const existing = readEntries(storage) ?? [];
    const entries = [
      { scope, snapshot },
      ...existing.filter((e) => e.scope !== scope),
    ].slice(0, MAX_SCOPES);
    try {
      storage.setItem(
        EDITOR_STORAGE_KEY,
        JSON.stringify({ version: SNAPSHOT_VERSION, entries }),
      );
    } catch {
      // Quota/denied storage — persistence silently degrades to in-session.
    }
  };
  /**
   * Write unconditionally, pending timer or not.
   *
   * `layout-persistence`'s flush writes only when a write was already
   * scheduled, and is right to: every field it persists lives in the store,
   * so "nothing scheduled" means "nothing changed". Here HALF the payload —
   * each open document's cursor and scroll — lives outside the store, in the
   * editor package, and changes to it are invisible to the subscription
   * below. Scrolling and moving the caret schedule nothing, so a
   * conditional flush would persist them only when some unrelated structural
   * change happened to be pending, and reading to the bottom of a file and
   * reloading would put you back at the top.
   */
  const flush = (): void => {
    if (timer !== null) clearTimeout(timer);
    write();
  };
  const schedule = (): void => {
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(write, WRITE_DEBOUNCE_MS);
  };

  const unsubscribe = store.subscribe((state, previous) => {
    if (
      state.groups !== previous.groups ||
      state.focusedGroupId !== previous.focusedGroupId ||
      state.groupSizes !== previous.groupSizes
    ) {
      schedule();
    }
  });
  target?.addEventListener("pagehide", flush);

  return () => {
    unsubscribe();
    target?.removeEventListener("pagehide", flush);
    flush();
  };
}

/**
 * Narrow a loaded snapshot to the documents that still exist, and repair the
 * structure the drop can invalidate: groups left empty are removed, the
 * active tab of a group that lost it falls back to the group's first tab,
 * and a focused group that vanished falls back to the first group.
 *
 * Returns null when nothing survives — the caller then keeps the store's
 * own default single empty group rather than seeding an empty structure.
 */
export function reconcileEditorSnapshot(
  snapshot: EditorSnapshot,
  isKnown: (ref: DocumentRef) => boolean,
): EditorSnapshot | null {
  const groups: EditorGroup[] = [];
  for (const group of snapshot.groups) {
    const tabs = group.tabs.filter((tab) => isKnown(tab.ref));
    if (tabs.length === 0) continue;
    const keys = new Set(tabs.map((tab) => documentKey(tab.ref)));
    const activeKey =
      group.activeKey !== null && keys.has(group.activeKey)
        ? group.activeKey
        : documentKey(tabs[0].ref);
    groups.push({ id: group.id, tabs, activeKey });
  }
  if (groups.length === 0) return null;

  const focusedGroupId = groups.some((g) => g.id === snapshot.focusedGroupId)
    ? snapshot.focusedGroupId
    : groups[0].id;
  // Sizes and view states for dropped groups/documents are dead weight in
  // every future write, so they go now rather than lingering until the user
  // happens to reopen the file.
  const liveGroups = new Set(groups.map((g) => g.id));
  const liveDocs = new Set(groups.flatMap((g) => g.tabs.map((t) => documentKey(t.ref))));
  return {
    groups,
    focusedGroupId,
    groupSizes: pick(snapshot.groupSizes, (id) => liveGroups.has(id)),
    viewStates: pick(snapshot.viewStates, (key) => liveDocs.has(key)),
  };
}

// ── Lenient field readers ────────────────────────────────────────────

interface StoredEntry {
  scope: string;
  snapshot: EditorSnapshot;
}

function readEntries(storage: Pick<Storage, "getItem">): StoredEntry[] | null {
  let raw: string | null;
  try {
    raw = storage.getItem(EDITOR_STORAGE_KEY);
  } catch {
    return null;
  }
  if (raw === null || raw === "") return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!isRecord(parsed) || parsed.version !== SNAPSHOT_VERSION) return null;
  if (!Array.isArray(parsed.entries)) return null;

  const out: StoredEntry[] = [];
  for (const entry of parsed.entries) {
    if (!isRecord(entry) || typeof entry.scope !== "string") return null;
    const snapshot = readSnapshot(entry.snapshot);
    if (snapshot === null) return null;
    out.push({ scope: entry.scope, snapshot });
  }
  return out;
}

function readSnapshot(value: unknown): EditorSnapshot | null {
  if (!isRecord(value)) return null;
  if (typeof value.focusedGroupId !== "string") return null;
  if (!Array.isArray(value.groups)) return null;

  const groups: EditorGroup[] = [];
  for (const raw of value.groups) {
    if (!isRecord(raw) || typeof raw.id !== "string") return null;
    if (!Array.isArray(raw.tabs)) return null;
    const tabs = [];
    for (const tab of raw.tabs) {
      if (!isRecord(tab) || typeof tab.pinned !== "boolean") return null;
      const ref = readRef(tab.ref);
      if (ref === null) return null;
      tabs.push({ ref, pinned: tab.pinned });
    }
    const activeKey = raw.activeKey;
    if (activeKey !== null && typeof activeKey !== "string") return null;
    groups.push({ id: raw.id, tabs, activeKey });
  }

  const groupSizes = readNumbers(value.groupSizes);
  const viewStates = readViewStates(value.viewStates);
  if (groupSizes === null || viewStates === null) return null;
  return { groups, focusedGroupId: value.focusedGroupId, groupSizes, viewStates };
}

function readRef(value: unknown): DocumentRef | null {
  if (!isRecord(value)) return null;
  const { typeId, docId, title } = value;
  if (typeof typeId !== "string" || typeof docId !== "string") return null;
  if (typeof title !== "string") return null;
  return { typeId, docId, title };
}

function readNumbers(value: unknown): Record<string, number> | null {
  if (value === undefined) return {};
  if (!isRecord(value)) return null;
  const out: Record<string, number> = {};
  for (const [key, px] of Object.entries(value)) {
    if (typeof px !== "number" || !Number.isFinite(px) || px <= 0) return null;
    out[key] = Math.round(px);
  }
  return out;
}

function readViewStates(value: unknown): Record<string, PersistedViewState> | null {
  if (value === undefined) return {};
  if (!isRecord(value)) return null;
  const out: Record<string, PersistedViewState> = {};
  for (const [key, state] of Object.entries(value)) {
    if (!isRecord(state)) return null;
    const { anchor, head, scrollTop } = state;
    if (!isOffset(anchor) || !isOffset(head) || !isOffset(scrollTop)) return null;
    out[key] = { anchor, head, scrollTop };
  }
  return out;
}

/** Offsets and scroll positions are finite and never negative. */
function isOffset(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function pick<T>(source: Record<string, T>, keep: (key: string) => boolean): Record<string, T> {
  const out: Record<string, T> = {};
  for (const [key, value] of Object.entries(source)) {
    if (keep(key)) out[key] = value;
  }
  return out;
}
