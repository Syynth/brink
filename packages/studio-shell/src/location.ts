/**
 * @brink/studio-shell — locations and navigation (docs/studio-shell-spec.md §6.1).
 *
 * Cross-surface linking is one protocol, not per-view behavior: a Location
 * names a place in one of the studio's four address spaces, and a resolver
 * registry translates step-by-step toward source space. Views emit whatever
 * space they naturally have; the `editor.reveal` command resolves and jumps.
 *
 * This package defines the protocol and the symbol→source resolution helper
 * (structural over the compile outline). The source resolver is the identity;
 * program (Compiled Output, #91) and session (State View links) resolvers
 * land with their consumers.
 */

export interface Span {
  start: number;
  end: number;
}

export type Location =
  | { kind: "source"; file: string; span: Span }
  | { kind: "symbol"; name: string } // qualified: "knot" or "knot.stitch"
  | { kind: "program"; address: string } // container path / bytecode address
  | { kind: "session"; ref: unknown }; // transcript entry, stack frame, …

export type SourceLocation = Extract<Location, { kind: "source" }>;

export const EDITOR_REVEAL_COMMAND_ID = "editor.reveal";
export const VIEW_REVEAL_COMMAND_ID = "view.reveal";

/** One translation step toward source; null when the place doesn't resolve. */
export type LocationResolver = (location: Location) => Location | null;

/** Spaces never chain deeper than session → program → symbol → source. */
const MAX_RESOLUTION_STEPS = 4;

export class LocationResolvers {
  private readonly resolvers = new Map<string, LocationResolver>();

  /** Register the resolver for one non-source kind. Throws on duplicates. */
  register(kind: Exclude<Location["kind"], "source">, resolver: LocationResolver): () => void {
    if (this.resolvers.has(kind)) {
      throw new Error(`duplicate location resolver for kind "${kind}"`);
    }
    this.resolvers.set(kind, resolver);
    return () => {
      this.resolvers.delete(kind);
    };
  }

  /**
   * Translate toward source. Returns null when a step has no resolver or a
   * resolver can't place the location; the step cap guards cycles.
   */
  resolve(location: Location): SourceLocation | null {
    let current = location;
    for (let step = 0; step < MAX_RESOLUTION_STEPS; step++) {
      if (current.kind === "source") return current;
      const resolver = this.resolvers.get(current.kind);
      if (resolver === undefined) return null;
      const next = resolver(current);
      if (next === null) return null;
      current = next;
    }
    return current.kind === "source" ? current : null;
  }
}

// ── Symbol space → source, over the compile outline ─────────────────

/** Structural subset of wasm-types' DocumentSymbol/FileOutline. */
export interface OutlineSymbolLike {
  name: string;
  start: number;
  end: number;
  children: readonly OutlineSymbolLike[];
}
export interface OutlineFileLike {
  path: string;
  symbols: readonly OutlineSymbolLike[];
}

/**
 * Resolve a qualified symbol name ("knot" / "knot.stitch") to its header span
 * in the outline. First match wins, in file order then symbol order —
 * deterministic for a given outline.
 */
export function resolveQualifiedSymbol(
  files: readonly OutlineFileLike[],
  qualifiedName: string,
): SourceLocation | null {
  const parts = qualifiedName.split(".").filter((p) => p !== "");
  if (parts.length === 0) return null;

  for (const file of files) {
    let scope: readonly OutlineSymbolLike[] = file.symbols;
    let found: OutlineSymbolLike | undefined;
    for (const part of parts) {
      found = scope.find((s) => s.name === part);
      if (found === undefined) break;
      scope = found.children;
    }
    if (found !== undefined) {
      return { kind: "source", file: file.path, span: { start: found.start, end: found.end } };
    }
  }
  return null;
}

// ── Program space → source, via the runtime DebugInfo resolver (D9, #3187) ──
//
// The `program` resolver landed with its consumer, per this file's own doc
// ("program... resolvers land with their consumers") — that consumer is the
// debugger epic's D9 ticket. `address` encodes a runtime `DebugPosition` as
// `"containerIdx:offset"` (a plain string, matching this protocol's
// `{ kind: "program"; address: string }` shape — "container path / bytecode
// address").

/** A `(containerIdx, offset)` position resolved to source, or `null` when it
 *  doesn't resolve (no `DebugInfo` section, or an out-of-range position) —
 *  the shape `StoryRunnerHandle.resolveDebugPosition`/
 *  `StorySessionHandle.resolveDebugPosition` (`@brink-lang/web`) return. */
export interface ProgramSourceLocation {
  file: string | null;
  range_start: number;
  range_len: number;
}

/** Parse a `program` Location's `address` into the `(containerIdx, offset)`
 *  pair it encodes. `null` for a malformed address — never throws. */
export function parseProgramAddress(
  address: string,
): { containerIdx: number; offset: number } | null {
  if (!/^\d+:\d+$/.test(address)) return null;
  const parts = address.split(":");
  const containerIdx = Number.parseInt(parts[0], 10);
  const offset = Number.parseInt(parts[1], 10);
  return { containerIdx, offset };
}

/** Encode a `(containerIdx, offset)` runtime position as a `program`
 *  Location's `address` — the inverse of {@link parseProgramAddress}. */
export function encodeProgramAddress(containerIdx: number, offset: number): string {
  return `${containerIdx}:${offset}`;
}

/**
 * Build the `program` Location resolver: `{ kind: "program"; address }` →
 * `{ kind: "source" }`, via a caller-supplied position resolver (the actual
 * wasm-backed `resolveDebugPosition` call — injected so this stays testable
 * without wasm, per the house rule against browser-only proof).
 *
 * `resolvePosition` returning `null` means "this position does not resolve
 * to source" (no `DebugInfo` section compiled, or the position is stale/
 * out of range) — this resolver returns `null` in that case too, same as a
 * malformed address. It does NOT gate on program-identity/checksum itself —
 * `docs/live-inspector-spec.md` §5's degraded-mode gate belongs to the
 * caller wiring this in (compare `programChecksum`/`compiledChecksum`
 * BEFORE calling `resolvePosition` at all, per `sessionDegraded`), since
 * this module has no session-state dependency of its own.
 */
export function makeProgramResolver(
  resolvePosition: (containerIdx: number, offset: number) => ProgramSourceLocation | null,
): LocationResolver {
  return (location) => {
    if (location.kind !== "program") return null;
    const parsed = parseProgramAddress(location.address);
    if (parsed === null) return null;
    const resolved = resolvePosition(parsed.containerIdx, parsed.offset);
    if (resolved === null || resolved.file === null) return null;
    return {
      kind: "source",
      file: resolved.file,
      span: { start: resolved.range_start, end: resolved.range_start + resolved.range_len },
    };
  };
}

/**
 * Build the `session` resolver's program-position half: a session ref that
 * carries a runtime `(containerIdx, offset)` position — a `DebugFrame` /
 * `DebugState`'s own `position` field (`@brink-lang/web`'s `DebugFrame`/
 * `DebugState`) — resolves to the `program` Location the `program` resolver
 * above then continues toward source. `docs/studio-shell-spec.md` §6.1:
 * "session → program (runtime state)".
 *
 * Anything else (no `position`, or a ref that isn't position-shaped) is
 * `null` — the chain simply doesn't continue, not an error; a session ref
 * with `location: "knot.stitch"` but no precise position should resolve
 * through the `symbol` space instead ({@link resolveQualifiedSymbol}), which
 * a caller is free to try as a fallback.
 */
export function resolveSessionPositionRef(location: Location): Location | null {
  if (location.kind !== "session") return null;
  const ref = location.ref;
  if (typeof ref !== "object" || ref === null) return null;
  const position = (ref as { position?: unknown }).position;
  if (typeof position !== "object" || position === null) return null;
  const { container_idx, offset } = position as { container_idx?: unknown; offset?: unknown };
  if (typeof container_idx !== "number" || typeof offset !== "number") return null;
  return { kind: "program", address: encodeProgramAddress(container_idx, offset) };
}

// ── view.reveal receivers ────────────────────────────────────────────

/**
 * Receiver registry for the generic `view.reveal(viewId, item)` command —
 * "Reveal in Binder", "Reveal in Graph". Each view registers its receiver
 * when it lands (Binder #82+, Story Graph #97).
 */
export class ViewRevealHandlers {
  private readonly handlers = new Map<string, (item: unknown) => void>();

  register(viewId: string, handler: (item: unknown) => void): () => void {
    if (this.handlers.has(viewId)) {
      throw new Error(`duplicate view.reveal handler for "${viewId}"`);
    }
    this.handlers.set(viewId, handler);
    return () => {
      this.handlers.delete(viewId);
    };
  }

  /** Returns false when no receiver is registered for the view. */
  reveal(viewId: string, item: unknown): boolean {
    const handler = this.handlers.get(viewId);
    if (handler === undefined) return false;
    handler(item);
    return true;
  }
}
