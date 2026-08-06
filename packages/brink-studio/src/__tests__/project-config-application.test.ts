/**
 * `brink.toml` is actually applied (issue #2324): before this fix,
 * `EditorSessionHandle.applyProjectConfig`/`discoverProjectConfig` (#1005,
 * #1414) were exposed and unit-tested but never called by anything outside
 * test files — `ProjectSession` loaded `brink.toml` into the wasm session as
 * an ordinary document (like every other file) but never asked the session
 * to *discover and apply* it, so every `[project]`/`[lints]` key in it was
 * silently inert. These tests drive `ProjectSession` directly (the seam
 * `discoverProjectConfig` was wired into), proving:
 *
 *  1. `initialize()` discovers and applies `brink.toml` before returning —
 *     "before the first analysis", per the issue's fix shape.
 *  2. An edit to `brink.toml` in the session (the CM6/`applyEdit` path) is
 *     re-applied, not just the initial load.
 *  3. `[project] entry` — which the fixture this issue names
 *     (`packages/brink-studio/src/main.tsx`'s `NATIVE_FIXTURE`) sets — has no
 *     real schema slot in `brink_project_config::ProjectConfig` (verified
 *     against `crates/internal/brink-project-config/src/lib.rs`): it
 *     surfaces as an unrecognized-key warning, not a silently accepted
 *     setting. `mountStudio`'s explicit `entryFile` argument stays the only
 *     thing that decides the entry file — there is nothing at the wasm-session
 *     layer for it to conflict with.
 *
 * Revert the `applyProjectConfig()` calls in `project-session.ts` and test 1
 * fails (zero warnings recorded instead of one) — proving this suite
 * actually exercises the fix, not just the mock.
 */

import { describe, it, expect } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";

async function makeProject(files: Record<string, string>, entryFile: string) {
  await initWasm();
  const warnings: string[][] = [];
  const provider = new InMemoryFileProvider(files);
  const project = new ProjectSession({
    provider,
    entryFile,
    onProjectConfigWarnings: (w) => warnings.push(w),
  });
  await project.initialize();
  return { project, warnings };
}

describe("ProjectSession applies brink.toml (#2324)", () => {
  it("discovers and applies brink.toml during initialize(), before the caller gets control back", async () => {
    const { warnings } = await makeProject(
      {
        "brink.toml": '[project]\nconventions = "conventions.brink"\n',
        "story.ink": "-> END\n",
      },
      "story.ink",
    );
    // One call, zero warnings: every key here is recognized.
    expect(warnings).toEqual([[]]);
  });

  it("reports [project] entry as an unrecognized key — it has no real schema slot", async () => {
    const { warnings } = await makeProject(
      {
        "brink.toml": '[project]\nentry = "story.ink"\nconventions = "conventions.brink"\n',
        "story.ink": "-> END\n",
      },
      "story.ink",
    );
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toEqual([expect.stringContaining("entry")]);
  });

  it("re-applies when brink.toml is edited in the session (applyEdit path)", async () => {
    const { project, warnings } = await makeProject(
      { "brink.toml": '[project]\nconventions = "conventions.brink"\n', "story.ink": "-> END\n" },
      "story.ink",
    );
    expect(warnings).toEqual([[]]); // from initialize()

    project.applyEdit("brink.toml", '[project]\nbogus-key = true\n');
    expect(warnings).toHaveLength(2);
    expect(warnings[1]).toEqual([expect.stringContaining("bogus-key")]);
  });

  it("re-applies when brink.toml is created after mount (addFile path)", async () => {
    const { project, warnings } = await makeProject({ "story.ink": "-> END\n" }, "story.ink");
    expect(warnings).toEqual([[]]); // initialize(): no brink.toml yet, discovery finds nothing

    await project.addFile("brink.toml", '[project]\nunknown-thing = 1\n');
    expect(warnings).toHaveLength(2);
    expect(warnings[1]).toEqual([expect.stringContaining("unknown-thing")]);
  });

  it("does not reapply for an edit to an unrelated file", async () => {
    const { project, warnings } = await makeProject(
      {
        "brink.toml": '[project]\nconventions = "conventions.brink"\n',
        "story.ink": "-> END\n",
      },
      "story.ink",
    );
    expect(warnings).toEqual([[]]);

    project.applyEdit("story.ink", "-> DONE\n");
    expect(warnings).toEqual([[]]); // still just the one call, from initialize()
  });
});
