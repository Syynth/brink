/**
 * Conflict slice tests (issue #320, Track V).
 *
 * The slice mirrors B1 conflicts into reactive state for the merge view and
 * routes the three resolutions back through the bound ProjectSession. These
 * tests prove: a recorded conflict shows up in `conflicts`/`conflictPaths`,
 * and each resolve action calls the matching ProjectSession method AND drops
 * the conflict from the slice (so the surface tears down). A stale or
 * unknown-path resolution is a no-op.
 */

import { describe, it, expect, vi } from "vitest";
import { createStudioStore, conflictPaths, type ProjectSession } from "@brink/studio-store";
import type { FileConflict } from "@brink/ink-editor";

const CONFLICT: FileConflict = {
  path: "main.ink",
  disk: "host edit",
  buffer: "studio edit",
  baseline: "original",
};

/** A ProjectSession test double recording the resolve calls. */
function stubProject() {
  return {
    resolveConflictUseDisk: vi.fn(),
    resolveConflictKeepMine: vi.fn(),
    resolveConflictMerged: vi.fn(),
  } as unknown as ProjectSession & {
    resolveConflictUseDisk: ReturnType<typeof vi.fn>;
    resolveConflictKeepMine: ReturnType<typeof vi.fn>;
    resolveConflictMerged: ReturnType<typeof vi.fn>;
  };
}

function makeStore() {
  const store = createStudioStore();
  const project = stubProject();
  store.setState({ _project: project });
  return { store, project };
}

describe("conflict slice", () => {
  it("starts with no conflicts", () => {
    const { store } = makeStore();
    expect(store.getState().conflicts).toEqual({});
    expect(conflictPaths(store.getState().conflicts)).toEqual([]);
  });

  it("setConflict records a conflict keyed by path", () => {
    const { store } = makeStore();
    store.getState().setConflict(CONFLICT);
    expect(store.getState().conflicts).toEqual({ "main.ink": CONFLICT });
    expect(conflictPaths(store.getState().conflicts)).toEqual(["main.ink"]);
  });

  it("setConflict replaces a prior conflict for the same path", () => {
    const { store } = makeStore();
    store.getState().setConflict(CONFLICT);
    const next: FileConflict = { ...CONFLICT, disk: "host edit 2" };
    store.getState().setConflict(next);
    expect(store.getState().conflicts).toEqual({ "main.ink": next });
  });

  it("conflictPaths is sorted (deterministic)", () => {
    const { store } = makeStore();
    store.getState().setConflict({ ...CONFLICT, path: "z.ink" });
    store.getState().setConflict({ ...CONFLICT, path: "a.ink" });
    expect(conflictPaths(store.getState().conflicts)).toEqual(["a.ink", "z.ink"]);
  });

  it("resolveUseDisk calls the project with the disk text and drops the conflict", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().resolveUseDisk("main.ink");
    expect(project.resolveConflictUseDisk).toHaveBeenCalledWith("main.ink", "host edit");
    expect(store.getState().conflicts).toEqual({});
  });

  it("resolveKeepMine calls the project and drops the conflict", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().resolveKeepMine("main.ink");
    expect(project.resolveConflictKeepMine).toHaveBeenCalledWith("main.ink");
    expect(store.getState().conflicts).toEqual({});
  });

  it("resolveMerge calls the project with the merged text and drops the conflict", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().resolveMerge("main.ink", "merged result");
    expect(project.resolveConflictMerged).toHaveBeenCalledWith("main.ink", "merged result");
    expect(store.getState().conflicts).toEqual({});
  });

  it("clearConflict drops a conflict without resolving it", () => {
    const { store, project } = makeStore();
    store.getState().setConflict(CONFLICT);
    store.getState().clearConflict("main.ink");
    expect(store.getState().conflicts).toEqual({});
    expect(project.resolveConflictKeepMine).not.toHaveBeenCalled();
    expect(project.resolveConflictUseDisk).not.toHaveBeenCalled();
  });

  it("resolving an unknown path is a no-op", () => {
    const { store, project } = makeStore();
    store.getState().resolveUseDisk("nope.ink");
    expect(project.resolveConflictUseDisk).not.toHaveBeenCalled();
    expect(store.getState().conflicts).toEqual({});
  });
});
