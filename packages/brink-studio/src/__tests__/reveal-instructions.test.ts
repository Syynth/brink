/**
 * `revealInstructionsAt` (W9/#3302) — the "Reveal in Program Explorer"
 * store half: resolves a source line through the session's resolver road,
 * targets the explorer (nonce-bumped for repeat reveals), and raises the
 * honest notifications instead of silently no-oping.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore, LocalSessionProvider } from "@brink/studio-store";

function sessionWith(resolve: (file: string, line: number) => unknown) {
  return {
    continueSingle: vi.fn(),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    resolveSourceLine: vi.fn(resolve),
    hasDebugInfo: vi.fn(() => true),
    debugBreakpoints: vi.fn(() => []),
  };
}

function bind(session: ReturnType<typeof sessionWith>) {
  const store = createStudioStore();
  const notices: string[] = [];
  store.setState({ _notify: (n) => notices.push(n.message) });
  const provider = new LocalSessionProvider({
    session: session as never,
    status: "running",
  });
  store.getState()._bindProvider(provider);
  return { store, notices };
}

describe("revealInstructionsAt (W9/#3302)", () => {
  it("resolves and targets the explorer; repeat reveals bump the nonce", () => {
    const session = sessionWith(() => ({ container_idx: 7, offset: 3 }));
    const { store } = bind(session);

    expect(store.getState().revealInstructionsAt("main.ink", 4)).toBe(true);
    expect(session.resolveSourceLine).toHaveBeenCalledWith("main.ink", 4);
    expect(store.getState().programExplorerTarget).toEqual({
      address: { container_idx: 7, offset: 3 },
      nonce: 1,
    });

    expect(store.getState().revealInstructionsAt("main.ink", 4)).toBe(true);
    expect(store.getState().programExplorerTarget?.nonce).toBe(2);
  });

  it("a line with no statement raises the honest notice", () => {
    const session = sessionWith(() => null);
    const { store, notices } = bind(session);

    expect(store.getState().revealInstructionsAt("main.ink", 9)).toBe(false);
    expect(store.getState().programExplorerTarget).toBeNull();
    expect(notices.join(" ")).toContain("No compiled instructions for main.ink:10");
  });

  it("no live session: says to start the story", () => {
    const store = createStudioStore();
    const notices: string[] = [];
    store.setState({ _notify: (n) => notices.push(n.message) });

    expect(store.getState().revealInstructionsAt("main.ink", 0)).toBe(false);
    expect(notices.join(" ")).toContain("Start the story");
  });
});
