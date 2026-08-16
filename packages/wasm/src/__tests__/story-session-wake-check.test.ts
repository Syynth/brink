import { describe, it, expect, vi } from "vitest";

// Review finding on PR #992 (issue #978, FS-3w): `WebSession::wake_check`
// (crates/brink-web/src/session.rs) had no passthrough on `StorySessionHandle`
// — the only `wakeCheck()` wrapper wrapped `StoryRunnerHandle`, a different
// raw type, so the session-level lever was unreachable from
// `@brink-lang/web` consumers using `StorySessionHandle` (spawnFlow/
// continueFlow/chooseFlow/destroyFlow/flowNames/flowDebugSnapshot). This
// mirrors `story-runner-flow.test.ts`'s `wakeCheck` coverage for the session
// wrapper.
//
// `brink-web` (the wasm-pack output) is replaced with a call-recording stub
// so this stays a pure wrapper-layer test: it pins the passthrough wiring,
// not wasm behavior — the Rust side is covered in `crates/brink-web`.

const hoisted = vi.hoisted(() => {
  const calls: Array<{ method: string; args: unknown[] }> = [];
  class WebSessionStub {
    constructor(...args: unknown[]) {
      calls.push({ method: "new", args });
    }
    wake_check(): string {
      calls.push({ method: "wake_check", args: [] });
      return JSON.stringify([]);
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

import { StorySessionHandle } from "../index";

describe("StorySessionHandle.wakeCheck (FS-3w §10.2 review fix)", () => {
  it("exposes wakeCheck (PR #992 review: the raw WebSession::wake_check was unreachable without it)", () => {
    const session = new StorySessionHandle(new Uint8Array());
    expect(typeof session.wakeCheck).toBe("function");
  });

  it("forwards to the raw wake_check and returns an empty woken list until parks exist", () => {
    hoisted.calls.length = 0;
    const session = new StorySessionHandle(new Uint8Array());
    expect(session.wakeCheck()).toEqual([]);
    expect(hoisted.calls.map((c) => c.method)).toContain("wake_check");
  });
});
