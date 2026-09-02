/**
 * Every live road steps ONE line per wasm call and stamps each row with
 * the knot/stitch the runtime reported before that call (ruled
 * 2026-09-02: "TS steps single lines") — so a batch that crosses a knot
 * boundary stamps the rows on each side correctly, and the Player can
 * reset speaker runs on the change.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore, LocalSessionProvider } from "@brink/studio-store";
import type { DebugRunOutcome, Line } from "@brink/wasm-types";

const text = (t: string): Line => ({ type: "text", text: `${t}\n`, tags: [] });
const choices: Line = { type: "choices", text: "", tags: [], choices: [] };

/** A session whose `currentPath()` moves as lines are consumed: the path
 *  BEFORE a continue is where the coming line is from. */
function scripted(feed: Line[], paths: (string | null)[], armed: boolean) {
  let i = 0;
  const outcome = (line: Line): DebugRunOutcome =>
    ({ lines: [line], reason: { type: line.type === "text" ? "step" : "choices" } }) as never;
  return {
    currentPath: vi.fn(() => paths[Math.min(i, paths.length - 1)] ?? null),
    continueSingle: vi.fn((): Line => feed[Math.min(i++, feed.length - 1)]),
    continueToPause: vi.fn((): Line[] => {
      throw new Error("batch road must not be used");
    }),
    debugRun: vi.fn((): DebugRunOutcome => {
      throw new Error("batch debug road must not be used");
    }),
    debugRunToLine: vi.fn((): DebugRunOutcome => outcome(feed[Math.min(i++, feed.length - 1)])),
    debugStep: vi.fn(),
    debugStepLine: vi.fn(),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    resolveDebugPosition: vi.fn(() => null),
    resolveSourceLine: vi.fn(() => null),
    hasDebugInfo: vi.fn(() => true),
    debugBreakpointAdd: vi.fn((): number => 0),
    debugBreakpointRemove: vi.fn((): boolean => true),
    debugBreakpointSetEnabled: vi.fn((): boolean => true),
    // An armed breakpoint is what puts the provider on the debug road.
    debugBreakpoints: vi.fn(() => (armed ? [{ id: 1 }] : [])),
    debugWatchpoints: vi.fn(() => []),
  };
}

function bind(feed: Line[], paths: (string | null)[], armed: boolean) {
  const store = createStudioStore();
  const session = scripted(feed, paths, armed);
  const provider = new LocalSessionProvider({ session: session as never, status: "running" });
  store.getState()._bindProvider(provider);
  store.getState().setSessionPaced(0); // all at once, so one call reveals the run
  return { store, provider, session };
}

const FEED = [text("Pleasure doing business."), text("The temple opens."), text("Three ways."), choices];
// Path read before each continue: line 0 is still in `barter`; from line 1 the runtime sits in `threshold`.
const PATHS = ["barter", "threshold", "threshold", "threshold"];

describe("transcript rows carry the knot/stitch they came from", () => {
  it("journaled road: one continueSingle per line, each stamped with the path read before it", () => {
    const { store, session } = bind(FEED, PATHS, false);
    store.getState().revealMaximally();
    expect(session.continueSingle).toHaveBeenCalledTimes(4);
    expect(session.continueToPause).not.toHaveBeenCalled();
    const rows = store.getState().sessionLines.filter((l) => l.kind === "line");
    expect(rows.map((r) => [r.text, r.path])).toEqual([
      ["Pleasure doing business.", "barter"],
      ["The temple opens.", "threshold"],
      ["Three ways.", "threshold"],
    ]);
    expect(store.getState().sessionStatus).toBe("awaiting-choice");
  });

  it("debug road: one debugRunToLine per line on 'run', same stamping", () => {
    const { store, session } = bind(FEED, PATHS, true);
    store.getState().revealMaximally();
    expect(session.debugRunToLine).toHaveBeenCalledTimes(4);
    expect(session.debugRun).not.toHaveBeenCalled();
    const rows = store.getState().sessionLines.filter((l) => l.kind === "line");
    expect(rows.map((r) => r.path)).toEqual(["barter", "threshold", "threshold"]);
  });

  it("a session without currentPath() stamps nothing", () => {
    const { store, session } = bind(FEED, PATHS, false);
    delete (session as { currentPath?: unknown }).currentPath;
    store.getState().revealMaximally();
    const rows = store.getState().sessionLines.filter((l) => l.kind === "line");
    expect(rows.every((r) => r.path === undefined)).toBe(true);
  });
});
