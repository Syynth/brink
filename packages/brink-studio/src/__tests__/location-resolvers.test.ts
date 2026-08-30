/**
 * W3/#3296 — the resolver REGISTRATION, over a real store.
 *
 * `LocationResolvers`' mechanics and `makeProgramResolver`'s own logic are
 * pinned by `shell-navigation.test.ts`; what was missing (the D9 survey's
 * "built — never registered" gap) is the wiring: that a program location
 * actually reaches the live provider's DebugInfo road, that the
 * `sessionDegraded` gate lives at this caller, and that a position-shaped
 * session ref chains all the way to source.
 */
import { describe, expect, it, vi } from "vitest";
import { LocationResolvers, encodeProgramAddress } from "@brink/studio-shell";
import { createStudioStore, ALL_CAPABILITIES } from "@brink/studio-store";
import { registerLocationResolvers } from "../location-resolvers";

function wired(overrides: {
  programChecksum?: string | null;
  compiledChecksum?: string | null;
  capabilities?: ReadonlySet<string>;
  resolve?: ReturnType<typeof vi.fn>;
}) {
  const store = createStudioStore();
  const resolve =
    overrides.resolve ??
    vi.fn(() => ({ file: "main.ink", range_start: 10, range_len: 7 }));
  store.setState({
    programChecksum: overrides.programChecksum ?? "abc",
    compiledChecksum: overrides.compiledChecksum ?? "abc",
    _provider: {
      capabilities: overrides.capabilities ?? ALL_CAPABILITIES,
      resolveDebugPosition: resolve,
    } as never,
  });
  const locations = new LocationResolvers();
  registerLocationResolvers(locations, store);
  return { locations, resolve };
}

describe("registerLocationResolvers (W3/#3296)", () => {
  it("resolves a program location to source through the live provider", () => {
    const { locations, resolve } = wired({});
    const target = locations.resolve({
      kind: "program",
      address: encodeProgramAddress(3, 42),
    });
    expect(resolve).toHaveBeenCalledWith(3, 42);
    expect(target).toEqual({
      kind: "source",
      file: "main.ink",
      span: { start: 10, end: 17 },
    });
  });

  it("suppresses under a degraded session — never resolves stale", () => {
    const { locations, resolve } = wired({
      programChecksum: "old",
      compiledChecksum: "new",
    });
    expect(
      locations.resolve({ kind: "program", address: encodeProgramAddress(3, 42) }),
    ).toBeNull();
    // Suppression happens BEFORE the provider is consulted: a stale answer
    // must not even be computed, let alone rendered.
    expect(resolve).not.toHaveBeenCalled();
  });

  it("returns null for an observe-only provider (no debug capability)", () => {
    const { locations, resolve } = wired({ capabilities: new Set(["start"]) });
    expect(
      locations.resolve({ kind: "program", address: encodeProgramAddress(0, 0) }),
    ).toBeNull();
    expect(resolve).not.toHaveBeenCalled();
  });

  it("chains a position-shaped session ref through program to source", () => {
    const { locations, resolve } = wired({});
    const target = locations.resolve({
      kind: "session",
      ref: { position: { container_idx: 5, offset: 9 } },
    });
    expect(resolve).toHaveBeenCalledWith(5, 9);
    expect(target?.kind).toBe("source");
  });

  it("still resolves symbols over the compile outline", () => {
    const { locations } = wired({});
    // The store's outline is empty by default — an unknown symbol is null,
    // which proves the resolver is registered (an UNregistered kind would
    // also be null; so also check a real hit through a seeded outline).
    expect(locations.resolve({ kind: "symbol", name: "nope" })).toBeNull();

    const store = createStudioStore();
    store.setState({
      outline: [
        {
          path: "main.ink",
          symbols: [{ name: "tavern", start: 4, end: 40, children: [] }],
        },
      ] as never,
    });
    const locations2 = new LocationResolvers();
    registerLocationResolvers(locations2, store);
    expect(locations2.resolve({ kind: "symbol", name: "tavern" })).toEqual({
      kind: "source",
      file: "main.ink",
      span: { start: 4, end: 40 },
    });
  });
});
