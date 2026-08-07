/**
 * Session-level read-only enforcement for a mounted stdlib file (issue
 * #2306, ruled 2026-08-06 "Mounted stdlib presents as a read-only library
 * node"). The ruling's part (3), built first per its own sequencing note:
 * a by-id route (project-wide search/replace, the editable results-buffer
 * row edit, or any future bulk caller not gated by `listFiles`) must not be
 * able to silently fork a mounted stdlib copy into the project — closed at
 * the session/API layer (`ProjectSession.applyEdit`, consulting
 * `EditorSessionHandle.isReadOnly`), not in UI code.
 *
 * Runs against the brink-web mock (src/__mocks__/brink-web.ts), whose
 * `__mockMarkReadOnlyForTest` seam marks a path read-only the same way the
 * real `EditorSession::new()` marks every `brink_environment::stdlib_sources()`
 * key on construction (proven for real in the Rust suite,
 * `crates/brink-web/src/editor/mod.rs`) — this file pins the TS wiring on
 * top of that primitive.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { EditorSessionHandle, initWasm } from "@brink-lang/web";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";

const MOUNTED_PATH = "std/core.brink";
const MOUNTED_TEXT = "=== core ===\n-> DONE\n";
const MAIN_INK = "-> DONE\n";

/** Reach into the mock's raw session to seed a read-only file — the
 *  wasm-boundary equivalent of the real constructor's stdlib mount (see the
 *  module doc). `EditorSessionHandle.session` is intentionally private on
 *  the production type; the cast is a test-only seam. */
function markReadOnly(handle: EditorSessionHandle, path: string, source: string): void {
  (
    handle as unknown as {
      session: { __mockMarkReadOnlyForTest(path: string, source: string): void };
    }
  ).session.__mockMarkReadOnlyForTest(path, source);
}

function stubDocuments(): StoreDocs {
  return { invalidateFile: () => {}, triggerCompile: () => {} } as unknown as StoreDocs;
}

async function makeProject(): Promise<{ session: EditorSessionHandle; project: ProjectSession }> {
  await initWasm();
  const session = new EditorSessionHandle();
  markReadOnly(session, MOUNTED_PATH, MOUNTED_TEXT);
  const provider = new InMemoryFileProvider({ "main.ink": MAIN_INK });
  const project = new ProjectSession({ provider, entryFile: "main.ink", session });
  await project.initialize();
  return { session, project };
}

beforeEach(async () => {
  await initWasm();
});

describe("EditorSessionHandle.isReadOnly", () => {
  it("is true for a mounted path and false for an ordinary one", async () => {
    const { session } = await makeProject();
    expect(session.isReadOnly(MOUNTED_PATH)).toBe(true);
    expect(session.isReadOnly("main.ink")).toBe(false);
    expect(session.isReadOnly("nonexistent.ink")).toBe(false);
  });
});

describe("ProjectSession.applyEdit read-only refusal (issue #2306)", () => {
  it("refuses a direct write to a mounted path: returns false, content unchanged, no host egress", async () => {
    const { session, project } = await makeProject();
    const applied = project.applyEdit(MOUNTED_PATH, "FORKED\n");
    expect(applied).toBe(false);
    expect(session.getFileSource(MOUNTED_PATH)).toBe(MOUNTED_TEXT);
  });

  it("still allows an ordinary file to be written (the guard is scoped to mounted paths)", async () => {
    const { session, project } = await makeProject();
    const applied = project.applyEdit("main.ink", "-> END\n");
    expect(applied).toBe(true);
    expect(session.getFileSource("main.ink")).toBe("-> END\n");
  });
});

describe("Search-replace store actions cannot fork a mounted file (issue #2306)", () => {
  it("applySearchRowEdit on a mounted path is refused: content unchanged, a read-only notification fires", async () => {
    const { session, project } = await makeProject();
    const store = createStudioStore();
    const notifications: unknown[] = [];
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _notify: (n) => notifications.push(n),
    });

    // The exact by-id shape #2306 names: a row edit against a path that
    // bypasses `runSearch`'s explicit mounted-file exclusion (the search
    // slice's candidate list, not `listFiles()`, is what filters mounts out
    // now) — e.g. a stale results-buffer row, or any future caller that
    // resolves a file by id rather than by running a fresh search.
    store.getState().applySearchRowEdit(MOUNTED_PATH, { start: 0, end: 4, text: "XXXX" });

    expect(session.getFileSource(MOUNTED_PATH)).toBe(MOUNTED_TEXT);
    expect(notifications).toEqual([
      expect.objectContaining({ severity: "warning", source: "search" }),
    ]);
  });

  it("replaceSearchMatch on a mounted path is refused: content unchanged", async () => {
    const { session, project } = await makeProject();
    const store = createStudioStore();
    store.setState({
      _project: project,
      _documents: stubDocuments(),
      _notify: () => {},
      searchQuery: "core",
      searchReplace: "REPLACED",
    });

    store.getState().replaceSearchMatch(MOUNTED_PATH, {
      start: 4,
      end: 8,
      line: 1,
      lineText: "=== core ===",
      lineStart: 4,
      lineEnd: 8,
      text: "core",
    });

    expect(session.getFileSource(MOUNTED_PATH)).toBe(MOUNTED_TEXT);
  });

  it(
    "a mounted path is listed but flagged mounted:true (issue #2306/#2343 " +
      "listed-but-marked flip), and project-wide search still never treats " +
      "it as a candidate even though its content would otherwise match",
    async () => {
      const { project } = await makeProject();
      const files = project.getSession().listFiles();
      const mountedEntry = files.find((f) => f.path === MOUNTED_PATH);
      expect(mountedEntry?.mounted).toBe(true);
      const mainEntry = files.find((f) => f.path === "main.ink");
      expect(mainEntry?.mounted).toBe(false);

      const store = createStudioStore();
      store.setState({ _project: project, _documents: stubDocuments(), _notify: () => {} });
      store.setState({ searchQuery: "core" }); // MOUNTED_TEXT contains "core" — would match if not excluded.
      store.getState().runSearch();
      const resultPaths = store.getState().searchResults?.files.map((f) => f.path) ?? [];
      expect(resultPaths).not.toContain(MOUNTED_PATH);
    },
  );
});
