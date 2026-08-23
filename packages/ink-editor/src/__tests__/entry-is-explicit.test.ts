/**
 * `ProjectSessionOptions.entryIsExplicit` (the file-anchored project open
 * model, ruled 2026-08-23 — `docs/decision-log.md` "A project is anchored
 * on a FILE"): a HUMAN'S EXPLICIT file open beats a governing
 * `brink.toml`'s `[project] entry`, revising #2331 for exactly this case —
 * that ruling stands for host-supplied *defaults*, which still lose to
 * authored config.
 *
 * Uses the same hand-built stub `session` pattern as
 * `project-session-destroy.test.ts` (no built wasm pkg needed): the
 * behavior under test lives entirely in `ProjectSession`'s precedence
 * bookkeeping, never in the wasm ops themselves.
 */

import { describe, it, expect } from "vitest";
import { ProjectSession, type ProjectSessionOptions } from "../project-session.js";
import { InMemoryFileProvider } from "../provider.js";

/** Stub session simulating a project whose `brink.toml` names `main.ink`
 *  as `[project] entry` while the host asked for `chapter3.ink`. */
function makeStubSession() {
  return {
    generation: 0,
    updateFile: () => {},
    removeFile: () => {},
    // Both files "exist" in the session, so a configured entry resolves.
    getFileSource: (path: string) =>
      path === "main.ink" || path === "chapter3.ink" ? "-> END\n" : null,
    discoverProjectConfig: () => [],
    getConfiguredEntry: () => "main.ink",
    getFileIncludes: () => [],
    listFiles: () => [],
    compileProject: () => {},
    free: () => {},
  };
}

async function initializedSession(
  extra: Partial<ProjectSessionOptions>,
): Promise<ProjectSession> {
  const provider = new InMemoryFileProvider({
    "main.ink": "-> END\n",
    "chapter3.ink": "-> END\n",
    "brink.toml": '[project]\nentry = "main.ink"\n',
  });
  const project = new ProjectSession({
    provider,
    entryFile: "chapter3.ink",
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
    session: makeStubSession() as any,
    ...extra,
  });
  await project.initialize();
  return project;
}

describe("entryIsExplicit (file-anchored open, ruled 2026-08-23)", () => {
  it("by default, a resolving [project] entry supersedes the host entryFile (#2331)", async () => {
    const project = await initializedSession({});
    expect(project.getEntryFile()).toBe("main.ink");
    project.destroy();
  });

  it("an explicit open keeps the opened file as entry even when config names another", async () => {
    const project = await initializedSession({ entryIsExplicit: true });
    expect(project.getEntryFile()).toBe("chapter3.ink");
    project.destroy();
  });
});
