/**
 * The Binder's structural symbol menu, end to end (#2577).
 *
 * `dispatchSymbolAction` (`packages/studio-ui/src/symbolMenuActions.ts`) is the
 * one dispatcher behind every knot/stitch context-menu refactor — the Binder's
 * menu and its drag-drop, the editor, and the Story Graph all route through it.
 * Seven of its branches (`reorderStitch`, `reorderKnot`, `reorderStitches`,
 * `reorderKnots`, `moveStitch`, `promoteStitch`, `demoteKnot`) call ops that the
 * studio's wasm mock had NO METHOD for at all, so before #2577 this file could
 * not have existed: every case below threw
 * `session.reorderStitch is not a function` — the op was untestable, not merely
 * untested.
 *
 * These run the real `ProjectSession` + real studio store against the mock (the
 * unit suite's `brink-web` alias), so what is exercised is the production
 * dispatcher and the production apply path, not a re-implementation.
 *
 * ⚠ The refusal cases below pin what the dispatcher does TODAY: `result.ok`
 * false → it returns silently, applying nothing and telling the user nothing.
 * That silence is #2544's production-side reporting contract (the rename
 * surfaces already notify; these seven do not), which needs a maintainer
 * ruling — so it is pinned here as observed behavior, not asserted as correct.
 * The value of pinning it is that #2544 now has a test to flip.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import {
  createStudioStore,
  type DocumentSessions as StoreDocs,
  type StoreNotification,
} from "@brink/studio-store";
import { dispatchSymbolAction } from "@brink/studio-ui";

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

/** A store wired the way `mount.tsx` wires it, plus a capture of every
 *  notification raised (the channel a refusal would report through). */
async function makeStore(files: Record<string, string>) {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const store = createStudioStore();
  store.setState({ _project: project, _documents: stubDocuments() });
  const raised: StoreNotification[] = [];
  store.getState().setNotifier((n) => raised.push(n));
  return { store, project, raised };
}

const MAIN = [
  "=== one ===",
  "First.",
  "= alpha",
  "A.",
  "= beta",
  "B.",
  "",
  "=== two ===",
  "Second.",
  "",
].join("\n");

/** Order of the stitch headers as they appear in a file's source. */
function stitchOrder(source: string): string[] {
  return [...source.matchAll(/^=\s+(\w+)/gm)].map((m) => m[1]!);
}

/** Order of the knot headers as they appear in a file's source. */
function knotOrder(source: string): string[] {
  return [...source.matchAll(/^===\s+(\w+)\s*===/gm)].map((m) => m[1]!);
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("dispatchSymbolAction: the structural ops the mock had no method for (#2577)", () => {
  it("reorderStitch moves a stitch down and writes the file back", async () => {
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderStitch",
      path: "main.ink",
      knot: "one",
      stitch: "alpha",
      direction: 1,
    });

    const src = project.getSession().getFileSource("main.ink")!;
    expect(stitchOrder(src)).toEqual(["beta", "alpha"]);
    // The knot header and the preamble-free body around it are untouched.
    expect(knotOrder(src)).toEqual(["one", "two"]);
    expect(src).toContain("=== one ===\nFirst.\n");
    // A successful apply raises exactly its one informational toast.
    expect(raised.filter((n) => n.severity === "error")).toHaveLength(0);
  });

  it("reorderKnot moves a knot down", async () => {
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderKnot",
      path: "main.ink",
      knot: "one",
      direction: 1,
    });

    expect(knotOrder(project.getSession().getFileSource("main.ink")!)).toEqual(["two", "one"]);
  });

  it("reorderStitches applies a whole drag-drop permutation", async () => {
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderStitches",
      path: "main.ink",
      knot: "one",
      order: ["beta", "alpha"],
    });

    expect(stitchOrder(project.getSession().getFileSource("main.ink")!)).toEqual([
      "beta",
      "alpha",
    ]);
  });

  it("moveStitch relocates a stitch and requalifies its diverts across files", async () => {
    const { store, project } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> one.alpha\n",
    });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });

    const session = project.getSession();
    const src = session.getFileSource("main.ink")!;
    // `alpha` now sits after `two`'s header, and `one` keeps only `beta`.
    expect(src.indexOf("= alpha")).toBeGreaterThan(src.indexOf("=== two ==="));
    expect(src.indexOf("= beta")).toBeLessThan(src.indexOf("=== two ==="));
    // The cross-file divert travelled with it.
    expect(session.getFileSource("other.ink")).toContain("-> two.alpha");
  });

  it("promoteStitch lifts a stitch to a knot and unqualifies its diverts", async () => {
    const { store, project } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> one.alpha\n",
    });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "promoteStitch",
      path: "main.ink",
      knot: "one",
      stitch: "alpha",
    });

    const session = project.getSession();
    const src = session.getFileSource("main.ink")!;
    // Promoted immediately after its former parent — the real op's position.
    expect(knotOrder(src)).toEqual(["one", "alpha", "two"]);
    expect(stitchOrder(src)).toEqual(["beta"]);
    expect(session.getFileSource("other.ink")).toContain("-> alpha");
  });

  it("demoteKnot folds a knot into another as its last stitch", async () => {
    const { store, project } = await makeStore({
      "main.ink": MAIN,
      "other.ink": "-> two\n",
    });
    const state = store.getState();

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "demoteKnot",
      path: "main.ink",
      knot: "two",
      destKnot: "one",
    });

    const session = project.getSession();
    const src = session.getFileSource("main.ink")!;
    expect(knotOrder(src)).toEqual(["one"]);
    expect(stitchOrder(src)).toEqual(["alpha", "beta", "two"]);
    expect(session.getFileSource("other.ink")).toContain("-> one.two");
  });
});

describe("a refused structural op applies nothing (#2577; reporting is #2544)", () => {
  it("moveStitch onto an occupied name leaves every file untouched", async () => {
    // `two` already owns an `alpha`, so the destination-collision check — the
    // one the real op runs before it even resolves the source stitch — refuses.
    const COLLIDING = `${MAIN}= alpha\nOther A.\n`;
    const OTHER = "-> one.alpha\n";
    const { store, project, raised } = await makeStore({
      "main.ink": COLLIDING,
      "other.ink": OTHER,
    });
    const state = store.getState();
    const before = project.getSession().getFileSource("main.ink")!;

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "moveStitch",
      path: "main.ink",
      srcKnot: "one",
      stitch: "alpha",
      destKnot: "two",
    });

    expect(project.getSession().getFileSource("main.ink")).toBe(before);
    // Nothing partial: the cross-file requalification did not happen either.
    expect(project.getSession().getFileSource("other.ink")).toBe(OTHER);

    // ⚠ #2544: the dispatcher's `if (result.ok && result.path)` swallows the
    // refusal — no notification, no toast, nothing distinguishes "destination
    // knot not found" from "you clicked and nothing needed doing". The rename
    // surfaces DO report (`notifyRenameRefusal`, #2528/#2543); these seven do
    // not. Pinned, not endorsed — flipping it is #2544's ruling to make.
    expect(raised).toEqual([]);
  });

  it("promoteStitch onto an existing knot name refuses without a partial write", async () => {
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const before = project.getSession().getFileSource("main.ink")!;

    // Promoting `alpha` is fine, but a stitch named `two` would collide with
    // the existing top-level knot — the real op's first check.
    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "promoteStitch",
      path: "main.ink",
      knot: "one",
      stitch: "two",
    });

    expect(project.getSession().getFileSource("main.ink")).toBe(before);
    expect(raised).toEqual([]);
  });

  it("reorderStitches with a non-permutation refuses", async () => {
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const before = project.getSession().getFileSource("main.ink")!;

    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "reorderStitches",
      path: "main.ink",
      knot: "one",
      order: ["alpha", "alpha"],
    });

    expect(project.getSession().getFileSource("main.ink")).toBe(before);
  });
});

/**
 * The two remaining ops #2577 added, driven through the real `@brink-lang/web`
 * wrapper (`ProjectSession.getSession()`) rather than the mock class directly,
 * so the wrapper's own JSON parse + typing is in the loop.
 *
 * `renameDir` has no studio consumer yet — the Binder's folder rename does not
 * call it — so this is the op's first exercise from TypeScript at all; the
 * `DirMoveResult` shape it answers is the one PR #2573 generated into the
 * refusal fixture with nothing to compare against.
 */
describe("renameDir and resolveCodeAction through the wasm wrapper (#2577)", () => {
  it("renameDir relocates every file under the folder and re-points inbound INCLUDEs", async () => {
    const { project } = await makeStore({
      "main.ink": "INCLUDE chapters/one.ink\n-> one\n",
      "chapters/one.ink": "=== one ===\nHi.\n-> END\n",
    });

    const result = project.getSession().renameDir("chapters", "acts");

    expect(result.ok).toBe(true);
    expect(result.moved_files).toEqual([
      {
        old_path: "chapters/one.ink",
        new_path: "acts/one.ink",
        new_source: "=== one ===\nHi.\n-> END\n",
      },
    ]);
    expect(result.cross_file_edits).toEqual([
      { path: "main.ink", new_source: "INCLUDE acts/one.ink\n-> one\n" },
    ]);
    expect(result.safe).toBe(true);
    expect(result.introduced_diagnostics).toEqual([]);
  });

  it("renameDir trims trailing slashes off both prefixes, like the real op", async () => {
    const { project } = await makeStore({
      "main.ink": "INCLUDE chapters/one.ink\n-> one\n",
      "chapters/one.ink": "=== one ===\nHi.\n-> END\n",
    });

    // brink_ide::dir_rename::rename_dir trims trailing slashes off both
    // prefixes before matching (crates/internal/brink-ide/src/dir_rename.rs:
    // 123-124); a trailing slash on either side must still succeed here.
    const result = project.getSession().renameDir("chapters/", "acts/");

    expect(result.ok).toBe(true);
    expect(result.moved_files).toEqual([
      {
        old_path: "chapters/one.ink",
        new_path: "acts/one.ink",
        new_source: "=== one ===\nHi.\n-> END\n",
      },
    ]);
    expect(result.cross_file_edits).toEqual([
      { path: "main.ink", new_source: "INCLUDE acts/one.ink\n-> one\n" },
    ]);
  });

  it("renameDir refuses an empty folder in the real op's wording", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });

    const result = project.getSession().renameDir("ghost", "other");

    expect(result.ok).toBe(false);
    expect(result.error).toBe("no files found under directory 'ghost'");
    // A refusal still ships the whole payload — see structural-refusal-shape.
    expect(result.moved_files).toEqual([]);
    expect(result.safe).toBe(true);
  });

  it("resolveCodeAction resolves a SortKnots action against the active file", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });
    const session = project.getSession();
    session.setActiveFile("main.ink");

    // MAIN's knots are already in order, and the real op reports a rewrite
    // that changes nothing as a refusal, not as a successful no-op.
    const noop = session.resolveCodeAction({ action: "SortKnots" }, 0);
    expect(noop.ok).toBe(false);
    expect(noop.error).toBe("code action produced no change");

    const reversed = await makeStore({
      "main.ink": "=== zeta ===\nZ.\n\n=== alpha ===\nA.\n",
    });
    const other = reversed.project.getSession();
    other.setActiveFile("main.ink");

    const sorted = other.resolveCodeAction({ action: "SortKnots" }, 0);
    expect(sorted.ok).toBe(true);
    expect(sorted.path).toBe("main.ink");
    expect(knotOrder(sorted.new_source!)).toEqual(["alpha", "zeta"]);
  });

  it("resolveCodeAction refuses an action tag it does not know", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });
    const session = project.getSession();
    session.setActiveFile("main.ink");

    const result = session.resolveCodeAction({ action: "Nonsense" }, 0);

    expect(result.ok).toBe(false);
    expect(result.error).toBe("invalid code-action data: unknown variant `Nonsense`");
  });
});
