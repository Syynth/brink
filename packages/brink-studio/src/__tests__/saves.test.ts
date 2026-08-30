/**
 * Runtime save/load (W14/#3307, RULED): store round-trip, Load-attaches
 * (write-back) vs Fork-copies (new slot), the default-location setting,
 * and the launcher-road load (starts a session when idle, diverts to the
 * saved knot).
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  createStudioStore,
  localStorageSaveStore,
  LocalSessionProvider,
  type SaveStore,
} from "@brink/studio-store";
import type { SaveState } from "@brink/wasm-types";

function memStorage(): Pick<Storage, "getItem" | "setItem"> {
  const map = new Map<string, string>();
  return { getItem: (k) => map.get(k) ?? null, setItem: (k, v) => void map.set(k, v) };
}

const STATE: SaveState = {
  version: 1,
  globals: { gold: { Int: 6 } },
  global_ids: {},
  visits: [],
  turns: [],
  turn_index: 4,
  rng_seed: 1,
  previous_random: 2,
} as never;

function scriptedSession() {
  return {
    continueSingle: vi.fn(() => ({ type: "text", text: "line\n", tags: [] })),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    hasDebugInfo: vi.fn(() => true),
    debugBreakpoints: vi.fn(() => []),
    saveState: vi.fn((): SaveState => STATE),
    exportTranscript: vi.fn(() => ({
      version: 1,
      checksum: 7,
      parts: [{ part: "line", container: 0, line: 0 }],
    })),
    renderTranscript: vi.fn(() => [{ text: "the story so far", tags: [] }]),
    loadState: vi.fn(() => ({
      unknown_globals: [],
      unresolved_renames: [],
      anonymous_states_dropped: 0,
    })),
  };
}

function storeWith(session = scriptedSession()) {
  const store = createStudioStore();
  const stores: Record<"project" | "local", SaveStore> = {
    project: localStorageSaveStore(memStorage(), "p"),
    local: localStorageSaveStore(memStorage(), "l"),
  };
  store.getState().setSaveStores(stores);
  const provider = new LocalSessionProvider({
    session: session as never,
    status: "running",
  });
  store.getState()._bindProvider(provider);
  store.setState({
    debugState: { turn_index: 4, current_location: "barter" } as never,
    programChecksum: "0xabc",
  });
  return { store, session, stores };
}

beforeEach(() => void 0);

describe("save/load (W14/#3307)", () => {
  it("localStorage store round-trips and allocates fresh slot ids", async () => {
    const s = localStorageSaveStore(memStorage(), "k");
    const meta = { name: "One", turn: 3, knotPath: "shop", checksum: "0x1", savedAt: 5 };
    const written = await s.write(null, { meta, state: STATE });
    expect(written.id).toBe("s1");
    const second = await s.write(null, { meta: { ...meta, name: "Two" }, state: STATE });
    expect(second.id).toBe("s2");
    expect((await s.read("s1"))?.meta.name).toBe("One");
    expect((await s.list()).map((m) => m.name)).toEqual(["One", "Two"]);
    await s.remove("s1");
    expect((await s.list()).map((m) => m.name)).toEqual(["Two"]);
  });

  it("saveCurrentState lands in the default location and ATTACHES; the next save writes back", async () => {
    const { store } = storeWith();
    store.getState().setSaveLocationDefault("project");

    await store.getState().saveCurrentState();
    expect(store.getState().attachedSlot).toEqual({ location: "project", id: "s1" });
    expect(store.getState().saveSlots.project).toHaveLength(1);
    expect(store.getState().saveSlots.project[0]).toMatchObject({
      turn: 4,
      knotPath: "barter",
      checksum: "0xabc",
    });

    // Attached: write-back, no new slot.
    await store.getState().saveCurrentState();
    expect(store.getState().saveSlots.project).toHaveLength(1);
  });

  it("Load attaches; Fork loads a copy unattached (next save = new slot)", async () => {
    const { store, session } = storeWith();
    await store.getState().saveCurrentState(); // s1 in local (default)

    await store.getState().loadSave("local", "s1");
    expect(session.loadState).toHaveBeenCalled();
    expect(session.goToPath).toHaveBeenCalledWith("barter");
    expect(store.getState().attachedSlot).toEqual({ location: "local", id: "s1" });

    await store.getState().forkSave("local", "s1");
    expect(store.getState().attachedSlot).toBeNull();
    await store.getState().saveCurrentState();
    // The fork's save allocated a NEW slot; the checkpoint is untouched.
    expect(store.getState().saveSlots.local).toHaveLength(2);
  });

  it("a save carries the STRUCTURAL transcript; a load re-renders it (RULED 2026-08-30)", async () => {
    const { store, session, stores } = storeWith();
    await store.getState().saveCurrentState();

    // The payload stores the part stream — not resolved text.
    expect(session.exportTranscript).toHaveBeenCalled();
    const payload = await stores.local.read("s1");
    expect(payload?.transcript).toMatchObject({ version: 1 });

    await store.getState().loadSave("local", "s1");
    // Rendered against the CURRENT program at load time — the seam that
    // makes an edited line's restored row show the edited text.
    expect(session.renderTranscript).toHaveBeenCalledWith(
      expect.objectContaining({ version: 1 }),
    );
    expect(store.getState().sessionText.join(" ")).toContain("the story so far");
  });

  it("removing the attached slot detaches", async () => {
    const { store } = storeWith();
    await store.getState().saveCurrentState();
    const attached = store.getState().attachedSlot;
    expect(attached).not.toBeNull();
    await store.getState().removeSave(attached!.location, attached!.id);
    expect(store.getState().attachedSlot).toBeNull();
  });
});
