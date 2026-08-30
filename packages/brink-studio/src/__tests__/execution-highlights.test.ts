/**
 * W6/#3299 — the execution-highlight POLICY, over real store state.
 *
 * The extension mechanics are pinned in ink-editor's own suite; the wasm
 * position→file:line road in `crates/brink-web`. What this suite pins is
 * the studio's policy: live vs paused kinds, suppressed-never-stale
 * under degraded, non-live statuses dark, wrong-file dark, and the range
 * riding along for the finer tiers.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore, ALL_CAPABILITIES } from "@brink/studio-store";
import { executionHighlightsFor } from "../execution-highlights";

function stateWith(overrides: {
  paused?: boolean;
  status?: string;
  program?: string | null;
  compiled?: string | null;
  position?: { container_idx: number; offset: number } | null;
  resolved?: { file: string; line: number; range_start: number; range_len: number } | null;
}) {
  const store = createStudioStore();
  store.setState({
    sessionStatus: (overrides.status ?? "running") as never,
    sessionPaused: overrides.paused ?? false,
    programChecksum: overrides.program === undefined ? "abc" : overrides.program,
    compiledChecksum: overrides.compiled === undefined ? "abc" : overrides.compiled,
    debugState:
      overrides.position === null
        ? null
        : ({ position: overrides.position ?? { container_idx: 3, offset: 7 } } as never),
    _provider: {
      capabilities: ALL_CAPABILITIES,
      resolveDebugLine: vi.fn(
        () =>
          overrides.resolved ?? {
            file: "main.ink",
            line: 4,
            range_start: 100,
            range_len: 12,
          },
      ),
    } as never,
  });
  return store.getState();
}

describe("executionHighlightsFor (W6/#3299)", () => {
  it("play is stepping: a running session lights the live band, 1-based", () => {
    expect(executionHighlightsFor(stateWith({}), "main.ink")).toEqual([
      { line: 5, kind: "live", rangeStart: 100, rangeLen: 12 },
    ]);
  });

  it("paused turns the band warning-kind", () => {
    expect(executionHighlightsFor(stateWith({ paused: true }), "main.ink")).toEqual([
      { line: 5, kind: "paused", rangeStart: 100, rangeLen: 12 },
    ]);
  });

  it("suppressed, never stale: a degraded session lights nothing", () => {
    expect(
      executionHighlightsFor(stateWith({ program: "old", compiled: "new" }), "main.ink"),
    ).toEqual([]);
  });

  it("only the position's own file lights", () => {
    expect(executionHighlightsFor(stateWith({}), "other.brink")).toEqual([]);
  });

  it("ended / errored / no-session states are dark", () => {
    for (const status of ["none", "ended", "error"]) {
      expect(executionHighlightsFor(stateWith({ status }), "main.ink")).toEqual([]);
    }
  });

  it("no runtime position (or no debug info to resolve it) is dark", () => {
    expect(executionHighlightsFor(stateWith({ position: null }), "main.ink")).toEqual([]);
    const st = stateWith({});
    (st._provider as unknown as { resolveDebugLine: unknown }).resolveDebugLine = () => null;
    expect(executionHighlightsFor(st, "main.ink")).toEqual([]);
  });
});
