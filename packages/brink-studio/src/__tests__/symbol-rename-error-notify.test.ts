/**
 * Symbol-rename failure reporting (#2528): a knot/stitch rename that FAILS must
 * tell the user, not vanish.
 *
 * `performSymbolRename` returns `{ applied: false, error }` when the underlying
 * `rename_symbol` / `rename_symbol_at` op refuses — "symbol not found" after the
 * knot was edited away between opening the menu and confirming, "file not
 * loaded", "cannot rename this symbol". `SymbolRenamePrompt` closes on
 * `outcome.error` exactly as it closes on success, and nothing else in the tree
 * ever read `outcome.error`, so the string was dropped on the floor: the prompt
 * disappeared, nothing was renamed, and the user got no signal at all.
 *
 * The fix routes that error to the surface this codebase already uses for a
 * failed rename — an error-severity store notification tagged `binder`, the
 * same channel `applyRename` (studio-store's binder slice) uses when a *file*
 * rename throws, and the same `source` the success path's `applyMoveResult`
 * toast carries. The prompt is untouched; where a failed rename *reports* is
 * settled by that existing pattern, and whether the prompt should additionally
 * stay open is a separate UX question left to the maintainer.
 *
 * The first two tests fail without the notification — one per caller of the
 * error path (offset-based F2 and name-based context menu). The third is a
 * preservation guard: a successful rename must keep raising exactly its one
 * informational toast, so the fix cannot be satisfied by notifying on every
 * outcome.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import {
  createStudioStore,
  type DocumentSessions as StoreDocs,
  type StoreNotification,
} from "@brink/studio-store";
import { performSymbolRename } from "@brink/studio-ui";

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

/** A store wired exactly as `mount.tsx` wires it — `setNotifier` bridging slice
 *  notifications to the shell — plus a capture of everything raised. */
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

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("symbol rename failure reporting (#2528)", () => {
  it("reports an offset-based (F2) rename the op refuses", async () => {
    const MAIN = "-> hello\n=== hello ===\nHi.\n-> END\n";
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    // Offset 0 sits in the leading `-> hello` divert, not in any declaration
    // name, so the rename op refuses rather than computing edits.
    const outcome = await performSymbolRename(
      state,
      state.applyMoveResult,
      { path: "main.ink", offset: 0, currentName: "hello" },
      "greeting",
      false,
    );

    expect(outcome.applied).toBe(false);
    expect(outcome.error).toBe("cannot rename this symbol");
    // Nothing was renamed, so the only thing that can tell the user apart from
    // a success is the notification.
    expect(project.getSession().getFileSource("main.ink")).toBe(MAIN);

    expect(raised).toHaveLength(1);
    expect(raised[0]!.severity).toBe("error");
    expect(raised[0]!.source).toBe("binder");
    // The op's own reason must survive into the message, not just a generic
    // "rename failed" — that reason is the whole point of `outcome.error`.
    expect(raised[0]!.message).toContain("cannot rename this symbol");
    expect(raised[0]!.message).toContain("hello");
  });

  it("reports a name-based (context menu) rename against a file that is gone", async () => {
    const { store, raised } = await makeStore({ "main.ink": "=== hello ===\n-> END\n" });
    const state = store.getState();

    // The context menu carries names captured when it opened; the file can be
    // closed or deleted before the prompt is confirmed.
    const outcome = await performSymbolRename(
      state,
      state.applyMoveResult,
      { path: "vanished.ink", knot: "hello", currentName: "hello" },
      "greeting",
      false,
    );

    expect(outcome.applied).toBe(false);
    expect(outcome.error).toBe("file not loaded");

    expect(raised).toHaveLength(1);
    expect(raised[0]!.severity).toBe("error");
    expect(raised[0]!.message).toContain("file not loaded");
  });

  it("still raises only its one informational toast when the rename succeeds", async () => {
    const MAIN = "-> hello\n=== hello ===\nHi.\n-> END\n";
    const { store, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    const outcome = await performSymbolRename(
      state,
      state.applyMoveResult,
      { path: "main.ink", knot: "hello", currentName: "hello" },
      "greeting",
      false,
    );

    expect(outcome.applied).toBe(true);
    // Preservation guard: the failure channel stays silent on success, so the
    // fix cannot degenerate into notifying unconditionally.
    expect(raised).toHaveLength(1);
    expect(raised[0]!.severity).toBe("info");
    expect(raised.some((n) => n.severity === "error")).toBe(false);
  });
});
