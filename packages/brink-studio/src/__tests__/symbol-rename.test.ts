/**
 * Knot/stitch safe-rename tests (#305): `performSymbolRename` drives the
 * studio's safe-by-default flow — apply directly when the rename introduces no
 * diagnostics, otherwise return the breakage report and apply only on force.
 *
 * Runs against the brink-web mock, whose `rename_symbol` rewrites the symbol's
 * header + diverts and flags an `E022` collision when a knot is renamed onto an
 * existing top-level knot. The real rename + diagnostic-diff math is covered by
 * Rust unit tests in brink-ide / brink-web.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink/ink-editor";
import { initWasm } from "@brink-lang/web";
import { createStudioStore, type DocumentSessions as StoreDocs } from "@brink/studio-store";
import { performSymbolRename } from "@brink/studio-ui";

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

async function makeStore(files: Record<string, string>) {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const store = createStudioStore();
  store.setState({ _project: project, _documents: stubDocuments() });
  return { store, project };
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("performSymbolRename", () => {
  it("applies a safe rename and rewrites references", async () => {
    const MAIN = "-> hello\n=== hello ===\nHi.\n-> END\n";
    const { store, project } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    const outcome = await performSymbolRename(
      state,
      state.applyMoveResult,
      { path: "main.ink", knot: "hello" },
      "greeting",
      false,
    );

    expect(outcome.applied).toBe(true);
    expect(outcome.diagnostics).toHaveLength(0);
    const src = project.getSession().getFileSource("main.ink")!;
    expect(src).toContain("=== greeting ===");
    expect(src).toContain("-> greeting");
  });

  it("blocks an unsafe rename, returns the breakage report, and force applies", async () => {
    const TWO = "-> a\n=== a ===\n-> END\n=== b ===\n-> END\n";
    const { store, project } = await makeStore({ "main.ink": TWO });
    const state = store.getState();
    const req = { path: "main.ink", knot: "a" };

    // Safe-by-default: the collision blocks the rename and reports breakage.
    const blocked = await performSymbolRename(state, state.applyMoveResult, req, "b", false);
    expect(blocked.applied).toBe(false);
    expect(blocked.diagnostics.some((d) => d.code === "E022")).toBe(true);
    expect(project.getSession().getFileSource("main.ink")).toBe(TWO); // untouched

    // Force overrides — the (already-computed) edits apply despite the breakage.
    const forced = await performSymbolRename(state, state.applyMoveResult, req, "b", true);
    expect(forced.applied).toBe(true);
    expect(project.getSession().getFileSource("main.ink")).not.toBe(TWO);
  });
});
