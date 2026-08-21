/**
 * Issue #2918: `ProjectSession.renameFile`/`renameDir` compute the
 * safe-by-default breakage gate correctly (`safe` / `introduced_diagnostics`,
 * #316), but the Binder's `applyRename`/`applyDirRename` (`studio-store`'s
 * binder slice) used to discard that half of the result and apply
 * `cross_file_edits` unconditionally — a move that broke a reference applied
 * with no user-facing report at all.
 *
 * The studio's wasm mock (`__mocks__/brink-web.ts`) always reports a computed
 * rename/move as `safe: true` — "the mock has no analyzer" (see its own doc
 * comments on `rename_file`/`rename_dir`) — so a `safe: false` result can
 * only be exercised here with a hand-stubbed `ProjectSession`, the same
 * pattern `conflict-slice.test.ts` and `save-retire-invariant.test.ts` use
 * for a fake bound to `_project`. This is deliberate: the bug and the fix
 * both live entirely in how the store *consumes* `ProjectSession.renameFile`/
 * `renameDir`'s already-correct return value, never in the wasm op itself.
 *
 * Ships the NOTIFICATION FLOOR the issue sanctions as the minimum ("AT
 * MINIMUM a post-move notification when safe is false"): the move still
 * applies (same undo contract as a safe move) and a `warning`-severity
 * notification reports the breakage through the same `_notify` channel
 * PR #2916 used for the refused-move (`ok: false`) error notification. This
 * is NOT the fuller "will break N references" preflight/confirm pattern from
 * #324 — that pattern is real (`packages/ink-editor/src/breakage.ts`,
 * `rename.ts`'s inline-rename widget), but it lives on a dedicated CM6
 * popover for the in-editor symbol rename, with no analog for the Binder's
 * type-a-new-name-in-the-tree rename (which, unlike delete's `pendingDelete`,
 * has no confirm step to hang a preflight off of today). Building a new
 * confirm-dialog UX for the Binder is out of this fix's scope; see #2918.
 */

import { describe, it, expect, vi } from "vitest";
import {
  createStudioStore,
  type ProjectSession,
  type DocumentSessions as StoreDocs,
} from "@brink/studio-store";
import type { RenameDiagnostic } from "@brink/wasm-types";

const BREAKING_DIAGNOSTIC: RenameDiagnostic = {
  severity: "error",
  code: "E022",
  message: "unresolved divert target 'oldName'",
  path: "main.ink",
  line: 3,
  col: 1,
};

function stubDocuments(): StoreDocs {
  return { invalidateFile: vi.fn(), triggerCompile: vi.fn() } as unknown as StoreDocs;
}

/** A hand-stubbed `ProjectSession` (see the file doc for why) whose
 *  `renameFile`/`renameDir` return a caller-controlled result — the studio
 *  wasm mock can never produce `safe: false`. */
function stubProject(overrides: {
  renameFile?: ReturnType<typeof vi.fn>;
  renameDir?: ReturnType<typeof vi.fn>;
}) {
  return {
    renameFile: overrides.renameFile ?? vi.fn(),
    renameDir: overrides.renameDir ?? vi.fn(),
  } as unknown as ProjectSession;
}

describe("Binder file rename: a move that breaks a reference (#2918)", () => {
  it("the move still applies (undo entry pushed) AND a warning notification reports the breakage — RED against the pre-#2918 shape, which sent an unconditional info notification with no safe/introducedDiagnostics check at all", async () => {
    const renameFile = vi.fn(async () => ({
      referrers: ["main.ink"],
      safe: false,
      introducedDiagnostics: [BREAKING_DIAGNOSTIC],
    }));
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: stubProject({ renameFile }),
      _documents: stubDocuments(),
      _notify: notify,
    });

    await store.getState().renameFile("oldName.ink", "newName.ink");

    // The floor semantic (not a preflight gate): the op still ran.
    expect(renameFile).toHaveBeenCalledWith("oldName.ink", "newName.ink");
    // Undo interplay: a breaking move that still applies must still push a
    // real undo entry — no phantom, no missing entry.
    expect(store.getState().undoStack).toHaveLength(1);
    expect(store.getState().undoStack[0]!.kind).toBe("rename");

    expect(notify).toHaveBeenCalledOnce();
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({
        severity: "warning",
        source: "binder",
        message: expect.stringContaining("breaks 1 reference"),
        actions: [{ label: "Undo", commandId: "binder.undo" }],
      }),
    );
  });

  it("a safe move still reports the pre-existing info notification, unaffected by the fix", async () => {
    const renameFile = vi.fn(async () => ({
      referrers: [],
      safe: true,
      introducedDiagnostics: [],
    }));
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: stubProject({ renameFile }),
      _documents: stubDocuments(),
      _notify: notify,
    });

    await store.getState().renameFile("a.ink", "b.ink");

    expect(store.getState().undoStack).toHaveLength(1);
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ severity: "info", message: "Renamed a.ink → b.ink" }),
    );
  });
});

describe("Binder folder rename: a move that breaks a reference (#2918)", () => {
  it("the move still applies (undo entry pushed) AND a warning notification reports the breakage — same fix shape as renameFile", async () => {
    const renameDir = vi.fn(async () => ({
      moved: [{ oldPath: "chapters/a.ink", newPath: "acts/a.ink" }],
      referrers: ["main.ink"],
      safe: false,
      introducedDiagnostics: [BREAKING_DIAGNOSTIC, BREAKING_DIAGNOSTIC],
    }));
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: stubProject({ renameDir }),
      _documents: stubDocuments(),
      _notify: notify,
    });

    await store.getState().renameFolder("chapters/", "acts/", ["chapters/a.ink"]);

    expect(renameDir).toHaveBeenCalledWith("chapters/", "acts/");
    expect(store.getState().undoStack).toHaveLength(1);
    expect(store.getState().undoStack[0]!.kind).toBe("rename-dir");

    expect(notify).toHaveBeenCalledOnce();
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({
        severity: "warning",
        source: "binder",
        message: expect.stringContaining("breaks 2 references"),
        actions: [{ label: "Undo", commandId: "binder.undo" }],
      }),
    );
  });

  it("a safe folder move still reports the pre-existing info notification, unaffected by the fix", async () => {
    const renameDir = vi.fn(async () => ({
      moved: [{ oldPath: "chapters/a.ink", newPath: "acts/a.ink" }],
      referrers: [],
      safe: true,
      introducedDiagnostics: [],
    }));
    const notify = vi.fn();
    const store = createStudioStore();
    store.setState({
      _project: stubProject({ renameDir }),
      _documents: stubDocuments(),
      _notify: notify,
    });

    await store.getState().renameFolder("chapters/", "acts/", ["chapters/a.ink"]);

    expect(store.getState().undoStack).toHaveLength(1);
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ severity: "info", message: "Renamed chapters/ → acts/" }),
    );
  });
});
