/**
 * Story Graph view model (issue #97, spec §4.1) — pure functions.
 *
 * Translates the wasm `StoryGraph` (whole-project knot/stitch/pseudo nodes +
 * divert/choice/tunnel/thread edges, #96) into the *view* graph the document
 * renders: knots-first with stitches revealed per-knot by an expansion set,
 * edges into a collapsed knot's stitches aggregated up to the knot
 * (cap-by-collapse — the unbounded-growth guard), and the live-session
 * overlay (current location + visit counts) merged in as plain data.
 *
 * Everything here is renderer-agnostic and side-effect free so collapse
 * aggregation, overlay mapping, and determinism are unit-testable without
 * mounting react-flow. The react-flow translation lives in
 * StoryGraphDocument.tsx; the dagre layout in story-graph-layout.ts.
 */

import type {
  StoryGraph,
  StoryGraphEdgeKind,
  StoryGraphNode,
  StoryGraphNodeKind,
} from "@brink/wasm-types";

// ── View types ──────────────────────────────────────────────────────

export interface GraphViewNode {
  /** The story-graph node id (qualified name, or END/DONE). */
  id: string;
  /** Display label: the last path segment for stitches, the id otherwise. */
  label: string;
  kind: StoryGraphNodeKind;
  /** The owning knot's id — set on visible stitches (subflow nesting). */
  parent?: string;
  /** Knot with stitches: shows the expand/collapse affordance. */
  expandable: boolean;
  /** Expandable knot currently showing its stitches. */
  expanded: boolean;
  /** Source location for `editor.reveal`; absent on pseudo-nodes. */
  file?: string;
  span?: { start: number; end: number };
}

export interface GraphViewEdge {
  /** Stable id: `from→to:kind`. */
  id: string;
  from: string;
  to: string;
  kind: StoryGraphEdgeKind;
  /**
   * Number of source edges folded into this one — > 1 when collapsing a
   * knot aggregated several stitch-level edges onto the same endpoints.
   */
  count: number;
}

export interface GraphView {
  nodes: GraphViewNode[];
  edges: GraphViewEdge[];
}

// ── Session overlay (spec §7.6 — plain session DATA, never the runner) ──

/** Structural subset of wasm-types' DebugState the overlay consumes. */
export interface DebugStateLike {
  current_location?: string;
  visit_counts: readonly { path: string; count: number }[];
}

export interface SessionOverlay {
  /** The debug snapshot's current location (a dot path), if any. */
  currentLocation: string | null;
  /** Visit counts keyed by dot path. */
  visits: ReadonlyMap<string, number>;
}

/**
 * Distill the session's debug snapshot into the overlay the graph consumes.
 * `null` in → `null` out: with no session there is no overlay (the graph
 * renders plain), and nothing here ever touches a runner handle.
 */
export function buildOverlay(debugState: DebugStateLike | null): SessionOverlay | null {
  if (debugState === null) return null;
  const visits = new Map<string, number>();
  for (const v of debugState.visit_counts) {
    visits.set(v.path, v.count);
  }
  return { currentLocation: debugState.current_location ?? null, visits };
}

// ── Collapse mapping ────────────────────────────────────────────────

function shortLabel(node: StoryGraphNode): string {
  if (node.kind !== "stitch") return node.name;
  const dot = node.name.lastIndexOf(".");
  return dot >= 0 ? node.name.slice(dot + 1) : node.name;
}

/**
 * Build the view graph for one expansion state.
 *
 * - Nodes: knots and END/DONE always; stitches only when their parent knot
 *   is in `expanded` (then they carry `parent` for nesting).
 * - Edges: endpoints inside a collapsed knot are remapped up to the knot;
 *   after remapping, duplicates fold into one edge with a `count`, and
 *   self-edges *created by the remapping* (intra-knot stitch hops) are
 *   dropped — a genuine `knot -> knot` self-divert is kept.
 * - Ordering is inherited from the source graph (already deterministic) and
 *   edge folding preserves first-appearance order.
 */
export function buildGraphView(
  graph: StoryGraph,
  expanded: ReadonlySet<string>,
): GraphView {
  const byId = new Map<string, StoryGraphNode>();
  const knotsWithStitches = new Set<string>();
  for (const node of graph.nodes) {
    byId.set(node.id, node);
    if (node.kind === "stitch" && node.parent !== undefined) {
      knotsWithStitches.add(node.parent);
    }
  }

  const nodes: GraphViewNode[] = [];
  for (const node of graph.nodes) {
    const isStitch = node.kind === "stitch" && node.parent !== undefined;
    if (isStitch && !expanded.has(node.parent as string)) continue;
    const expandable = node.kind === "knot" && knotsWithStitches.has(node.id);
    nodes.push({
      id: node.id,
      label: shortLabel(node),
      kind: node.kind,
      ...(isStitch ? { parent: node.parent } : {}),
      expandable,
      expanded: expandable && expanded.has(node.id),
      ...(node.file !== undefined ? { file: node.file } : {}),
      ...(node.start !== undefined && node.end !== undefined
        ? { span: { start: node.start, end: node.end } }
        : {}),
    });
  }

  /** Remap an endpoint inside a collapsed knot up to the knot itself. */
  const mapEndpoint = (id: string): string => {
    const node = byId.get(id);
    if (
      node !== undefined &&
      node.kind === "stitch" &&
      node.parent !== undefined &&
      !expanded.has(node.parent)
    ) {
      return node.parent;
    }
    return id;
  };

  const folded = new Map<string, GraphViewEdge>();
  for (const edge of graph.edges) {
    const from = mapEndpoint(edge.from);
    const to = mapEndpoint(edge.to);
    // Aggregation self-loop: both endpoints folded into one collapsed knot.
    // A genuine self-divert (from === to in the source graph) is kept.
    if (from === to && edge.from !== edge.to) continue;
    const id = `${from}→${to}:${edge.kind}`;
    const existing = folded.get(id);
    if (existing !== undefined) {
      existing.count += 1;
    } else {
      folded.set(id, { id, from, to, kind: edge.kind, count: 1 });
    }
  }

  return { nodes, edges: [...folded.values()] };
}

// ── Overlay mapping ─────────────────────────────────────────────────

/**
 * Map the session's current location (a dot path, possibly deeper than any
 * graph node — e.g. weave sub-containers) to the *visible* node that should
 * be highlighted: the longest dot-prefix of the path that names a visible
 * node. A stitch inside a collapsed knot falls back to the knot.
 */
export function currentNodeId(
  location: string | null,
  view: GraphView,
): string | null {
  if (location === null) return null;
  const visible = new Set(view.nodes.map((n) => n.id));
  const parts = location.split(".");
  for (let len = parts.length; len >= 1; len--) {
    const prefix = parts.slice(0, len).join(".");
    if (visible.has(prefix)) return prefix;
  }
  return null;
}

/**
 * The visit count to badge a visible node with: its own recorded count
 * (knot and stitch counts are tracked separately by the runtime; a collapsed
 * knot shows the knot's own count). `null` when the overlay has none.
 */
export function nodeVisitCount(
  nodeId: string,
  overlay: SessionOverlay | null,
): number | null {
  if (overlay === null) return null;
  return overlay.visits.get(nodeId) ?? null;
}
