import { describe, it, expect, vi } from "vitest";

// Issue #1573 (review finding on PR #1577): `Story::did_safe_exit` /
// `FlowInstance::did_safe_exit` were promoted off the `testing` feature gate
// and the raw `#[wasm_bindgen]` methods exist on `StoryRunner`/`WebSession`
// (`crates/brink-web`), but `StoryRunnerHandle`/`StorySessionHandle` — the
// only surface `@brink-lang/web` consumers ever touch — had no `didSafeExit`
// passthrough, so the lever was unreachable from the published package.
//
// `brink-web` (the wasm-pack output) is replaced by a call-recording stub so
// this stays a pure wrapper-layer test — real runtime behavior stays covered
// by the Rust-side `wasm_bindgen_test` modules in `crates/brink-web` and
// `crates/brink-runtime`.

const hoisted = vi.hoisted(() => {
  const calls: Array<{ method: string; args: unknown[] }> = [];
  class StoryRunnerStub {
    constructor(...args: unknown[]) {
      calls.push({ method: "new", args });
    }
    did_safe_exit(): boolean {
      calls.push({ method: "did_safe_exit", args: [] });
      return true;
    }
  }
  class WebSessionStub {
    constructor(...args: unknown[]) {
      calls.push({ method: "new", args });
    }
    did_safe_exit(): boolean {
      calls.push({ method: "did_safe_exit", args: [] });
      return false;
    }
  }
  return { calls, StoryRunnerStub, WebSessionStub };
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
  WebSession: hoisted.WebSessionStub,
  diffSnapshots: () => "",
}));

import { StoryRunnerHandle, StorySessionHandle } from "../index";

describe("didSafeExit wrapper passthrough (#1573)", () => {
  it("StoryRunnerHandle exposes didSafeExit, forwarding to the raw did_safe_exit", () => {
    hoisted.calls.length = 0;
    const runner = new StoryRunnerHandle(new Uint8Array());
    expect(typeof runner.didSafeExit).toBe("function");
    expect(runner.didSafeExit()).toBe(true);
    expect(hoisted.calls).toContainEqual({ method: "did_safe_exit", args: [] });
  });

  it("StorySessionHandle exposes didSafeExit, forwarding to the raw did_safe_exit", () => {
    hoisted.calls.length = 0;
    const session = new StorySessionHandle(new Uint8Array());
    expect(typeof session.didSafeExit).toBe("function");
    expect(session.didSafeExit()).toBe(false);
    expect(hoisted.calls).toContainEqual({ method: "did_safe_exit", args: [] });
  });
});
