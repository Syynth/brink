/**
 * `ProjectSession.compileProjectAsync` (docs/editor-worker-spec.md W2a):
 * the compile road over the async session facade. Pins the client-side
 * compile coalescing (spec §6 — one in flight; concurrent callers share
 * it), the generation cache carrying over from the sync road, and the
 * destroy() teardown rejecting an in-flight compile BEFORE the wasm
 * handle is freed (the freed-wasm discipline).
 *
 * Uses a hand-built stub session (same pattern as
 * `project-session-destroy.test.ts`) — the machinery under test is
 * ProjectSession bookkeeping + the W1 substrate, not the wasm compile.
 */

import { describe, expect, it, vi } from "vitest";
import type { EditorSessionHandle } from "@brink-lang/web";
import { ProjectSession } from "../project-session.js";
import { InMemoryFileProvider } from "../provider.js";
import { QueryDroppedError } from "../worker/session-client.js";

function makeStub() {
  const compileProject = vi.fn(() => ({ warnings: [], files: ["main.ink"] }));
  const free = vi.fn();
  const stub = {
    generation: 0,
    compileProject,
    free,
  };
  return { stub, compileProject, free };
}

function makeSession(stub: object): ProjectSession {
  return new ProjectSession({
    provider: new InMemoryFileProvider({ "main.ink": "-> DONE\n" }),
    entryFile: "main.ink",
    session: stub as unknown as EditorSessionHandle,
  });
}

describe("ProjectSession.compileProjectAsync", () => {
  it("dedups concurrent callers onto one in-flight compile", async () => {
    const { stub, compileProject } = makeStub();
    const project = makeSession(stub);
    const a = project.compileProjectAsync();
    const b = project.compileProjectAsync();
    expect(b).toBe(a); // the literal same promise — spec §6's one-in-flight
    const [ra, rb] = await Promise.all([a, b]);
    expect(compileProject).toHaveBeenCalledTimes(1);
    expect(compileProject).toHaveBeenCalledWith("main.ink");
    expect(ra).toEqual(rb);
  });

  it("serves repeat calls at the same generation from the compile cache", async () => {
    const { stub, compileProject } = makeStub();
    const project = makeSession(stub);
    await project.compileProjectAsync();
    const again = await project.compileProjectAsync();
    expect(compileProject).toHaveBeenCalledTimes(1);
    expect(again).toEqual({ warnings: [], files: ["main.ink"] });
  });

  it("shares the cache with the sync road in both directions", async () => {
    const { stub, compileProject } = makeStub();
    const project = makeSession(stub);
    await project.compileProjectAsync();
    project.compileProject(); // sync road — same generation, cache hit
    expect(compileProject).toHaveBeenCalledTimes(1);
    stub.generation = 1;
    project.compileProject(); // sync road compiles the new generation…
    expect(compileProject).toHaveBeenCalledTimes(2);
    await project.compileProjectAsync(); // …and the async road hits its cache
    expect(compileProject).toHaveBeenCalledTimes(2);
  });

  it("recompiles when the session generation moves", async () => {
    const { stub, compileProject } = makeStub();
    const project = makeSession(stub);
    await project.compileProjectAsync();
    stub.generation = 1;
    await project.compileProjectAsync();
    expect(compileProject).toHaveBeenCalledTimes(2);
  });

  it("destroy() rejects an in-flight compile before freeing the session", async () => {
    const { stub, compileProject, free } = makeStub();
    const project = makeSession(stub);
    const inFlight = project.compileProjectAsync();
    project.destroy();
    await expect(inFlight).rejects.toBeInstanceOf(QueryDroppedError);
    // The queued query never reached the (now freed) session.
    expect(compileProject).not.toHaveBeenCalled();
    expect(free).toHaveBeenCalledTimes(1);
    // A post-destroy landing tick must not resurrect state or throw.
    await Promise.resolve();
  });
});
