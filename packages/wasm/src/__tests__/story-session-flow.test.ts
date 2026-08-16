import { describe, it, expect, vi } from "vitest";

// #1000 (PR #992/FS-3w review): `StorySessionHandle.spawnFlow` returned
// `void` while `StoryRunnerHandle.spawnFlow` returns a `FlowHandle` — the two
// session surfaces diverged, so a session consumer couldn't drive a spawned
// flow via the flow-addressed API at all. This mirrors
// `story-runner-flow.test.ts`'s flow-addressed coverage for the session
// wrapper, proving the two stay parallel (the "add the parallel wrapper"
// house rule).
//
// Also covers #999 for the session leg: `FlowHandle.continueMaximally`
// forwards to the raw `continue_flow_maximally` wasm binding in one call
// (the actual line-limit enforcement is covered in `crates/brink-runtime`
// and `crates/brink-web`; see `story-runner-flow.test.ts`'s equivalent note).
//
// `brink-web` (the wasm-pack output) is replaced by a call-recording stub so
// this stays a pure wrapper-layer test — real runtime behavior stays covered
// by `crates/brink-runtime` and the Rust-side `wasm_bindgen_test` modules in
// `crates/brink-web`.

const hoisted = vi.hoisted(() => {
  const calls: Array<{ method: string; args: unknown[] }> = [];
  class WebSessionStub {
    constructor(...args: unknown[]) {
      calls.push({ method: "new", args });
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
        // Mirrors the wasm leg's `RuntimeError::LineLimitExceeded` shape for
        // an infinite-emitting flow (#999).
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
    flow_debug_snapshot(name: unknown): string {
      calls.push({ method: "flow_debug_snapshot", args: [name] });
      return JSON.stringify({ status: "active" });
    }
    journal_event_count(): number {
      return 0;
    }
  }
  return { calls, WebSessionStub };
});

vi.mock("brink-web", () => ({
  default: async () => undefined,
  compile: () => "",
  compile_fragment: () => "",
  program_checksum: () => "",
  token_type_names: () => "[]",
  token_modifier_names: () => "[]",
  EditorSession: class {},
  StoryRunner: class {},
  WebSpeculation: class {},
  WebSession: hoisted.WebSessionStub,
  diffSnapshots: () => "",
}));

import { StorySessionHandle, FlowHandle } from "../index";

function newSession(): StorySessionHandle {
  hoisted.calls.length = 0;
  return new StorySessionHandle(new Uint8Array());
}

describe("StorySessionHandle flow-addressed consumption (#1000)", () => {
  it("spawnFlow returns a FlowHandle and forwards spawn_flow(name, path), aligned with StoryRunnerHandle.spawnFlow", () => {
    const session = newSession();
    const handle = session.spawnFlow("npc", "start");
    expect(handle).toBeInstanceOf(FlowHandle);
    expect(handle.name).toBe("npc");
    expect(hoisted.calls).toContainEqual({
      method: "spawn_flow",
      args: ["npc", "start"],
    });
  });

  it("flow(name) yields a handle whose continue() drives that flow's stream", () => {
    const session = newSession();
    const handle = session.flow("npc");
    const line = handle.continue();
    expect(line).toEqual({ type: "text", text: "npc", tags: [] });
    expect(hoisted.calls).toContainEqual({
      method: "continue_flow",
      args: ["npc"],
    });
  });

  it("FlowHandle.continueMaximally forwards to the raw continue_flow_maximally in one call (#999)", () => {
    const session = newSession();
    const lines = session.spawnFlow("npc").continueMaximally();
    expect(lines.map((l) => l.type)).toEqual(["text", "done"]);
    expect(hoisted.calls.filter((c) => c.method === "continue_flow_maximally")).toHaveLength(1);
    expect(hoisted.calls.filter((c) => c.method === "continue_flow")).toHaveLength(0);
  });

  it("FlowHandle.continueMaximally propagates the wasm leg's line-limit error unchanged (#999)", () => {
    const session = newSession();
    expect(() => session.flow("boom").continueMaximally()).toThrow(/line limit exceeded/);
  });

  it("FlowHandle.choose / debugSnapshot / destroy forward with the flow id", () => {
    const session = newSession();
    const handle = session.flow("npc");
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
