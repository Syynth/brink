/**
 * Hot reload (W15/#3308, spec §F8 REVISED): replay stays the primary
 * road (exact position + transcript survive a clean replay); when replay
 * DIVERGES, FAILS, or the reload throws outright, the durable state
 * migrates via the W14 checkpoint road instead of truncating or
 * dropping everything — globals/visits/turn survive the edit, the
 * position falls back to the recorded knot, and the chip's reloadedAt
 * flash marks the migration.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore, LocalSessionProvider } from "@brink/studio-store";
import type { ReplayOutcome, SaveState } from "@brink/wasm-types";

const STATE = { version: 1, globals: { gold: { Int: 6 } }, turn_index: 3 } as never as SaveState;

function scriptedSession(overrides: Record<string, unknown> = {}) {
  return {
    continueSingle: vi.fn(() => ({ type: "text", text: "line\n", tags: [] })),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => ({ status: "active", current_location: "barter" })),
    onJournalDirty: vi.fn(() => () => {}),
    hasDebugInfo: vi.fn(() => true),
    debugBreakpoints: vi.fn(() => []),
    programModel: vi.fn(() => ({ checksum: "0xnew" })),
    programInkt: vi.fn(() => ""),
    saveState: vi.fn((): SaveState => STATE),
    loadState: vi.fn(() => ({
      unknown_globals: [],
      unresolved_renames: [],
      anonymous_states_dropped: 0,
    })),
    reload: vi.fn(
      (): ReplayOutcome => ({ type: "replayed", warnings: [] }),
    ),
    ...overrides,
  };
}

function bind(session: ReturnType<typeof scriptedSession>) {
  const store = createStudioStore();
  const provider = new LocalSessionProvider({
    session: session as never,
    status: "running",
    persist: false,
  } as never);
  store.getState()._bindProvider(provider);
  // Seed the debug mirror so the migration records the current knot.
  provider.getSnapshot(); // no-op read
  return { store, provider };
}

describe("hot reload (W15/#3308)", () => {
  it("a clean replay keeps the session and flashes reloadedAt", () => {
    const session = scriptedSession();
    const { store, provider } = bind(session);
    provider.start(new Uint8Array([1]));
    expect(session.reload).toHaveBeenCalled();
    expect(session.loadState).not.toHaveBeenCalled();
    expect(store.getState().sessionReloadedAt).not.toBeNull();
  });

  it("a 'clean' replay that REGRESSES the turn migrates instead (journal-bypass reality, #3335)", () => {
    // A session played under armed breakpoints journals nothing (measured
    // live, #3335) — its replay reports "replayed" while landing at the
    // START. The turn regression is the tell.
    const session = scriptedSession({
      // Pre-reload the session sits at turn 3…
      debugSnapshot: vi.fn(() => ({
        status: "active",
        current_location: "barter",
        turn_index: 3,
      })),
    });
    const { store, provider } = bind(session);
    provider.pause(); // prime the debug mirror (turn 3)
    // …but the replayed session reports turn 0.
    session.debugSnapshot.mockReturnValue({
      status: "active",
      current_location: "start",
      turn_index: 0,
    } as never);
    provider.start(new Uint8Array([1]));

    expect(session.restart).toHaveBeenCalled();
    expect(session.loadState).toHaveBeenCalledWith(STATE);
    expect(session.goToPath).toHaveBeenCalledWith("barter");
    expect(store.getState().sessionReloadedAt).not.toBeNull();
  });

  it("a DIVERGED replay migrates durable state instead of truncating", () => {
    const session = scriptedSession({
      reload: vi.fn(
        (): ReplayOutcome => ({
          type: "diverged",
          at_event: 2,
          expected: {} as never,
          found: {} as never,
        }),
      ),
    });
    const { store, provider } = bind(session);
    // Prime the debug mirror with a current location for the divert.
    provider.pause();
    provider.start(new Uint8Array([1]));

    expect(session.saveState).toHaveBeenCalled();
    expect(session.restart).toHaveBeenCalled();
    expect(session.loadState).toHaveBeenCalledWith(STATE);
    expect(session.goToPath).toHaveBeenCalledWith("barter");
    expect(store.getState().sessionReloadedAt).not.toBeNull();
  });

  it("a FAILED replay migrates too", () => {
    const session = scriptedSession({
      reload: vi.fn(
        (): ReplayOutcome => ({
          type: "failed",
          at_event: 1,
          reason: { type: "runtime_error", message: "boom" } as never,
        }),
      ),
    });
    const { provider } = bind(session);
    provider.pause();
    provider.start(new Uint8Array([1]));
    expect(session.loadState).toHaveBeenCalledWith(STATE);
  });

  it("a lossy migration surfaces the report as a Reloaded transcript notice", () => {
    const session = scriptedSession({
      reload: vi.fn((): ReplayOutcome => {
        throw new Error("link failure");
      }),
    });
    const { store, provider } = bind(session);
    // The migration runs on the FRESH session the factory builds — that
    // one reports the lossy load.
    const factory = vi.fn(
      () =>
        scriptedSession({
          loadState: vi.fn(() => ({
            unknown_globals: [],
            unresolved_renames: [],
            anonymous_states_dropped: 2,
          })),
        }) as never,
    );
    (provider as unknown as { sessionFactory: unknown }).sessionFactory = factory;
    provider.pause();
    provider.start(new Uint8Array([1]));

    // The reload threw → fresh session → migration ran on it.
    expect(factory).toHaveBeenCalled();
    expect(store.getState().sessionText.join(" ")).toContain(
      "Reloaded — 2 anonymous visit states dropped",
    );
  });
});
