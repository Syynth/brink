/**
 * StoryGraphDocument — the "story-graph" document type (issue #97, spec §4.1,
 * §7.8).
 *
 * A custom-rendered (non-CM6) editor document: a pan/zoom react-flow graph of
 * the story's structure from the wasm story-graph query (#96). Knots-first —
 * one node per knot, collapsed by default; expanding a knot (chevron or
 * double-click) reveals its stitches as a nested subflow, re-laid-out off the
 * render path (dagre, story-graph-layout.ts). Edges style by kind; END/DONE
 * are small terminal pseudo-nodes. Clicking a node dispatches `editor.reveal`
 * with its source location (§6.1). Read-only — no authoring from the graph.
 *
 * Compile-bound + session-overlaid: the graph itself renders `storyGraph`
 * from the compile slice (refreshed on each successful compile; a failed
 * compile keeps the last good graph, like Compiled Output keeps the last
 * program). While a session runs, the overlay highlights the current
 * location's node (falling back to the knot when the location sits inside a
 * collapsed one) and badges visit counts — consuming session DATA from the
 * store only (`debugState`), never the runner handle (§7.6,
 * provider-agnostic).
 */

import { memo, useCallback, useMemo, useState } from "react";
// react-flow's structural stylesheet (positioning, panes, handles). All
// *colors* are re-skinned with --bs-* tokens in styles/story-graph.css.
import "@xyflow/react/dist/style.css";
import {
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  Panel,
  Position,
  ReactFlow,
  type Edge,
  type Node,
  type NodeProps,
  type NodeTypes,
} from "@xyflow/react";
import type { StoryGraph } from "@brink/wasm-types";
import {
  EDITOR_REVEAL_COMMAND_ID,
  useShell,
  type CommandRegistry,
  type DocumentRef,
  type DocumentViewProps,
  type EditorGroupsStore,
  type Location as ShellLocation,
} from "@brink/studio-shell";
import { sessionDegraded } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import {
  buildGraphView,
  buildOverlay,
  currentNodeId,
  nodeVisitCount,
  type DebugStateLike,
  type GraphView,
  type GraphViewEdge,
  type GraphViewNode,
  type SessionOverlay,
} from "./story-graph-model.js";
import { layoutGraphView, type GraphLayout } from "./story-graph-layout.js";

// ── Document type (spec §7.8) ───────────────────────────────────────

export const STORY_GRAPH_TYPE_ID = "story-graph";
export const STORY_GRAPH_DOC_ID = "story-graph";
export const OPEN_STORY_GRAPH_COMMAND_ID = "story.openGraph";

/** The singleton DocumentRef — one stable identity, one tab. */
export function storyGraphRef(): DocumentRef {
  return {
    typeId: STORY_GRAPH_TYPE_ID,
    docId: STORY_GRAPH_DOC_ID,
    title: "Story Graph",
  };
}

/**
 * Register `story.openGraph` (palette/hamburger: "Story: Open Story Graph",
 * no default keybinding). Opens pinned into the focused group; the groups
 * store's reveal policy focuses an existing tab wherever it lives.
 */
export function registerStoryGraphCommand(
  commands: CommandRegistry,
  editorGroups: EditorGroupsStore,
): () => void {
  return commands.register({
    id: OPEN_STORY_GRAPH_COMMAND_ID,
    title: "Story: Open Story Graph",
    run: () =>
      editorGroups.getState().openDocument(storyGraphRef(), { pinned: true }),
  });
}

// ── Model hook (layout off the render path) ─────────────────────────

export interface StoryGraphModel {
  view: GraphView;
  /** Memoized on structure (graph + expansion) only — overlay changes never re-layout. */
  graphLayout: GraphLayout;
  overlay: SessionOverlay | null;
  /** The visible node to highlight as the session's current location. */
  currentId: string | null;
}

/**
 * Derive the render model. Layout is memoized on the structural inputs
 * (graph identity + expansion set); the session overlay merges in
 * downstream, so a story advancing restyles nodes without re-running dagre.
 * Exported for the off-render-path smoke test.
 */
export function useStoryGraphModel(
  graph: StoryGraph,
  expanded: ReadonlySet<string>,
  debugState: DebugStateLike | null,
): StoryGraphModel {
  const view = useMemo(() => buildGraphView(graph, expanded), [graph, expanded]);
  const graphLayout = useMemo(() => layoutGraphView(view), [view]);
  const overlay = useMemo(() => buildOverlay(debugState), [debugState]);
  const currentId = useMemo(
    () => currentNodeId(overlay?.currentLocation ?? null, view),
    [overlay, view],
  );
  return useMemo(
    () => ({ view, graphLayout, overlay, currentId }),
    [view, graphLayout, overlay, currentId],
  );
}

// ── react-flow translation (pure, exported for tests) ──────────────

export interface StoryNodeData extends Record<string, unknown> {
  node: GraphViewNode;
  current: boolean;
  visits: number | null;
  onToggle: (id: string) => void;
}

export type StoryFlowNode = Node<StoryNodeData, "story">;

/**
 * View nodes → react-flow nodes. The view's deterministic id order already
 * puts every parent knot before its stitches (prefix order), which is the
 * order react-flow requires for subflows.
 */
export function toFlowNodes(
  model: StoryGraphModel,
  onToggle: (id: string) => void,
): StoryFlowNode[] {
  const { view, graphLayout, overlay, currentId } = model;
  return view.nodes.map((node) => {
    const pos = graphLayout.get(node.id);
    return {
      id: node.id,
      type: "story" as const,
      position: { x: pos?.x ?? 0, y: pos?.y ?? 0 },
      data: {
        node,
        current: node.id === currentId,
        visits: nodeVisitCount(node.id, overlay),
        onToggle,
      },
      // Sizes come from the layout so edges anchor where dagre planned;
      // expanded knots take their computed cluster size.
      style: { width: pos?.width, height: pos?.height },
      draggable: false,
      connectable: false,
      selectable: false,
      ...(node.parent !== undefined
        ? { parentId: node.parent, extent: "parent" as const }
        : {}),
    };
  });
}

/** View edges → react-flow edges, styled per kind via CSS classes. */
export function toFlowEdges(edges: readonly GraphViewEdge[]): Edge[] {
  return edges.map((edge) => ({
    id: edge.id,
    source: edge.from,
    target: edge.to,
    type: "smoothstep",
    className: `brink-graph-edge brink-graph-edge-${edge.kind}`,
    markerEnd: `url(#brink-arrow-${edge.kind})`,
    ...(edge.count > 1 ? { label: `×${edge.count}` } : {}),
    focusable: false,
  }));
}

// ── Custom node ─────────────────────────────────────────────────────

function StoryNodeInner({ data }: NodeProps<StoryFlowNode>) {
  const { node, current, visits, onToggle } = data;
  const kindClass = `brink-graph-node brink-graph-node-${node.kind}`;
  const stateClass =
    (node.expanded ? " expanded" : "") + (current ? " current" : "");

  return (
    <div
      className={kindClass + stateClass}
      data-graph-node={node.id}
      data-kind={node.kind}
      data-expanded={node.expandable ? node.expanded : undefined}
      data-current={current ? "true" : undefined}
      data-visits={visits ?? undefined}
    >
      <Handle type="target" position={Position.Top} isConnectable={false} />
      <div className="brink-graph-node-header">
        {node.expandable && (
          <button
            type="button"
            className="brink-graph-node-toggle"
            title={node.expanded ? "Collapse stitches" : "Expand stitches"}
            aria-label={node.expanded ? "Collapse stitches" : "Expand stitches"}
            aria-expanded={node.expanded}
            onClick={(e) => {
              e.stopPropagation();
              onToggle(node.id);
            }}
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
              <path
                d={node.expanded ? "M2 6.5l3-3 3 3" : "M3.5 2l3 3-3 3"}
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
        )}
        <span className="brink-graph-node-label">{node.label}</span>
        {visits !== null && (
          <span className="brink-graph-node-visits" title={`${visits} visits`}>
            {visits}
          </span>
        )}
      </div>
      <Handle type="source" position={Position.Bottom} isConnectable={false} />
    </div>
  );
}

const StoryNode = memo(StoryNodeInner);
const NODE_TYPES: NodeTypes = { story: StoryNode };

// ── Edge arrowheads ─────────────────────────────────────────────────
//
// react-flow's built-in markers take concrete colors; CSS-variable theming
// needs our own <defs>, referenced by `markerEnd: url(#…)` and filled via
// the --bs-* tokens in story-graph.css.

const EDGE_KINDS = ["divert", "choice", "tunnel", "thread"] as const;

function EdgeMarkerDefs() {
  return (
    <svg className="brink-graph-markers" aria-hidden width="0" height="0">
      <defs>
        {EDGE_KINDS.map((kind) => (
          <marker
            key={kind}
            id={`brink-arrow-${kind}`}
            viewBox="0 0 10 10"
            refX="8.5"
            refY="5"
            markerWidth="6.5"
            markerHeight="6.5"
            orient="auto-start-reverse"
          >
            <path
              d="M0 0.5 L9.5 5 L0 9.5 z"
              className={`brink-graph-arrow brink-graph-arrow-${kind}`}
            />
          </marker>
        ))}
      </defs>
    </svg>
  );
}

// ── Legend ──────────────────────────────────────────────────────────

function Legend() {
  return (
    <div className="brink-graph-legend">
      <div className="brink-graph-legend-row">
        <span className="brink-graph-legend-swatch swatch-divert" />
        divert
      </div>
      <div className="brink-graph-legend-row">
        <span className="brink-graph-legend-swatch swatch-choice" />
        choice
      </div>
      <div className="brink-graph-legend-row">
        <span className="brink-graph-legend-swatch swatch-tunnel" />
        tunnel
      </div>
      <div className="brink-graph-legend-row">
        <span className="brink-graph-legend-swatch swatch-thread" />
        thread
      </div>
    </div>
  );
}

// ── Canvas ──────────────────────────────────────────────────────────

function StoryGraphCanvas({ graph }: { graph: StoryGraph }) {
  const { commands } = useShell();
  // Session overlay inputs — DATA from the session slice only (the debug
  // snapshot is already name-resolved); no runner handle anywhere (§7.6).
  const debugState = useStudioStore((s) => s.debugState);
  // Degraded mode (spec §5, #181): when the running program isn't the studio's
  // latest compile, the snapshot's locations/visits are keyed to a *different*
  // program, so drop the source-position overlay (current-location highlight +
  // visit badges) by withholding the debug state. The structural graph stays.
  const degraded = useStudioStore((s) =>
    sessionDegraded(s.programChecksum, s.compiledChecksum),
  );
  const overlaySource = degraded ? null : debugState;

  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const onToggle = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const model = useStoryGraphModel(graph, expanded, overlaySource);

  const nodes = useMemo(() => toFlowNodes(model, onToggle), [model, onToggle]);
  const edges = useMemo(() => toFlowEdges(model.view.edges), [model.view.edges]);

  const revealNode = useCallback(
    (node: StoryFlowNode) => {
      const view = node.data.node;
      if (view.file === undefined || view.span === undefined) return;
      const location: ShellLocation = {
        kind: "source",
        file: view.file,
        span: view.span,
      };
      commands.dispatch(EDITOR_REVEAL_COMMAND_ID, location);
    },
    [commands],
  );

  // Right-click a knot/stitch node → the shared symbol context menu (play from
  // here + the structural refactors), rendered by SymbolContextMenuHost. The
  // node id is the qualified ink path; `node.file` locates its declaration.
  const openSymbolMenu = useStudioStore((s) => s.openSymbolMenu);
  const onNodeContextMenu = useCallback(
    (e: React.MouseEvent, n: StoryFlowNode) => {
      const node = n.data.node;
      if ((node.kind !== "knot" && node.kind !== "stitch") || node.file === undefined) return;
      e.preventDefault();
      const dot = node.id.indexOf(".");
      openSymbolMenu({
        path: node.file,
        knot: dot >= 0 ? node.id.slice(0, dot) : node.id,
        stitch: dot >= 0 ? node.id.slice(dot + 1) : undefined,
        x: e.clientX,
        y: e.clientY,
      });
    },
    [openSymbolMenu],
  );

  return (
    <div className="brink-story-graph" data-graph-ready="true">
      <EdgeMarkerDefs />
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={NODE_TYPES}
        fitView
        fitViewOptions={{ padding: 0.15, maxZoom: 1 }}
        minZoom={0.05}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        edgesFocusable={false}
        zoomOnDoubleClick={false}
        proOptions={{ hideAttribution: true }}
        onNodeClick={(_e, node) => revealNode(node as StoryFlowNode)}
        onNodeContextMenu={(e, node) => onNodeContextMenu(e, node as StoryFlowNode)}
        onNodeDoubleClick={(_e, node) => {
          const data = (node as StoryFlowNode).data;
          if (data.node.expandable) onToggle(data.node.id);
        }}
      >
        <Background variant={BackgroundVariant.Dots} gap={24} size={1.25} />
        <Controls showInteractive={false} position="bottom-left" />
        <Panel position="bottom-right">
          <Legend />
        </Panel>
      </ReactFlow>
    </div>
  );
}

// ── Document component ──────────────────────────────────────────────

export function StoryGraphDocument(_props: DocumentViewProps) {
  const graph = useStudioStore((s) => s.storyGraph);

  if (graph === null || graph.nodes.length === 0) {
    // Compile-bound placeholder: no successful compile has produced a graph
    // yet (or the project has no knots to chart).
    return (
      <div className="brink-story-graph-empty">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No story graph yet</p>
          <p className="state-view-empty-hint">
            The story&apos;s knot/stitch structure appears here after the
            first successful compile.
          </p>
        </div>
      </div>
    );
  }

  return <StoryGraphCanvas graph={graph} />;
}
