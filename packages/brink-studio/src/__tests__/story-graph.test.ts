/**
 * Story-graph wrapper test (#96): `getStoryGraph()` on EditorSessionHandle
 * parses the wasm `story_graph()` JSON into the typed StoryGraph shape.
 *
 * Runs against the brink-web mock (src/__mocks__/brink-web.ts), which derives
 * nodes from the same header parse as the outline. The real extraction
 * (edges, weave aggregation, pseudo-nodes, determinism) is covered by Rust
 * tests in brink-ide and brink-web.
 */

import { describe, it, expect } from "vitest";
import { EditorSessionHandle } from "@brink-lang/web";

const MAIN_INK = ["=== start ===", "Hello.", "= gate", "Onward.", "=== hub ===", "Hub."].join(
  "\n",
);

describe("getStoryGraph", () => {
  it("parses the story graph from the session", () => {
    const session = new EditorSessionHandle();
    session.updateFile("main.ink", MAIN_INK);

    const graph = session.getStoryGraph();
    expect(graph).not.toBeNull();
    expect(graph!.edges).toEqual([]);

    // Nodes sorted by id; stitches carry their parent knot id.
    expect(graph!.nodes.map((n) => n.id)).toEqual(["hub", "start", "start.gate"]);
    const gate = graph!.nodes.find((n) => n.id === "start.gate")!;
    expect(gate.kind).toBe("stitch");
    expect(gate.parent).toBe("start");
    expect(gate.file).toBe("main.ink");

    const hub = graph!.nodes.find((n) => n.id === "hub")!;
    expect(hub.kind).toBe("knot");
    expect(hub.parent).toBeUndefined();
  });
});
