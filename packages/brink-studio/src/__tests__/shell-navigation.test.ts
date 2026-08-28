/**
 * @brink/studio-shell unit tests — Location/navigation protocol (shell issue
 * 1.4, spec §6.1): resolver registry, symbol→source over the outline, and
 * view.reveal receivers.
 */

import { describe, expect, it, vi } from "vitest";
import {
  LocationResolvers,
  resolveQualifiedSymbol,
  ViewRevealHandlers,
  encodeProgramAddress,
  parseProgramAddress,
  makeProgramResolver,
  resolveSessionPositionRef,
  type Location,
  type OutlineFileLike,
  type ProgramSourceLocation,
} from "@brink/studio-shell";

const OUTLINE: OutlineFileLike[] = [
  {
    path: "main.ink",
    symbols: [
      { name: "intro", start: 10, end: 15, children: [] },
      {
        name: "warden",
        start: 40,
        end: 46,
        children: [
          { name: "turn", start: 60, end: 64, children: [] },
          { name: "specials", start: 90, end: 98, children: [] },
        ],
      },
    ],
  },
  {
    path: "extra.ink",
    symbols: [{ name: "intro", start: 5, end: 9, children: [] }],
  },
];

describe("resolveQualifiedSymbol", () => {
  it("resolves a top-level knot to its header span", () => {
    expect(resolveQualifiedSymbol(OUTLINE, "warden")).toEqual({
      kind: "source",
      file: "main.ink",
      span: { start: 40, end: 46 },
    });
  });

  it("resolves a qualified stitch through children", () => {
    expect(resolveQualifiedSymbol(OUTLINE, "warden.specials")).toEqual({
      kind: "source",
      file: "main.ink",
      span: { start: 90, end: 98 },
    });
  });

  it("is deterministic: first match in file order wins", () => {
    expect(resolveQualifiedSymbol(OUTLINE, "intro")?.file).toBe("main.ink");
  });

  it("returns null for misses and empty names", () => {
    expect(resolveQualifiedSymbol(OUTLINE, "nope")).toBeNull();
    expect(resolveQualifiedSymbol(OUTLINE, "warden.nope")).toBeNull();
    expect(resolveQualifiedSymbol(OUTLINE, "")).toBeNull();
  });
});

describe("LocationResolvers", () => {
  it("source locations resolve to themselves", () => {
    const resolvers = new LocationResolvers();
    const source: Location = { kind: "source", file: "a.ink", span: { start: 1, end: 2 } };
    expect(resolvers.resolve(source)).toBe(source);
  });

  it("chains across spaces toward source", () => {
    const resolvers = new LocationResolvers();
    resolvers.register("session", (loc) =>
      loc.kind === "session" ? { kind: "program", address: String(loc.ref) } : null,
    );
    resolvers.register("program", (loc) =>
      loc.kind === "program" ? { kind: "symbol", name: loc.address } : null,
    );
    resolvers.register("symbol", (loc) =>
      loc.kind === "symbol" ? resolveQualifiedSymbol(OUTLINE, loc.name) : null,
    );

    expect(resolvers.resolve({ kind: "session", ref: "warden.turn" })).toEqual({
      kind: "source",
      file: "main.ink",
      span: { start: 60, end: 64 },
    });
  });

  it("returns null without a resolver for the kind, or when a step fails", () => {
    const resolvers = new LocationResolvers();
    expect(resolvers.resolve({ kind: "symbol", name: "intro" })).toBeNull();
    resolvers.register("symbol", () => null);
    expect(resolvers.resolve({ kind: "symbol", name: "intro" })).toBeNull();
  });

  it("guards against resolver cycles via the step cap", () => {
    const resolvers = new LocationResolvers();
    resolvers.register("program", (loc) =>
      loc.kind === "program" ? { kind: "session", ref: loc.address } : null,
    );
    resolvers.register("session", (loc) =>
      loc.kind === "session" ? { kind: "program", address: String(loc.ref) } : null,
    );
    expect(resolvers.resolve({ kind: "program", address: "x" })).toBeNull();
  });

  it("rejects duplicate resolvers; disposer frees the slot", () => {
    const resolvers = new LocationResolvers();
    const dispose = resolvers.register("symbol", () => null);
    expect(() => resolvers.register("symbol", () => null)).toThrow(/duplicate/);
    dispose();
    resolvers.register("symbol", () => null);
  });
});

describe("ViewRevealHandlers", () => {
  it("dispatches to the registered receiver and reports unhandled views", () => {
    const handlers = new ViewRevealHandlers();
    const received = vi.fn();
    const dispose = handlers.register("binder", received);

    expect(handlers.reveal("binder", { id: 7 })).toBe(true);
    expect(received).toHaveBeenCalledWith({ id: 7 });
    expect(handlers.reveal("graph", {})).toBe(false);

    dispose();
    expect(handlers.reveal("binder", {})).toBe(false);
  });

  it("rejects duplicate receivers", () => {
    const handlers = new ViewRevealHandlers();
    handlers.register("binder", () => {});
    expect(() => handlers.register("binder", () => {})).toThrow(/duplicate/);
  });
});

// ── program→source resolver (D9, issue #3187) ───────────────────────
//
// Unlike the mocked "program"/"session" resolvers above (which only prove
// `LocationResolvers`' own chaining/cycle-guard mechanics), these exercise
// the REAL resolver this ticket builds: `parseProgramAddress` /
// `encodeProgramAddress` (the wire encoding for the `program` Location
// space's `address` string) and `makeProgramResolver` /
// `resolveSessionPositionRef` (the actual "session → program → source"
// chain `docs/studio-shell-spec.md` §6.1 names as this ticket's job). The
// wasm call itself (`resolveDebugPosition`) is injected as a fake here —
// its own correctness is proven Rust-side over a real `StoryRunner`/
// `WebSession` (`crates/brink-web/src/story_runner.rs`,
// `crates/brink-web/src/session.rs`) — this suite proves the TS-side
// wiring: address encode/decode, the resolver chain, and the "unresolvable
// means null, not a throw" contract.

describe("program address encoding", () => {
  it("round-trips through encode/parse", () => {
    expect(parseProgramAddress(encodeProgramAddress(3, 17))).toEqual({
      containerIdx: 3,
      offset: 17,
    });
  });

  it("rejects malformed addresses without throwing", () => {
    expect(parseProgramAddress("not-an-address")).toBeNull();
    expect(parseProgramAddress("3")).toBeNull();
    expect(parseProgramAddress("3:17:99")).toBeNull();
    expect(parseProgramAddress("-1:17")).toBeNull();
    expect(parseProgramAddress("3:-1")).toBeNull();
    expect(parseProgramAddress("3.5:17")).toBeNull();
    // Number()-based parsing previously accepted all of these silently:
    // Number("") === 0 makes "3:" -> {containerIdx:3, offset:0} and ":5" ->
    // {containerIdx:0, offset:5} instead of null, and Number() also accepts
    // hex, exponent notation, and surrounding whitespace.
    expect(parseProgramAddress("3:")).toBeNull();
    expect(parseProgramAddress(":5")).toBeNull();
    expect(parseProgramAddress("0x10:0")).toBeNull();
    expect(parseProgramAddress("1e3:0")).toBeNull();
    expect(parseProgramAddress(" 3:7")).toBeNull();
  });
});

describe("makeProgramResolver", () => {
  it("resolves a program address to source via the injected position resolver", () => {
    const resolvePosition = vi.fn(
      (containerIdx: number, offset: number): ProgramSourceLocation | null =>
        containerIdx === 2 && offset === 40
          ? { file: "main.ink", range_start: 40, range_len: 6 }
          : null,
    );
    const resolver = makeProgramResolver(resolvePosition);

    expect(resolver({ kind: "program", address: encodeProgramAddress(2, 40) })).toEqual({
      kind: "source",
      file: "main.ink",
      span: { start: 40, end: 46 },
    });
    expect(resolvePosition).toHaveBeenCalledWith(2, 40);
  });

  it("returns null (not a throw) when the position doesn't resolve", () => {
    // No DebugInfo section compiled, or an out-of-range position — both
    // report through the same "doesn't resolve" contract as the wasm call.
    const resolver = makeProgramResolver(() => null);
    expect(resolver({ kind: "program", address: encodeProgramAddress(0, 0) })).toBeNull();
  });

  it("returns null for the reserved synthetic sentinel (file: null)", () => {
    const resolver = makeProgramResolver(() => ({
      file: null,
      range_start: 0,
      range_len: 0,
    }));
    expect(resolver({ kind: "program", address: encodeProgramAddress(0, 0) })).toBeNull();
  });

  it("returns null for a malformed address and for a non-program location", () => {
    const resolver = makeProgramResolver(() => ({
      file: "main.ink",
      range_start: 0,
      range_len: 1,
    }));
    expect(resolver({ kind: "program", address: "garbage" })).toBeNull();
    expect(resolver({ kind: "symbol", name: "intro" })).toBeNull();
  });

  it("ignores checksum/degraded-mode entirely — that gate is the caller's job", () => {
    // makeProgramResolver has no session-state dependency; a caller MUST
    // check sessionDegraded() before wiring resolvePosition at all
    // (docs/live-inspector-spec.md §5). This test documents that boundary:
    // the resolver itself will happily resolve if asked to.
    const resolver = makeProgramResolver(() => ({
      file: "main.ink",
      range_start: 0,
      range_len: 1,
    }));
    expect(resolver({ kind: "program", address: encodeProgramAddress(0, 0) })).not.toBeNull();
  });
});

describe("resolveSessionPositionRef", () => {
  it("extracts a program address from a DebugFrame-shaped session ref", () => {
    const ref = { kind: "function", location: "warden.turn", position: { container_idx: 2, offset: 40 }, temps: 0 };
    expect(resolveSessionPositionRef({ kind: "session", ref })).toEqual({
      kind: "program",
      address: encodeProgramAddress(2, 40),
    });
  });

  it("returns null for a ref with no position (e.g. an external frame)", () => {
    const ref = { kind: "external", location: undefined, position: undefined, temps: 0 };
    expect(resolveSessionPositionRef({ kind: "session", ref })).toBeNull();
  });

  it("returns null for a non-session location and a malformed ref", () => {
    expect(resolveSessionPositionRef({ kind: "symbol", name: "intro" })).toBeNull();
    expect(resolveSessionPositionRef({ kind: "session", ref: "just a string" })).toBeNull();
    expect(resolveSessionPositionRef({ kind: "session", ref: null })).toBeNull();
  });
});

describe("the full session → program → source chain", () => {
  it("resolves a State View call-stack frame all the way to a source span", () => {
    const resolvers = new LocationResolvers();
    resolvers.register("session", resolveSessionPositionRef);
    resolvers.register(
      "program",
      makeProgramResolver((containerIdx, offset) =>
        containerIdx === 2 && offset === 40
          ? { file: "main.ink", range_start: 40, range_len: 6 }
          : null,
      ),
    );

    const frame = {
      kind: "function",
      location: "warden.turn",
      position: { container_idx: 2, offset: 40 },
      temps: 0,
    };
    expect(resolvers.resolve({ kind: "session", ref: frame })).toEqual({
      kind: "source",
      file: "main.ink",
      span: { start: 40, end: 46 },
    });
  });

  it("degrades to null when the session resolver finds no position (stale/parked frame)", () => {
    const resolvers = new LocationResolvers();
    resolvers.register("session", resolveSessionPositionRef);
    resolvers.register("program", makeProgramResolver(() => null));

    const externalFrame = { kind: "external", location: "shout", position: undefined, temps: 0 };
    expect(resolvers.resolve({ kind: "session", ref: externalFrame })).toBeNull();
  });
});
