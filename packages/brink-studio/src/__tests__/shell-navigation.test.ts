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
  type Location,
  type OutlineFileLike,
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
