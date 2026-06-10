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
