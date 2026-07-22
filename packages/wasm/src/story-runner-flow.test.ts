import { describe, it, expect, vi } from "vitest";

// FS-3w (issue #978, `docs/flow-suspension-spec.md` §10.1/§11.4): the
// flow-addressed web surface. This pins the wrapper-layer wiring, with
// `brink-web` (the wasm-pack output) replaced by a call-recording stub —
// real runtime behavior stays covered by `crates/brink-runtime` and the
// Rust-side `wasm_bindgen_test` modules in `crates/brink-web`.
//
// Two invariants matter here:
//   1. Story-level drive methods (`continueStory`/`continueSingle`) are
//      unchanged documented sugar for the primary flow — a zero-breakage
//      regression guard that existing consumers still forward exactly as
//      before.
//   2. The new flow-addressed surface (`flow()`/`spawnFlow()` handles,
//      per-flow `Line` streams, and `wakeCheck()` returning an empty woken
//      list) forwards to the raw session methods with the right arguments.
//
// #999: `FlowHandle.continueMaximally` forwards to the raw
// `continue_flow_maximally` wasm binding in ONE call rather than looping the
// single-line `continue_flow` client-side without a cap — the actual
// line-limit enforcement (and its infinite-emitting-story proof) lives in
// `crates/brink-runtime` (`continue_flow_maximally_shared_errors_at_line_limit`)
// and `crates/brink-web` (`continue_flow_maximally_errors_at_line_limit_on_infinite_emitting_flow`);
// this file just pins that the wrapper doesn't reintroduce an unbounded loop.

const hoisted = vi.hoisted(() => {
  const calls: Array<{ method: string; args: unknown[] }> = [];
  class StoryRunnerStub {
    constructor(bytes: unknown) {
      calls.push({ method: "new", args: [bytes] });
    }
    continue_story(): string {
      calls.push({ method: "continue_story", args: [] });
      return JSON.stringify([
        { type: "text", text: "hi", tags: [] },
        { type: "done", text: "", tags: [] },
      ]);
    }
    continue_single(): string {
      calls.push({ method: "continue_single", args: [] });
      return JSON.stringify({ type: "text", text: "hi", tags: [] });
    }
    spawn_flow(name: unknown, path: unknown): void {
      calls.push({ method: "spawn_flow", args: [name, path] });
    }
    continue_flow(name: unknown): string {
      calls.push({ method: "continue_flow", args: [name] });
      return JSON.stringify({ type: "text", text: "npc", tags: [] });
    }
    continue_flow_maximally(name: unknown): string {
      calls.push({ method: "continue_flow_maximally", args: [name] });
      if (name === "boom") {
        // Mirrors the wasm leg's `RuntimeError::LineLimitExceeded` shape
        // (`JsError::new(&format!("runtime error: {e}"))`) for an
        // infinite-emitting flow (#999).
        throw new Error("runtime error: line limit exceeded (10000 lines in a single turn)");
      }
      return JSON.stringify([
        { type: "text", text: "npc", tags: [] },
        { type: "done", text: "", tags: [] },
      ]);
    }
    choose_flow(name: unknown, index: unknown): void {
      calls.push({ method: "choose_flow", args: [name, index] });
    }
    destroy_flow(name: unknown): void {
      calls.push({ method: "destroy_flow", args: [name] });
    }
    flow_names(): string {
      calls.push({ method: "flow_names", args: [] });
      return JSON.stringify(["npc"]);
    }
    flow_debug_snapshot(name: unknown): string {
      calls.push({ method: "flow_debug_snapshot", args: [name] });
      return JSON.stringify({ status: "active" });
    }
    wake_check(): string {
      calls.push({ method: "wake_check", args: [] });
      return JSON.stringify([]);
    }
  }
  return { calls, StoryRunnerStub };
});

vi.mock("brink-web", () => ({
  default: async () => undefined,
  compile: () => "",
  compile_fragment: () => "",
  program_checksum: () => "",
  token_type_names: () => "[]",
  token_modifier_names: () => "[]",
  EditorSession: class {},
  StoryRunner: hoisted.StoryRunnerStub,
  WebSpeculation: class {},
  WebSession: class {},
  diffSnapshots: () => "",
}));

import { StoryRunnerHandle, FlowHandle } from "./index";

function newRunner(): StoryRunnerHandle {
  hoisted.calls.length = 0;
  return new StoryRunnerHandle(new Uint8Array());
}

describe("story-level drive is unchanged sugar for the primary flow (FS-3w §10.1)", () => {
  it("continueSingle forwards to the raw continue_single, unchanged", () => {
    const runner = newRunner();
    const line = runner.continueSingle();
    expect(line).toEqual({ type: "text", text: "hi", tags: [] });
    expect(hoisted.calls.map((c) => c.method)).toContain("continue_single");
  });

  it("continueStory forwards to the raw continue_story, unchanged", () => {
    const runner = newRunner();
    const lines = runner.continueStory();
    expect(lines).toHaveLength(2);
    expect(lines[1]?.type).toBe("done");
    expect(hoisted.calls.map((c) => c.method)).toContain("continue_story");
  });
});

describe("flow-addressed consumption (FS-3w §10.1)", () => {
  it("spawnFlow returns a FlowHandle and forwards spawn_flow(name, path)", () => {
    const runner = newRunner();
    const handle = runner.spawnFlow("npc", "start");
    expect(handle).toBeInstanceOf(FlowHandle);
    expect(handle.name).toBe("npc");
    expect(hoisted.calls).toContainEqual({
      method: "spawn_flow",
      args: ["npc", "start"],
    });
  });

  it("flow(name) yields a handle whose continue() drives that flow's stream", () => {
    const runner = newRunner();
    const handle = runner.flow("npc");
    const line = handle.continue();
    expect(line).toEqual({ type: "text", text: "npc", tags: [] });
    expect(hoisted.calls).toContainEqual({
      method: "continue_flow",
      args: ["npc"],
    });
  });

  it("FlowHandle.continueMaximally forwards to the raw continue_flow_maximally in one call (#999)", () => {
    const runner = newRunner();
    const lines = runner.flow("npc").continueMaximally();
    expect(lines.map((l) => l.type)).toEqual(["text", "done"]);
    // Exactly one call — proves the cap lives wasm-side (a single
    // round-trip), not a client-side loop over `continue_flow`.
    expect(hoisted.calls.filter((c) => c.method === "continue_flow_maximally")).toHaveLength(1);
    expect(hoisted.calls.filter((c) => c.method === "continue_flow")).toHaveLength(0);
  });

  it("FlowHandle.continueMaximally propagates the wasm leg's line-limit error unchanged (#999)", () => {
    const runner = newRunner();
    // An infinite-emitting flow's wasm leg throws rather than the wrapper
    // looping forever — this pins that the error surfaces to the caller
    // as-is (same shape `continueStory`'s cap error takes), not swallowed
    // or reshaped.
    expect(() => runner.flow("boom").continueMaximally()).toThrow(/line limit exceeded/);
  });

  it("FlowHandle.choose / debugSnapshot / destroy forward with the flow id", () => {
    const runner = newRunner();
    const handle = runner.flow("npc");
    handle.choose(2);
    handle.debugSnapshot();
    handle.destroy();
    expect(hoisted.calls).toContainEqual({
      method: "choose_flow",
      args: ["npc", 2],
    });
    expect(hoisted.calls).toContainEqual({
      method: "flow_debug_snapshot",
      args: ["npc"],
    });
    expect(hoisted.calls).toContainEqual({
      method: "destroy_flow",
      args: ["npc"],
    });
  });
});

describe("wakeCheck (FS-3w §10.2)", () => {
  it("forwards to the raw wake_check and returns an empty woken list until parks exist", () => {
    const runner = newRunner();
    expect(runner.wakeCheck()).toEqual([]);
    expect(hoisted.calls.map((c) => c.method)).toContain("wake_check");
  });
});
