/**
 * Story Graph document tests (issue #97, spec §4.1 / §7.8).
 *
 * Covers: command registration + dispatch opening/focusing the singleton tab
 * (never duplicating), the pure view-model mapping (knots-first collapse with
 * edge aggregation + counts, expansion revealing stitches as subflow
 * children), the session overlay mapping (current-location longest-prefix
 * fallback to the knot, visit-count badges — plain session DATA, spec §7.6),
 * the dagre layout (positions for every node, parent-relative children,
 * deterministic), the react-flow translation, and the component's
 * compile-bound placeholder state.
 */

import { describe, expect, it, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { StoryGraph } from "@brink/wasm-types";
import {
  CommandRegistry,
  createEditorGroupsStore,
  documentKey,
  findTab,
} from "@brink/studio-shell";
import { createStudioStore, sessionDegraded } from "@brink/studio-store";
import {
  OPEN_STORY_GRAPH_COMMAND_ID,
  STORY_GRAPH_TYPE_ID,
  StoreProvider,
  StoryGraphDocument,
  buildGraphView,
  buildOverlay,
  currentNodeId,
  layoutGraphView,
  nodeVisitCount,
  registerStoryGraphCommand,
  storyGraphRef,
  toFlowEdges,
  toFlowNodes,
  type StoryGraphModel,
} from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

// A small fixture exercising every mapping rule: a knot with stitches (hub),
// plain knots (intro, loop with a genuine self-divert), END/DONE pseudo-nodes,
// and edges of several kinds, some landing on stitches.
const GRAPH: StoryGraph = {
  nodes: [
    { id: "DONE", name: "DONE", kind: "done" },
    { id: "END", name: "END", kind: "end" },
    { id: "hub", name: "hub", kind: "knot", file: "main.ink", start: 40, end: 43 },
    {
      id: "hub.east",
      name: "hub.east",
      kind: "stitch",
      parent: "hub",
      file: "main.ink",
      start: 60,
      end: 64,
    },
    {
      id: "hub.west",
      name: "hub.west",
      kind: "stitch",
      parent: "hub",
      file: "main.ink",
      start: 90,
      end: 94,
    },
    { id: "intro", name: "intro", kind: "knot", file: "main.ink", start: 4, end: 9 },
    { id: "loop", name: "loop", kind: "knot", file: "other.ink", start: 4, end: 8 },
  ],
  edges: [
    { from: "hub.east", to: "hub.west", kind: "divert" },
    { from: "hub.west", to: "END", kind: "divert" },
    { from: "hub", to: "loop", kind: "tunnel" },
    { from: "intro", to: "hub.east", kind: "choice" },
    { from: "intro", to: "hub.west", kind: "choice" },
    { from: "loop", to: "DONE", kind: "thread" },
    { from: "loop", to: "loop", kind: "divert" },
  ],
};

const NONE = new Set<string>();
const HUB = new Set(["hub"]);

/** Assemble the render model the way useStoryGraphModel does, hook-free. */
function makeModel(
  expanded: ReadonlySet<string>,
  debugState: Parameters<typeof buildOverlay>[0] = null,
): StoryGraphModel {
  const view = buildGraphView(GRAPH, expanded);
  const graphLayout = layoutGraphView(view);
  const overlay = buildOverlay(debugState);
  const currentId = currentNodeId(overlay?.currentLocation ?? null, view);
  return { view, graphLayout, overlay, currentId };
}

// ── Command wiring (singleton document, spec §7.8) ──────────────────

describe("story.openGraph", () => {
  it("opens the singleton pinned in the focused group", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerStoryGraphCommand(commands, groups);

    expect(commands.dispatch(OPEN_STORY_GRAPH_COMMAND_ID)).toBe(true);

    const key = documentKey(storyGraphRef());
    const found = findTab(groups.getState().groups, key);
    expect(found).not.toBeNull();
    expect(found!.tab.pinned).toBe(true);
    expect(found!.tab.ref.typeId).toBe(STORY_GRAPH_TYPE_ID);
    expect(found!.tab.ref.title).toBe("Story Graph");
    expect(found!.group.activeKey).toBe(key);
  });

  it("re-dispatch focuses the existing tab instead of duplicating", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerStoryGraphCommand(commands, groups);

    commands.dispatch(OPEN_STORY_GRAPH_COMMAND_ID);
    const homeGroupId = groups.getState().focusedGroupId;
    groups.getState().openDocument(
      { typeId: "ink-file", docId: "main.ink", title: "main.ink" },
      { group: "split-right" },
    );
    expect(groups.getState().focusedGroupId).not.toBe(homeGroupId);

    commands.dispatch(OPEN_STORY_GRAPH_COMMAND_ID);
    const s = groups.getState();
    expect(s.focusedGroupId).toBe(homeGroupId);

    const key = documentKey(storyGraphRef());
    const instances = s.groups.flatMap((g) =>
      g.tabs.filter((t) => documentKey(t.ref) === key),
    );
    expect(instances).toHaveLength(1);
  });

  it("reopens after the tab is closed", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerStoryGraphCommand(commands, groups);

    const key = documentKey(storyGraphRef());
    commands.dispatch(OPEN_STORY_GRAPH_COMMAND_ID);
    groups.getState().closeTab(groups.getState().focusedGroupId, key);
    expect(findTab(groups.getState().groups, key)).toBeNull();

    commands.dispatch(OPEN_STORY_GRAPH_COMMAND_ID);
    expect(findTab(groups.getState().groups, key)).not.toBeNull();
  });
});

// ── View model: collapse aggregation ────────────────────────────────

describe("buildGraphView", () => {
  it("hides stitches by default and marks their knot expandable", () => {
    const view = buildGraphView(GRAPH, NONE);

    expect(view.nodes.map((n) => n.id)).toEqual([
      "DONE",
      "END",
      "hub",
      "intro",
      "loop",
    ]);
    const hub = view.nodes.find((n) => n.id === "hub")!;
    expect(hub.expandable).toBe(true);
    expect(hub.expanded).toBe(false);
    // Knots without stitches are not expandable.
    expect(view.nodes.find((n) => n.id === "intro")!.expandable).toBe(false);
    // No node carries a parent while everything is collapsed.
    expect(view.nodes.every((n) => n.parent === undefined)).toBe(true);
  });

  it("aggregates stitch-level edges up to the collapsed knot, folding duplicates", () => {
    const view = buildGraphView(GRAPH, NONE);
    const byId = new Map(view.edges.map((e) => [e.id, e]));

    // intro's two choice edges into hub's stitches fold into one ×2 edge.
    const choice = byId.get("intro→hub:choice");
    expect(choice).toBeDefined();
    expect(choice!.count).toBe(2);

    // hub.west → END remaps to hub → END.
    expect(byId.get("hub→END:divert")!.count).toBe(1);

    // The intra-knot stitch hop (hub.east → hub.west) would self-loop after
    // remapping — dropped, not rendered as hub → hub.
    expect(byId.has("hub→hub:divert")).toBe(false);

    // A genuine self-divert in the source graph is kept.
    expect(byId.has("loop→loop:divert")).toBe(true);

    // Knot-level edges pass through untouched.
    expect(byId.has("hub→loop:tunnel")).toBe(true);
    expect(byId.has("loop→DONE:thread")).toBe(true);
  });

  it("expansion reveals stitches as children and un-aggregates their edges", () => {
    const view = buildGraphView(GRAPH, HUB);

    const east = view.nodes.find((n) => n.id === "hub.east")!;
    expect(east.parent).toBe("hub");
    expect(east.label).toBe("east"); // short label: last path segment
    expect(view.nodes.find((n) => n.id === "hub")!.expanded).toBe(true);

    const ids = view.edges.map((e) => e.id);
    expect(ids).toContain("intro→hub.east:choice");
    expect(ids).toContain("intro→hub.west:choice");
    expect(ids).toContain("hub.east→hub.west:divert");
    expect(ids).toContain("hub.west→END:divert");
    expect(ids).not.toContain("intro→hub:choice");
    expect(view.edges.every((e) => e.count === 1)).toBe(true);
  });

  it("carries source locations for reveal, absent on pseudo-nodes", () => {
    const view = buildGraphView(GRAPH, NONE);
    const intro = view.nodes.find((n) => n.id === "intro")!;
    expect(intro.file).toBe("main.ink");
    expect(intro.span).toEqual({ start: 4, end: 9 });
    const end = view.nodes.find((n) => n.id === "END")!;
    expect(end.file).toBeUndefined();
    expect(end.span).toBeUndefined();
  });
});

// ── Session overlay mapping (spec §7.6 — plain data) ────────────────

describe("session overlay", () => {
  const debugState = {
    current_location: "hub.east",
    visit_counts: [
      { path: "hub", count: 3 },
      { path: "hub.east", count: 2 },
      { path: "intro", count: 1 },
    ],
  };

  it("no session → no overlay (plain graph, zero errors)", () => {
    expect(buildOverlay(null)).toBeNull();
    expect(nodeVisitCount("hub", null)).toBeNull();
    expect(currentNodeId(null, buildGraphView(GRAPH, NONE))).toBeNull();
  });

  it("highlights the knot when the current location is a collapsed stitch", () => {
    const view = buildGraphView(GRAPH, NONE);
    const overlay = buildOverlay(debugState)!;
    expect(currentNodeId(overlay.currentLocation, view)).toBe("hub");
  });

  it("highlights the stitch itself once its knot is expanded", () => {
    const view = buildGraphView(GRAPH, HUB);
    const overlay = buildOverlay(debugState)!;
    expect(currentNodeId(overlay.currentLocation, view)).toBe("hub.east");
  });

  it("falls back through deeper paths (weave sub-containers) to the longest visible prefix", () => {
    const collapsed = buildGraphView(GRAPH, NONE);
    expect(currentNodeId("hub.east.opts.0", collapsed)).toBe("hub");
    const expanded = buildGraphView(GRAPH, HUB);
    expect(currentNodeId("hub.east.opts.0", expanded)).toBe("hub.east");
    // A location naming nothing visible highlights nothing.
    expect(currentNodeId("nowhere.at.all", collapsed)).toBeNull();
  });

  it("badges a node with its own visit count only", () => {
    const overlay = buildOverlay(debugState)!;
    expect(nodeVisitCount("hub", overlay)).toBe(3);
    expect(nodeVisitCount("hub.east", overlay)).toBe(2);
    expect(nodeVisitCount("loop", overlay)).toBeNull();
  });

  // Degraded mode (spec §5, #181): when the running program isn't the studio's
  // latest compile, the snapshot's locations/visits key to a *different*
  // program, so the canvas withholds the debug state — dropping both the
  // current-location highlight and the visit badges, while the structural
  // graph stays. This mirrors StoryGraphCanvas's `degraded ? null : debugState`.
  it("drops the overlay (highlight + badges) when the session is degraded", () => {
    const view = buildGraphView(GRAPH, NONE);
    const degraded = sessionDegraded("0xrunning0", "0xcompiled"); // differ
    expect(degraded).toBe(true);

    const overlaySource = degraded ? null : debugState;
    const overlay = buildOverlay(overlaySource);
    expect(overlay).toBeNull();
    expect(currentNodeId(overlay?.currentLocation ?? null, view)).toBeNull();
    expect(nodeVisitCount("hub", overlay)).toBeNull();
  });

  it("keeps the overlay when identity matches (full fidelity)", () => {
    const view = buildGraphView(GRAPH, NONE);
    const degraded = sessionDegraded("0xsame", "0xsame");
    expect(degraded).toBe(false);

    const overlay = buildOverlay(degraded ? null : debugState)!;
    expect(currentNodeId(overlay.currentLocation, view)).toBe("hub");
    expect(nodeVisitCount("hub", overlay)).toBe(3);
  });
});

// ── Layout (dagre, off the render path) ─────────────────────────────

describe("layoutGraphView", () => {
  it("positions every visible node with non-negative coordinates", () => {
    const view = buildGraphView(GRAPH, NONE);
    const layout = layoutGraphView(view);
    for (const node of view.nodes) {
      const pos = layout.get(node.id);
      expect(pos).toBeDefined();
      expect(pos!.x).toBeGreaterThanOrEqual(0);
      expect(pos!.y).toBeGreaterThanOrEqual(0);
      expect(pos!.width).toBeGreaterThan(0);
      expect(pos!.height).toBeGreaterThan(0);
    }
  });

  it("sizes an expanded knot as a cluster containing its parent-relative stitches", () => {
    const view = buildGraphView(GRAPH, HUB);
    const layout = layoutGraphView(view);
    const hub = layout.get("hub")!;
    for (const id of ["hub.east", "hub.west"]) {
      const kid = layout.get(id)!;
      // Parent-relative (react-flow subflow convention), inside the cluster.
      expect(kid.x).toBeGreaterThan(0);
      expect(kid.y).toBeGreaterThan(0);
      expect(kid.x + kid.width).toBeLessThanOrEqual(hub.width);
      expect(kid.y + kid.height).toBeLessThanOrEqual(hub.height);
    }
    // The cluster grew beyond the collapsed-knot footprint.
    const collapsed = layoutGraphView(buildGraphView(GRAPH, NONE)).get("hub")!;
    expect(hub.height).toBeGreaterThan(collapsed.height);
  });

  it("is deterministic for a given view", () => {
    const view = buildGraphView(GRAPH, HUB);
    expect(layoutGraphView(view)).toEqual(layoutGraphView(view));
  });
});

// ── react-flow translation ──────────────────────────────────────────

describe("flow translation", () => {
  it("maps nodes with subflow parenting, current flag, and visit badges", () => {
    const model = makeModel(HUB, {
      current_location: "hub.east",
      visit_counts: [{ path: "hub.east", count: 2 }],
    });
    const onToggle = () => {};
    const nodes = toFlowNodes(model, onToggle);

    // Parent knots precede their stitches (react-flow subflow order).
    const idx = (id: string) => nodes.findIndex((n) => n.id === id);
    expect(idx("hub")).toBeLessThan(idx("hub.east"));

    const east = nodes[idx("hub.east")]!;
    expect(east.parentId).toBe("hub");
    expect(east.extent).toBe("parent");
    expect(east.data.current).toBe(true);
    expect(east.data.visits).toBe(2);
    expect(east.data.onToggle).toBe(onToggle);

    const intro = nodes[idx("intro")]!;
    expect(intro.parentId).toBeUndefined();
    expect(intro.data.current).toBe(false);
    expect(intro.data.visits).toBeNull();
    // Read-only canvas: nothing draggable/connectable/selectable.
    expect(intro.draggable).toBe(false);
    expect(intro.connectable).toBe(false);
    expect(intro.selectable).toBe(false);
  });

  it("maps edges with per-kind classes, arrow markers, and ×N labels", () => {
    const view = buildGraphView(GRAPH, NONE);
    const edges = toFlowEdges(view.edges);
    const byId = new Map(edges.map((e) => [e.id, e]));

    const choice = byId.get("intro→hub:choice")!;
    expect(choice.className).toContain("brink-graph-edge-choice");
    expect(choice.markerEnd).toBe("url(#brink-arrow-choice)");
    expect(choice.label).toBe("×2");

    const tunnel = byId.get("hub→loop:tunnel")!;
    expect(tunnel.className).toContain("brink-graph-edge-tunnel");
    expect(tunnel.label).toBeUndefined();
  });
});

// ── Component placeholder (compile-bound) ───────────────────────────

describe("StoryGraphDocument component", () => {
  let root: Root | null = null;
  let container: HTMLDivElement | null = null;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;
  });

  function mount(store: ReturnType<typeof createStudioStore>) {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
      root!.render(
        createElement(StoreProvider, {
          store,
          children: createElement(StoryGraphDocument, {
            doc: storyGraphRef(),
            groupId: "group-1",
            active: true,
          }),
        }),
      );
    });
  }

  it("renders the placeholder before the first successful compile", () => {
    const store = createStudioStore();
    mount(store);
    expect(container!.querySelector(".brink-story-graph-empty")).not.toBeNull();
    expect(container!.textContent).toContain("No story graph yet");
  });

  it("keeps the placeholder for a graph with no knots to chart", () => {
    const store = createStudioStore();
    mount(store);
    act(() => store.setState({ storyGraph: { nodes: [], edges: [] } }));
    expect(container!.querySelector(".brink-story-graph-empty")).not.toBeNull();
  });
});
