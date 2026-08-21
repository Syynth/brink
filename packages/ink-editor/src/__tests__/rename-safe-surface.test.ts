/**
 * Issue #2918: `ProjectSession.renameFile`/`renameDir` used to discard the
 * wasm op's `safe`/`introduced_diagnostics` breakage-gate verdict — the
 * session's `rename_file`/`rename_dir` compute it correctly (`StructuralResult`/
 * `DirMoveResult`, #316), but the pre-#2918 return values were a bare
 * `string[]` (referrers) / `{ moved, referrers }`, with no way for ANY
 * caller — including a caller with no interest in the Binder's undo/notify
 * plumbing — to learn a move broke a reference.
 *
 * Uses the hand-built stub `session` pattern from
 * `project-session-destroy.test.ts` (not a real `EditorSessionHandle`) so
 * this needs no built `crates/brink-web/www/pkg` — the fix under test is
 * `ProjectSession`'s own return-value plumbing, not the wasm op.
 */

import { describe, it, expect, vi } from "vitest";
import { ProjectSession } from "../project-session.js";
import { InMemoryFileProvider } from "../provider.js";

function makeStubSession(overrides: {
  renameFile?: ReturnType<typeof vi.fn>;
  renameDir?: ReturnType<typeof vi.fn>;
}) {
  return {
    generation: 0,
    updateFile: vi.fn(),
    removeFile: vi.fn(),
    getFileSource: vi.fn(() => null),
    discoverProjectConfig: vi.fn(() => []),
    getFileIncludes: vi.fn(() => []),
    listFiles: vi.fn(() => []),
    isReadOnly: vi.fn(() => false),
    renameFile:
      overrides.renameFile ??
      vi.fn(() => ({
        ok: true,
        new_source: "content",
        cross_file_edits: [],
        safe: true,
        introduced_diagnostics: [],
      })),
    renameDir:
      overrides.renameDir ??
      vi.fn(() => ({
        ok: true,
        moved_files: [],
        cross_file_edits: [],
        safe: true,
        introduced_diagnostics: [],
      })),
    compileProject: vi.fn(),
    free: vi.fn(),
  };
}

type StubSession = ReturnType<typeof makeStubSession>;

function makeProjectSession(session: StubSession) {
  const provider = new InMemoryFileProvider({ "main.ink": "-> END\n", "lib.ink": "-> END\n" });
  const project = new ProjectSession({
    provider,
    entryFile: "main.ink",
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
    session: session as any,
  });
  return { project, provider };
}

describe("ProjectSession.renameFile surfaces safe/introducedDiagnostics (#2918)", () => {
  it("a move the wasm op reports safe: false resolves with safe: false and the diagnostics, not just the referrer list", async () => {
    const diagnostic = {
      severity: "error" as const,
      code: "E022",
      message: "unresolved divert target",
      path: "main.ink",
      line: 1,
      col: 1,
    };
    const session = makeStubSession({
      renameFile: vi.fn(() => ({
        ok: true,
        new_source: "content",
        cross_file_edits: [{ path: "main.ink", new_source: "rewritten" }],
        safe: false,
        introduced_diagnostics: [diagnostic],
      })),
    });
    const { project } = makeProjectSession(session);

    const result = await project.renameFile("lib.ink", "util.ink");

    // RED against the pre-#2918 shape: that version resolved with a bare
    // `["main.ink"]` array — `.safe`/`.introducedDiagnostics` did not exist
    // on the resolved value at all, so any caller inspecting them today
    // would have seen `undefined`, never `false`/the diagnostic.
    expect(result).toEqual({
      referrers: ["main.ink"],
      safe: false,
      introducedDiagnostics: [diagnostic],
    });
  });

  it("a safe move resolves with safe: true and an empty diagnostics list", async () => {
    const session = makeStubSession({
      renameFile: vi.fn(() => ({
        ok: true,
        new_source: "content",
        cross_file_edits: [],
        safe: true,
        introduced_diagnostics: [],
      })),
    });
    const { project } = makeProjectSession(session);

    const result = await project.renameFile("lib.ink", "util.ink");

    expect(result).toEqual({ referrers: [], safe: true, introducedDiagnostics: [] });
  });
});

describe("ProjectSession.renameDir surfaces safe/introducedDiagnostics (#2918)", () => {
  it("a folder move the wasm op reports safe: false resolves with safe: false and the diagnostics", async () => {
    const diagnostic = {
      severity: "error" as const,
      code: "E022",
      message: "unresolved divert target",
      path: "main.ink",
      line: 1,
      col: 1,
    };
    const session = makeStubSession({
      renameDir: vi.fn(() => ({
        ok: true,
        moved_files: [
          { old_path: "chapters/a.ink", new_path: "acts/a.ink", new_source: "content" },
        ],
        cross_file_edits: [{ path: "main.ink", new_source: "rewritten" }],
        safe: false,
        introduced_diagnostics: [diagnostic],
      })),
    });
    const { project } = makeProjectSession(session);

    const result = await project.renameDir("chapters", "acts");

    expect(result).toEqual({
      moved: [{ oldPath: "chapters/a.ink", newPath: "acts/a.ink" }],
      referrers: ["main.ink"],
      safe: false,
      introducedDiagnostics: [diagnostic],
    });
  });
});
