/**
 * Runtime-value hover policy (W12/#3305): globals always while live,
 * frame locals only while paused in the selected frame's scope, and the
 * ruled suppressions (degraded, no session) — suppressed, never stale.
 */
import { describe, expect, it } from "vitest";
import { createStudioStore } from "@brink/studio-store";
import { debugValueDisplay, runtimeValueNote } from "../runtime-hover";

function stateWith(overrides: {
  status?: string;
  paused?: boolean;
  program?: string;
  compiled?: string;
  frameIdx?: number | null;
}) {
  const store = createStudioStore();
  store.setState({
    sessionStatus: (overrides.status ?? "running") as never,
    sessionPaused: overrides.paused ?? false,
    programChecksum: overrides.program ?? "0xabc",
    compiledChecksum: overrides.compiled ?? "0xabc",
    selectedFrameIdx: overrides.frameIdx ?? null,
    debugState: {
      globals: [{ name: "gold", value: "12" }],
      call_stack: [
        {
          kind: "function",
          temps: 1,
          locals: [{ slot: 0, name: "price", value: { type: "int", value: 6 } }],
        },
        {
          kind: "root",
          temps: 1,
          locals: [{ slot: 0, name: "price", value: { type: "int", value: 9 } }],
        },
      ],
    } as never,
  });
  return store.getState();
}

describe("runtimeValueNote (W12/#3305)", () => {
  it("a global's live value shows whenever the session runs", () => {
    expect(runtimeValueNote(stateWith({}), "gold")).toBe(
      "`gold = 12` — global, runtime",
    );
  });

  it("frame locals show only while PAUSED, scoped to the selected frame", () => {
    expect(runtimeValueNote(stateWith({}), "price")).toBeNull();
    expect(runtimeValueNote(stateWith({ paused: true }), "price")).toBe(
      "`price = 6` — local, runtime",
    );
    expect(runtimeValueNote(stateWith({ paused: true, frameIdx: 1 }), "price")).toBe(
      "`price = 9` — local, runtime",
    );
  });

  it("degraded and dead sessions show nothing — suppressed, never stale", () => {
    expect(
      runtimeValueNote(stateWith({ program: "0xOLD", compiled: "0xNEW" }), "gold"),
    ).toBeNull();
    for (const status of ["none", "ended", "error"]) {
      expect(runtimeValueNote(stateWith({ status }), "gold")).toBeNull();
    }
  });

  it("an unknown identifier shows nothing", () => {
    expect(runtimeValueNote(stateWith({}), "torchbearer")).toBeNull();
  });
});

describe("debugValueDisplay", () => {
  it("renders each variant one-line", () => {
    expect(debugValueDisplay({ type: "string", value: "hi" })).toBe('"hi"');
    expect(debugValueDisplay({ type: "list", members: ["sword", "charm"] })).toBe(
      "(sword, charm)",
    );
    expect(
      debugValueDisplay({
        type: "struct",
        name: "Pos",
        fields: [{ name: "x", value: { type: "int", value: 3 } }],
      }),
    ).toBe("Pos{x: 3}");
    expect(debugValueDisplay({ type: "other", display: "<weird>" })).toBe("<weird>");
  });
});
