/**
 * Story Graph auto-layout (issue #97, spec §4.1) — dagre, off the render path.
 *
 * Layered top-down layout (the storyline convention: stories flow downward
 * from their entry knot). Pure function over the view graph so the document
 * memoizes it on *structure* only — overlay changes (current location, visit
 * counts) restyle nodes without re-running layout.
 *
 * Expanded knots are laid out in two passes (dagre's compound support is
 * unreliable): each expanded knot's stitches get their own small dagre run
 * to size the knot as a cluster, then the top level runs with cluster-sized
 * knot nodes. Child positions are relative to the parent (react-flow subflow
 * convention). Cross-cluster stitch edges rank via their parent knots.
 */

import { Graph, layout } from "@dagrejs/dagre";
import type { GraphView, GraphViewNode } from "./story-graph-model.js";

export interface NodeLayout {
  /** Top-left position — absolute for top-level nodes, parent-relative for stitches. */
  x: number;
  y: number;
  width: number;
  height: number;
}

export type GraphLayout = ReadonlyMap<string, NodeLayout>;

// Node dimensions — kept in sync with the CSS (story-graph.css). Width is
// estimated from the label (the node font is 12px; ~7.2px/char covers it).
const KNOT_HEIGHT = 40;
const STITCH_HEIGHT = 30;
const PSEUDO_HEIGHT = 26;
/** Vertical space reserved for an expanded knot's header row. */
const CLUSTER_HEADER = 36;
const CLUSTER_PAD_X = 14;
const CLUSTER_PAD_BOTTOM = 14;

function nodeSize(node: GraphViewNode): { width: number; height: number } {
  const chars = node.label.length;
  if (node.kind === "end" || node.kind === "done") {
    return { width: Math.max(56, chars * 8 + 24), height: PSEUDO_HEIGHT };
  }
  if (node.kind === "stitch") {
    return { width: Math.max(80, chars * 7.2 + 30), height: STITCH_HEIGHT };
  }
  // Knot: label + badge slack; expandable knots also fit the chevron.
  const slack = node.expandable ? 58 : 36;
  return { width: Math.max(110, chars * 7.2 + slack), height: KNOT_HEIGHT };
}

interface DagreOpts {
  nodesep: number;
  ranksep: number;
  marginx?: number;
  marginy?: number;
}

/** One dagre pass; node sizes in, top-left positions out (dagre yields centers). */
function runDagre(
  nodes: readonly { id: string; width: number; height: number }[],
  edges: readonly { from: string; to: string }[],
  opts: DagreOpts,
): Map<string, NodeLayout> {
  const g = new Graph();
  g.setGraph({ rankdir: "TB", ...opts });
  g.setDefaultEdgeLabel(() => ({}));
  for (const node of nodes) {
    g.setNode(node.id, { width: node.width, height: node.height });
  }
  for (const edge of edges) {
    // Self-loops confuse dagre's ranking; the edge still renders.
    if (edge.from !== edge.to) g.setEdge(edge.from, edge.to);
  }
  layout(g);

  const out = new Map<string, NodeLayout>();
  for (const node of nodes) {
    const pos = g.node(node.id);
    out.set(node.id, {
      x: pos.x - node.width / 2,
      y: pos.y - node.height / 2,
      width: node.width,
      height: node.height,
    });
  }
  return out;
}

/**
 * Lay out the view graph. Returns top-left positions and sizes for every
 * node: absolute for top-level nodes, relative to the parent knot for
 * stitches of expanded knots. Deterministic for a given view (insertion
 * order is the view's deterministic order).
 */
export function layoutGraphView(view: GraphView): GraphLayout {
  const result = new Map<string, NodeLayout>();

  // Group visible stitches under their (expanded) parent knots.
  const children = new Map<string, GraphViewNode[]>();
  for (const node of view.nodes) {
    if (node.parent === undefined) continue;
    const list = children.get(node.parent);
    if (list !== undefined) list.push(node);
    else children.set(node.parent, [node]);
  }

  // Pass 1 — each expanded knot's internal subgraph sizes the cluster.
  const clusterSize = new Map<string, { width: number; height: number }>();
  for (const [parent, kids] of children) {
    const ids = new Set(kids.map((k) => k.id));
    const internalEdges = view.edges.filter(
      (e) => ids.has(e.from) && ids.has(e.to),
    );
    const positions = runDagre(
      kids.map((k) => ({ id: k.id, ...nodeSize(k) })),
      internalEdges,
      { nodesep: 18, ranksep: 26 },
    );
    let maxX = 0;
    let maxY = 0;
    for (const [id, pos] of positions) {
      result.set(id, {
        ...pos,
        x: pos.x + CLUSTER_PAD_X,
        y: pos.y + CLUSTER_HEADER,
      });
      maxX = Math.max(maxX, pos.x + pos.width);
      maxY = Math.max(maxY, pos.y + pos.height);
    }
    const headerWidth = nodeSize(
      view.nodes.find((n) => n.id === parent) ?? {
        id: parent,
        label: parent,
        kind: "knot",
        expandable: true,
        expanded: true,
      },
    ).width;
    clusterSize.set(parent, {
      width: Math.max(maxX + CLUSTER_PAD_X * 2, headerWidth),
      height: maxY + CLUSTER_HEADER + CLUSTER_PAD_BOTTOM,
    });
  }

  // Pass 2 — top level, with expanded knots at cluster size and edges
  // remapped onto top-level nodes for ranking.
  const topNodes = view.nodes.filter((n) => n.parent === undefined);
  const parentOf = new Map<string, string>();
  for (const [parent, kids] of children) {
    for (const kid of kids) parentOf.set(kid.id, parent);
  }
  const topEdges: { from: string; to: string }[] = [];
  for (const edge of view.edges) {
    const from = parentOf.get(edge.from) ?? edge.from;
    const to = parentOf.get(edge.to) ?? edge.to;
    if (from === to) continue; // internal to one cluster — already laid out
    topEdges.push({ from, to });
  }
  const topPositions = runDagre(
    topNodes.map((n) => ({ id: n.id, ...(clusterSize.get(n.id) ?? nodeSize(n)) })),
    topEdges,
    { nodesep: 36, ranksep: 56, marginx: 24, marginy: 24 },
  );
  for (const [id, pos] of topPositions) {
    result.set(id, pos);
  }

  return result;
}
