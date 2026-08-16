/**
 * Inline (F2) rename refusal reporting (#2543): a rename the op REFUSES must
 * not be reported to the user as a success.
 *
 * The inline rename commits through a different path than the modal prompt.
 * `SymbolRenamePrompt` goes through `performSymbolRename`, which grew an
 * `!result.ok` branch in #2528; the editor's F2 goes
 * editor → `commitRename` → `onRenameCommit` (`mount.tsx`) →
 * `applyComputedRename` → `applyMoveResult`, and NOTHING on that path looked at
 * `result.ok`. A refused rename therefore reached the apply seam, which pushed
 * an undo entry, raised the confirming **info** toast ("Rename X to Y") with an
 * Undo button, and re-keyed the symbol's open tab — for an edit that never
 * happened. Strictly worse than #2528's silent close, which at least did
 * nothing visible.
 *
 * Reachability: the widget only mounts when `prepareRename`
 * (`packages/ink-editor/src/rename.ts`) resolves a range at the cursor —
 * `brink_ide::rename::prepare_rename` returns one for a declaration or a
 * reference site, and those renames go on to *succeed*. A refusal that IS
 * reachable through a mounted widget is e.g. the file unloading out from under
 * an open rename ("file not loaded"), or the analysis⇄db identity-space
 * mismatch guarded by
 * `rename_refuses_rather_than_silently_dropping_edits_when_identity_spaces_disagree`
 * (`crates/internal/brink-ide/src/rename.rs`). Below, offset 0 (inside the
 * leading `-> hello` divert) is the mock's stand-in for "the op refuses" — it
 * is not itself a claim that F2 mounts the widget there; `renameSymbolAt` is
 * exercised directly, bypassing `prepareRename` entirely.
 *
 * Why the guard is on `ok` and not on `safe`:
 *
 *   Rust's `error_json` (`crates/brink-web/src/editor_refactor.rs`) serializes
 *   the whole `StructuralResultJs`, so a refusal ships `safe: true` with empty
 *   `introduced_diagnostics` — and `isSafeRename` (`breakage.ts`) reads exactly
 *   those two fields, so it calls a refusal "safe" and `settleCommit` commits
 *   it. That `safe: true` is a real lie, but `safe` is not the field that means
 *   "the operation happened" — `ok` is. Flipping `error_json` to `safe: false`
 *   would (a) change every structural op's payload at once, which is #2544's
 *   class-level sweep, and (b) not even fix this path: the inline report would
 *   render "⚠ breaks 0" with a Force button that commits the same refused
 *   result. The honest guard is `ok`, at the consumers that treat a result as
 *   applicable. The first assertion below pins the `safe: true` shape so the
 *   reason this bug existed stays visible.
 *
 * `packages/brink-studio/src/__tests__/inline-rename.test.ts`'s `mountRename`
 * harness proves the other half — that `settleCommit` really does hand a
 * refusal to `commitRename` unmodified when a widget IS mounted, with no
 * `prepareRename` bypass.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession, isSafeRename } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import {
  createStudioStore,
  type DocumentSessions as StoreDocs,
  type StoreNotification,
} from "@brink/studio-store";
import { applyComputedRename } from "@brink/studio-ui";
import type { StructuralResult } from "@brink/wasm-types";

const MAIN = "-> hello\n=== hello ===\nHi.\n-> END\n";

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

/** A store wired as `mount.tsx` wires it, plus captures of every notification
 *  raised and every symbol-tab re-key requested. */
async function makeStore(files: Record<string, string>) {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const store = createStudioStore();
  const rekeys: Array<[string, string, string]> = [];
  store.setState({ _project: project, _documents: stubDocuments() });
  store.getState().setDocSymbolRenamer((path, oldName, newName) => {
    rekeys.push([path, oldName, newName]);
  });
  const raised: StoreNotification[] = [];
  store.getState().setNotifier((n) => raised.push(n));
  return { store, project, raised, rekeys };
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("inline rename refusal (#2543)", () => {
  it("a refused rename is still reported as `safe` by the editor's gate", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });

    // Offset 0 (inside the leading `-> hello` divert) is a convenient stand-in
    // for "the op refuses" — it calls `renameSymbolAt` directly, bypassing
    // `prepareRename`, so it is not a claim that F2 reaches this offset in
    // production. See the file header for a refusal that IS reachable through
    // a mounted widget.
    const refused = project.getSession().renameSymbolAt("main.ink", 0, "greeting");

    expect(refused.ok).toBe(false);
    // The lie, pinned: `safe` says nothing about whether the op happened, so
    // the inline gate waves a refusal straight through to the commit path.
    expect(refused.safe).toBe(true);
    expect(refused.introduced_diagnostics).toEqual([]);
    expect(isSafeRename(refused)).toBe(true);
  });

  it("does not report a refused rename as a success", async () => {
    const { store, project, raised, rekeys } = await makeStore({ "main.ink": MAIN });
    const refused = project.getSession().renameSymbolAt("main.ink", 0, "greeting");
    const state = store.getState();

    await applyComputedRename(state, state.applyMoveResult, {
      path: "main.ink",
      currentName: "hello",
      newName: "greeting",
      result: refused,
    });

    // Nothing was renamed…
    expect(project.getSession().getFileSource("main.ink")).toBe(MAIN);
    // …so nothing may claim it was: no confirming info toast, no Undo entry
    // offering to undo an edit that never happened, and no re-keyed tab.
    expect(rekeys).toEqual([]);
    expect(store.getState().undoStack).toHaveLength(0);
    expect(raised.some((n) => n.severity === "info")).toBe(false);

    // The user must be told, on the same channel #2528 uses for the modal path.
    expect(raised).toHaveLength(1);
    expect(raised[0]!.severity).toBe("error");
    expect(raised[0]!.source).toBe("binder");
    expect(raised[0]!.message).toContain("hello");
    expect(raised[0]!.message).toContain("cannot rename this symbol");
  });

  it("still applies, toasts and re-keys when the rename succeeds", async () => {
    const { store, project, raised, rekeys } = await makeStore({ "main.ink": MAIN });
    // Offset 13 lands in the `=== hello ===` declaration name.
    const ok = project.getSession().renameSymbolAt("main.ink", 13, "greeting");
    expect(ok.ok).toBe(true);
    const state = store.getState();

    await applyComputedRename(state, state.applyMoveResult, {
      path: "main.ink",
      currentName: "hello",
      newName: "greeting",
      result: ok,
    });

    const src = project.getSession().getFileSource("main.ink")!;
    expect(src).toContain("=== greeting ===");
    expect(src).toContain("-> greeting");
    expect(rekeys).toEqual([["main.ink", "hello", "greeting"]]);
    expect(store.getState().undoStack).toHaveLength(1);
    expect(raised).toHaveLength(1);
    expect(raised[0]!.severity).toBe("info");
  });

  it("the apply seam itself refuses an `ok: false` result", async () => {
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });

    // A refusal reaching `applyMoveResult` from ANY caller must not become a
    // toast + undo entry. Per-op error reporting for the remaining structural
    // ops is #2544; this is only the backstop that keeps a refusal from
    // reading as a confirmation.
    const refused: StructuralResult = {
      ok: false,
      cross_file_edits: [],
      introduced_diagnostics: [],
      safe: true,
      error: "cannot rename this symbol",
    };
    await store.getState().applyMoveResult(refused, "Rename hello to greeting", ["main.ink"]);

    expect(project.getSession().getFileSource("main.ink")).toBe(MAIN);
    expect(store.getState().undoStack).toHaveLength(0);
    expect(raised).toEqual([]);
  });
});
