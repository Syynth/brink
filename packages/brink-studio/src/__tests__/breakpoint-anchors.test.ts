/**
 * Source-anchored breakpoints (W4/#3297) — the store model.
 *
 * The stored identity is `(file, line)`; the runtime `(container_idx,
 * offset)` set is DERIVED, re-bound through the provider's
 * `resolveSourceLine` on every sync and re-armed from scratch. This suite
 * drives the real slice over a fake debug provider whose runtime
 * breakpoint set behaves like `BreakpointSet` (insert returns ids, remove
 * deletes), so arming/derivation is asserted against observable provider
 * state, not implementation calls alone.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore, ALL_CAPABILITIES } from "@brink/studio-store";
import type { Breakpoint, ProgramAddress } from "@brink/wasm-types";

/** A provider double with a real little breakpoint set. */
function fakeProvider(
  resolve: (file: string, line: number) => ProgramAddress | null,
) {
  let nextId = 0;
  const armed: Breakpoint[] = [];
  const provider = {
    kind: "local" as const,
    capabilities: ALL_CAPABILITIES,
    resolveSourceLine: vi.fn(resolve),
    debugBreakpointAdd(containerIdx: number, offset: number, name?: string): number {
      const id = nextId++;
      armed.push({
        id,
        container_idx: containerIdx,
        offset,
        enabled: true,
        name: name ?? `${containerIdx}:${offset}`,
      });
      return id;
    },
    debugBreakpointRemove(id: number): boolean {
      const at = armed.findIndex((b) => b.id === id);
      if (at < 0) return false;
      armed.splice(at, 1);
      return true;
    },
    debugBreakpoints: () => [...armed],
  };
  return { provider, armed };
}

function storeWith(resolve: (file: string, line: number) => ProgramAddress | null) {
  const store = createStudioStore();
  const { provider, armed } = fakeProvider(resolve);
  store.setState({ _provider: provider as never });
  return { store, provider, armed };
}

const codeAt =
  (lines: Record<string, number[]>) =>
  (file: string, line: number): ProgramAddress | null =>
    (lines[file] ?? []).includes(line) ? { container_idx: 1, offset: line * 10 } : null;

describe("source-anchored breakpoints (W4/#3297)", () => {
  it("toggle adds a bound, armed anchor; toggling the same line removes it", () => {
    const { store, armed } = storeWith(codeAt({ "main.ink": [4] }));

    store.getState().breakpointToggleAtLine("main.ink", 4);
    const [a] = store.getState().sourceBreakpoints;
    expect(a).toMatchObject({ file: "main.ink", line: 4, enabled: true });
    expect(a?.address).toEqual({ container_idx: 1, offset: 40 });
    expect(armed).toHaveLength(1);
    expect(armed[0]?.name).toBe("main.ink:5"); // 1-based display name

    store.getState().breakpointToggleAtLine("main.ink", 4);
    expect(store.getState().sourceBreakpoints).toHaveLength(0);
    expect(armed).toHaveLength(0);
  });

  it("snaps a no-code line to the nearest following bindable line (spec F2)", () => {
    const { store } = storeWith(codeAt({ "main.ink": [6] }));

    store.getState().breakpointToggleAtLine("main.ink", 3);
    expect(store.getState().sourceBreakpoints[0]?.line).toBe(6);

    // Toggling where the dot actually renders removes it — the whole point
    // of anchoring at the snapped line.
    store.getState().breakpointToggleAtLine("main.ink", 6);
    expect(store.getState().sourceBreakpoints).toHaveLength(0);
  });

  it("keeps anchors and nulls bindings without a debug provider", () => {
    const store = createStudioStore();
    store.getState().breakpointToggleAtLine("main.ink", 2);
    const [a] = store.getState().sourceBreakpoints;
    expect(a).toMatchObject({ file: "main.ink", line: 2, address: null });
  });

  it("disabled anchors stay listed but are never armed", () => {
    const { store, armed } = storeWith(codeAt({ "main.ink": [4] }));
    store.getState().breakpointToggleAtLine("main.ink", 4);
    const key = store.getState().sourceBreakpoints[0]?.key ?? "";

    store.getState().breakpointSetEnabled(key, false);
    expect(store.getState().sourceBreakpoints[0]?.enabled).toBe(false);
    expect(armed).toHaveLength(0);

    store.getState().breakpointSetEnabled(key, true);
    expect(armed).toHaveLength(1);
  });

  it("re-arms from scratch on every sync — the runtime set never drifts", () => {
    const { store, provider, armed } = storeWith(codeAt({ "main.ink": [4, 8] }));
    store.getState().breakpointToggleAtLine("main.ink", 4);
    store.getState().breakpointToggleAtLine("main.ink", 8);
    expect(armed).toHaveLength(2);

    // A stray runtime breakpoint (e.g. armed by a raw debug command) is
    // cleared by the next sync: anchors are the only source of truth.
    provider.debugBreakpointAdd(9, 999, "stray");
    store.getState()._syncSourceBreakpoints();
    expect(armed).toHaveLength(2);
    expect(armed.every((b) => b.name.startsWith("main.ink:"))).toBe(true);
  });

  it("moves shift lines through edits and collapse duplicates", () => {
    const { store } = storeWith(codeAt({ "main.ink": [2, 5] }));
    store.getState().breakpointToggleAtLine("main.ink", 2);
    store.getState().breakpointToggleAtLine("main.ink", 5);

    store.getState().breakpointsMoved("main.ink", [
      { from: 2, to: 3 },
      { from: 5, to: 6 },
    ]);
    expect(store.getState().sourceBreakpoints.map((b) => b.line)).toEqual([3, 6]);

    // A deletion mapped both onto line 3 — one survives.
    store.getState().breakpointsMoved("main.ink", [{ from: 6, to: 3 }]);
    expect(store.getState().sourceBreakpoints.map((b) => b.line)).toEqual([3]);
  });

  it("persists the minimal 0-based shape through the sink on every mutation", () => {
    const sink = vi.fn();
    const { store } = storeWith(codeAt({ "main.ink": [4] }));
    store.getState().setBreakpointsSink(sink);

    store.getState().breakpointToggleAtLine("main.ink", 4);
    expect(sink).toHaveBeenLastCalledWith([{ file: "main.ink", line: 4, enabled: true }]);

    const key = store.getState().sourceBreakpoints[0]?.key ?? "";
    store.getState().breakpointSetEnabled(key, false);
    expect(sink).toHaveBeenLastCalledWith([{ file: "main.ink", line: 4, enabled: false }]);

    store.getState().breakpointRemove(key);
    expect(sink).toHaveBeenLastCalledWith([]);
  });

  it("applyPersistedBreakpoints seeds and binds in one step", () => {
    const { store, armed } = storeWith(codeAt({ "main.ink": [4], "other.brink": [1] }));
    store.getState().applyPersistedBreakpoints([
      { file: "main.ink", line: 4, enabled: true },
      { file: "other.brink", line: 1, enabled: false },
    ]);
    const anchors = store.getState().sourceBreakpoints;
    expect(anchors).toHaveLength(2);
    expect(anchors[0]?.address).not.toBeNull();
    expect(armed).toHaveLength(1); // the disabled one is listed, not armed
  });

  it("a compile result triggers a rebind (the derived half goes stale exactly then)", () => {
    const { store, provider } = storeWith(codeAt({ "main.ink": [4] }));
    store.getState().breakpointToggleAtLine("main.ink", 4);
    const before = provider.resolveSourceLine.mock.calls.length;

    store.getState().setCompileResult([], { errors: 0, warnings: 0 }, [], null);
    expect(provider.resolveSourceLine.mock.calls.length).toBeGreaterThan(before);
  });
});
