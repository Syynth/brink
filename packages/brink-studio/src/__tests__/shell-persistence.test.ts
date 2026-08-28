/**
 * Layout persistence tests (shell issue 3.2 / #88, spec §7.1): versioned
 * snapshot round-trip, lenient loading, debounced writes, and unknown-id
 * cleanup via the registry sync that follows a restore.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  attachLayoutPersistence,
  createShellLayoutStore,
  LAYOUT_STORAGE_KEY,
  loadLayoutSnapshot,
  snapshotLayout,
  type ToolWindowDescriptor,
} from "@brink/studio-shell";

function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    dump: () => map.get(LAYOUT_STORAGE_KEY) ?? null,
  };
}

const descriptor = (id: string, dock: "left" | "right" | "bottom"): ToolWindowDescriptor => ({
  id,
  title: id,
  icon: null,
  defaultPlacement: { dock, section: "start" },
  defaultOpen: false,
  component: () => null,
});

function validPayload() {
  return {
    version: 1,
    placements: {
      binder: { dock: "bottom", section: "end" },
      player: { dock: "right", section: "start" },
    },
    open: { "bottom.end": "binder", "right.start": "player" },
    dockSizes: { left: 200, right: 333.4, bottom: 150 },
    maximized: null,
  };
}

describe("loadLayoutSnapshot", () => {
  it("round-trips a valid payload (sizes rounded, missing open keys null)", () => {
    const storage = memoryStorage({ [LAYOUT_STORAGE_KEY]: JSON.stringify(validPayload()) });
    const snap = loadLayoutSnapshot(storage);
    expect(snap).not.toBeNull();
    expect(snap!.placements.binder).toEqual({ dock: "bottom", section: "end" });
    expect(snap!.open["bottom.end"]).toBe("binder");
    expect(snap!.open["left.start"]).toBeNull();
    expect(snap!.dockSizes.right).toBe(333);
  });

  it("rejects garbage, wrong versions, and invalid shapes", () => {
    const bad = [
      "not json",
      JSON.stringify({ ...validPayload(), version: 2 }),
      JSON.stringify({ ...validPayload(), placements: { x: { dock: "middle", section: "start" } } }),
      JSON.stringify({ ...validPayload(), dockSizes: { left: -5, right: 1, bottom: 1 } }),
      JSON.stringify([1, 2, 3]),
    ];
    for (const raw of bad) {
      expect(loadLayoutSnapshot(memoryStorage({ [LAYOUT_STORAGE_KEY]: raw }))).toBeNull();
    }
    expect(loadLayoutSnapshot(memoryStorage())).toBeNull();
    expect(
      loadLayoutSnapshot({
        getItem: () => {
          throw new Error("denied");
        },
      }),
    ).toBeNull();
  });

  it("clears open occupants whose placement disagrees with their section", () => {
    const payload = validPayload();
    payload.open = { "left.start": "binder" } as never; // binder is placed bottom.end
    const snap = loadLayoutSnapshot(
      memoryStorage({ [LAYOUT_STORAGE_KEY]: JSON.stringify(payload) }),
    );
    expect(snap!.open["left.start"]).toBeNull();
  });

  it("restore + syncFromRegistry drops unknown ids and keeps surviving state", () => {
    const store = createShellLayoutStore();
    const snap = loadLayoutSnapshot(
      memoryStorage({ [LAYOUT_STORAGE_KEY]: JSON.stringify(validPayload()) }),
    );
    store.setState(snap!);
    // Registry knows binder but not player; a new "problems" window appears.
    store.getState().syncFromRegistry([descriptor("binder", "left"), descriptor("problems", "bottom")]);
    const s = store.getState();
    expect(s.placements.binder).toEqual({ dock: "bottom", section: "end" }); // user move kept
    expect(s.placements.player).toBeUndefined(); // unknown id dropped
    expect(s.open["right.start"]).toBeNull();
    expect(s.placements.problems).toEqual({ dock: "bottom", section: "start" }); // new id seeded
  });
});

describe("attachLayoutPersistence", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("writes the durable subset debounced; transient changes don't write", () => {
    const store = createShellLayoutStore();
    store.getState().syncFromRegistry([descriptor("binder", "left")]);
    const storage = memoryStorage();
    const detach = attachLayoutPersistence(store, storage, null);

    vi.advanceTimersByTime(1000);
    const afterSync = storage.dump(); // sync ran before attach — no write yet

    store.getState().setTier("medium"); // transient only
    vi.advanceTimersByTime(1000);
    expect(storage.dump()).toBe(afterSync);

    store.getState().moveToolWindow("binder", "bottom", "end");
    expect(storage.dump()).toBe(afterSync); // not yet — debounced
    vi.advanceTimersByTime(300);
    const written = JSON.parse(storage.dump()!);
    expect(written.version).toBe(1);
    expect(written.placements.binder).toEqual({ dock: "bottom", section: "end" });
    expect(written).not.toHaveProperty("tier");
    detach();
  });

  it("detach flushes a pending write", () => {
    const store = createShellLayoutStore();
    store.getState().syncFromRegistry([descriptor("binder", "left")]);
    const storage = memoryStorage();
    const detach = attachLayoutPersistence(store, storage, null);
    store.getState().setDockSize("left", 300);
    detach(); // no timer advance — flush must write
    expect(JSON.parse(storage.dump()!).dockSizes.left).toBe(300);
  });

  it("snapshotLayout picks exactly the durable fields", () => {
    const store = createShellLayoutStore();
    const snap = snapshotLayout(store.getState());
    // `editorView` joined the durable set with Single File view (decision
    // log 2026-08-26): which view you write in is a preference that should
    // outlive a reload, like the dock layout around it.
    //
    // `singleFileCompanionOpen` joined it for the same reason (#3165) —
    // whether you keep the player visible is a working-style preference,
    // not a property of a story, so it is global rather than per-project.
    expect(Object.keys(snap).sort()).toEqual([
      "dockSizes",
      "editorView",
      "maximized",
      "open",
      "placements",
      "singleFileCompanionOpen",
    ]);
  });

  it("round-trips the Single File companion toggle, and defaults it OPEN", () => {
    // The bug (#3165): this was component-local `useState(true)`, so every
    // mount reopened the split — a reload, and also switching to Code view
    // and back. The cost fell entirely on authors who had deliberately
    // hidden it; anyone happy with it open never noticed.
    const storage = memoryStorage();

    const store = createShellLayoutStore();
    expect(store.getState().singleFileCompanionOpen).toBe(true);
    store.getState().setSingleFileCompanionOpen(false);
    storage.setItem(
      LAYOUT_STORAGE_KEY,
      // `version` is stamped by the writer, not by `snapshotLayout` — a
      // hand-built payload without it is rejected wholesale.
      JSON.stringify({ version: 1, ...snapshotLayout(store.getState()) }),
    );

    expect(loadLayoutSnapshot(storage)?.singleFileCompanionOpen).toBe(false);
  });

  it("treats a payload written before the toggle existed as open", () => {
    // Lenient on purpose, like `editorView`: an older payload has no such
    // key, and losing a whole dock layout over one missing boolean would be
    // the worse trade.
    // `validPayload()` IS a pre-#3165 payload — it predates the key, which
    // is exactly the case under test. Building a minimal object by hand
    // instead would test the other field validators rather than this one.
    const storage = memoryStorage({
      [LAYOUT_STORAGE_KEY]: JSON.stringify(validPayload()),
    });
    expect("singleFileCompanionOpen" in validPayload()).toBe(false);

    expect(loadLayoutSnapshot(storage)?.singleFileCompanionOpen).toBe(true);
  });
});
