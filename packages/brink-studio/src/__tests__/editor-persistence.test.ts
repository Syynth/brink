/**
 * Editor-state persistence (`@brink/studio-shell`'s `editor-persistence`).
 *
 * The durable half of the editor-groups store, scoped per project (decision
 * log 2026-08-26). These cover the three things that actually bite: the
 * round trip, the least-recently-used eviction that makes per-project keying
 * safe, and the reconciliation that runs when the files behind a restored
 * tab are no longer there.
 */

import { describe, expect, it, vi } from "vitest";

import {
  EDITOR_STORAGE_KEY,
  MAX_SCOPES,
  attachEditorPersistence,
  createEditorGroupsStore,
  documentKey,
  loadEditorSnapshot,
  reconcileEditorSnapshot,
  type DocumentRef,
  type EditorSnapshot,
} from "@brink/studio-shell";

/** A minimal in-memory Storage stand-in. */
function fakeStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    raw: map,
  };
}

const fileRef = (path: string): DocumentRef => ({
  typeId: "ink-file",
  docId: path,
  title: path,
});

/** The store's own key spelling — `JSON.stringify([typeId, docId])`. */
const keyOf = (path: string): string => documentKey(fileRef(path));

function snapshotWith(paths: string[]): EditorSnapshot {
  return {
    groups: [
      {
        id: "group-1",
        tabs: paths.map((p) => ({ ref: fileRef(p), pinned: true })),
        activeKey: paths.length > 0 ? keyOf(paths[0]) : null,
      },
    ],
    focusedGroupId: "group-1",
    groupSizes: {},
    viewStates: {},
  };
}

describe("editor persistence", () => {
  it("round-trips a scope's snapshot", () => {
    vi.useFakeTimers();
    const storage = fakeStorage();
    const store = createEditorGroupsStore();
    const detach = attachEditorPersistence(
      store,
      storage,
      { scope: "/projects/a", viewStates: () => ({ [keyOf("main.ink")]: { anchor: 4, head: 9, scrollTop: 120 } }) },
      null,
    );
    store.getState().openDocument(fileRef("main.ink"));
    vi.runAllTimers();
    detach();
    vi.useRealTimers();

    const loaded = loadEditorSnapshot(storage, "/projects/a");
    expect(loaded?.groups[0].tabs.map((t) => t.ref.docId)).toEqual(["main.ink"]);
    expect(loaded?.viewStates[keyOf("main.ink")]).toEqual({ anchor: 4, head: 9, scrollTop: 120 });
    // A scope nobody wrote reads as "nothing remembered", not as a throw.
    expect(loadEditorSnapshot(storage, "/projects/never-opened")).toBe(null);
  });

  it("keeps scopes apart so two projects do not overwrite each other", () => {
    vi.useFakeTimers();
    const storage = fakeStorage();
    for (const [scope, path] of [
      ["/projects/a", "a.ink"],
      ["/projects/b", "b.ink"],
    ] as const) {
      const store = createEditorGroupsStore();
      const detach = attachEditorPersistence(store, storage, { scope, viewStates: () => ({}) }, null);
      store.getState().openDocument(fileRef(path));
      vi.runAllTimers();
      detach();
    }
    vi.useRealTimers();

    expect(loadEditorSnapshot(storage, "/projects/a")?.groups[0].tabs[0].ref.docId).toBe("a.ink");
    expect(loadEditorSnapshot(storage, "/projects/b")?.groups[0].tabs[0].ref.docId).toBe("b.ink");
  });

  it("evicts the least recently written scope past the cap", () => {
    vi.useFakeTimers();
    const storage = fakeStorage();
    // One more project than the cap allows, oldest first.
    for (let i = 0; i <= MAX_SCOPES; i++) {
      const store = createEditorGroupsStore();
      const detach = attachEditorPersistence(
        store,
        storage,
        { scope: `/projects/${i}`, viewStates: () => ({}) },
        null,
      );
      store.getState().openDocument(fileRef(`file-${i}.ink`));
      vi.runAllTimers();
      detach();
    }
    vi.useRealTimers();

    // The first one written is the one that fell off; everything after it stayed.
    expect(loadEditorSnapshot(storage, "/projects/0")).toBe(null);
    expect(loadEditorSnapshot(storage, `/projects/${MAX_SCOPES}`)).not.toBe(null);
    const parsed = JSON.parse(storage.raw.get(EDITOR_STORAGE_KEY) ?? "{}") as {
      entries: unknown[];
    };
    expect(parsed.entries).toHaveLength(MAX_SCOPES);
  });

  it("captures cursor and scroll on detach even with no structural change pending", () => {
    // The regression this guards: cursor and scroll live outside the store,
    // so scrolling schedules no write. A flush that only fired when a write
    // was already pending would drop them, and reading to the bottom of a
    // file and reloading would land you back at the top.
    const storage = fakeStorage();
    const store = createEditorGroupsStore();
    let scrollTop = 0;
    const detach = attachEditorPersistence(
      store,
      storage,
      { scope: "/p", viewStates: () => ({ [keyOf("main.ink")]: { anchor: 0, head: 0, scrollTop } }) },
      null,
    );
    // Structural change first, flushed, so nothing is pending afterwards.
    vi.useFakeTimers();
    store.getState().openDocument(fileRef("main.ink"));
    vi.runAllTimers();
    vi.useRealTimers();

    // Now only the scroll moves — nothing the store can see.
    scrollTop = 1500;
    detach();

    expect(loadEditorSnapshot(storage, "/p")?.viewStates[keyOf("main.ink")]?.scrollTop).toBe(1500);
  });

  it("survives a corrupt or foreign-version payload by forgetting it", () => {
    expect(loadEditorSnapshot(fakeStorage({ [EDITOR_STORAGE_KEY]: "{not json" }), "s")).toBe(null);
    expect(
      loadEditorSnapshot(
        fakeStorage({ [EDITOR_STORAGE_KEY]: JSON.stringify({ version: 99, entries: [] }) }),
        "s",
      ),
    ).toBe(null);
  });

  describe("reconciliation against the files that still exist", () => {
    it("drops tabs whose file is gone and re-points the active tab", () => {
      const snapshot = snapshotWith(["gone.ink", "kept.ink"]);
      const reconciled = reconcileEditorSnapshot(snapshot, (ref) => ref.docId !== "gone.ink");
      expect(reconciled?.groups[0].tabs.map((t) => t.ref.docId)).toEqual(["kept.ink"]);
      // The active tab WAS the dropped one, so it falls back rather than
      // pointing at a tab that is no longer there.
      expect(reconciled?.groups[0].activeKey).toBe(keyOf("kept.ink"));
    });

    it("returns null when nothing survives, so the store keeps its own default", () => {
      expect(reconcileEditorSnapshot(snapshotWith(["gone.ink"]), () => false)).toBe(null);
    });

    it("forgets sizes and view states belonging to what it dropped", () => {
      const snapshot: EditorSnapshot = {
        ...snapshotWith(["kept.ink"]),
        groups: [
          { id: "group-1", tabs: [{ ref: fileRef("kept.ink"), pinned: true }], activeKey: keyOf("kept.ink") },
          { id: "group-2", tabs: [{ ref: fileRef("gone.ink"), pinned: true }], activeKey: keyOf("gone.ink") },
        ],
        groupSizes: { "group-1": 400, "group-2": 300 },
        viewStates: {
          [keyOf("kept.ink")]: { anchor: 1, head: 1, scrollTop: 0 },
          [keyOf("gone.ink")]: { anchor: 2, head: 2, scrollTop: 50 },
        },
      };
      const reconciled = reconcileEditorSnapshot(snapshot, (ref) => ref.docId !== "gone.ink");
      expect(reconciled?.groups.map((g) => g.id)).toEqual(["group-1"]);
      expect(reconciled?.groupSizes).toEqual({ "group-1": 400 });
      expect(Object.keys(reconciled?.viewStates ?? {})).toEqual([keyOf("kept.ink")]);
    });

    it("re-points the focused group when that group disappeared", () => {
      const snapshot: EditorSnapshot = {
        ...snapshotWith(["kept.ink"]),
        groups: [
          { id: "group-1", tabs: [{ ref: fileRef("kept.ink"), pinned: true }], activeKey: keyOf("kept.ink") },
          { id: "group-2", tabs: [{ ref: fileRef("gone.ink"), pinned: true }], activeKey: keyOf("gone.ink") },
        ],
        focusedGroupId: "group-2",
      };
      expect(reconcileEditorSnapshot(snapshot, (ref) => ref.docId !== "gone.ink")?.focusedGroupId).toBe(
        "group-1",
      );
    });
  });

  it("never re-mints a restored group's id on the next split", () => {
    // The counter lives in the store's closure, so seeding at construction
    // is the only way it can know "group-3" is already taken.
    const store = createEditorGroupsStore({
      groups: [
        { id: "group-1", tabs: [{ ref: fileRef("a.ink"), pinned: true }], activeKey: keyOf("a.ink") },
        { id: "group-3", tabs: [{ ref: fileRef("b.ink"), pinned: true }], activeKey: keyOf("b.ink") },
      ],
      focusedGroupId: "group-3",
      groupSizes: {},
    });
    store.getState().splitGroup();
    const ids = store.getState().groups.map((g) => g.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).not.toContain("group-2");
  });
});
