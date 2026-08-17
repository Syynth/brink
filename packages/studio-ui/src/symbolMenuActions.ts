/**
 * The single dispatcher for knot/stitch context-menu actions — "play from
 * here" plus the structural refactors (reorder / move / promote / demote).
 * Pure (takes the store state + `applyMoveResult`), so the Binder (incl. its
 * drag-drop), the editor, and the Story Graph all drive identical logic.
 */

import type {
  FileOutline,
  StructuralResult,
  RenameDiagnostic,
} from "@brink/wasm-types";
import type { StudioState, SymbolRenameRequest } from "@brink/studio-store";
import { scheduleIdleWork } from "@brink-lang/editor";
import type { ContextMenuAction } from "./BinderContextMenu.js";

/** The declaration-name offset (whole-file UTF-16) of a knot or stitch in the
 *  project outline, or null when the symbol is not found. Used to seed the
 *  editor's inline rename from the context menu (which carries names, not an
 *  offset). */
function symbolDeclarationOffset(
  outline: FileOutline[],
  path: string,
  knot: string,
  stitch?: string,
): number | null {
  const file = outline.find((f) => f.path === path);
  if (!file) return null;
  const knotSym = file.symbols.find((s) => s.kind === "knot" && s.name === knot);
  if (!knotSym) return null;
  if (stitch === undefined) return knotSym.start;
  const stitchSym = knotSym.children.find((c) => c.kind === "stitch" && c.name === stitch);
  return stitchSym ? stitchSym.start : null;
}

/**
 * Off the paint path (#722/#2761's remedy, generalized here for #2767):
 * `moveStitch` / `promoteStitch` / `demoteKnot` are the three
 * `dispatchSymbolAction` branches whose Rust op runs the full op-agnostic
 * breakage gate (`gated_move_json` → `structural_result::gate_with_source`,
 * `crates/brink-web/src/editor_refactor.rs` / `crates/internal/brink-ide/src/
 * structural_result.rs`) — an overlay re-analysis of the whole project, the
 * same cost class as the rename collision check #722 and #2761 already
 * cover. Called synchronously and inline from a React event handler (the
 * context-menu click, the drag-drop `onDrop`) with no yield point, it can
 * block the main thread — and therefore paint — under load exactly like the
 * two prior incidents.
 *
 * The four `reorder*` branches deliberately do NOT go through this: they
 * change no qualification, so the pure op returns `StructuralResult::
 * safe_source` and skips the gate entirely (see that type's doc comment) —
 * genuinely cheap, not merely assumed so. Wrapping them here would add an
 * idle-hop and a pending-indicator flash for zero benefit.
 *
 * This commits `structuralOpPending` synchronously (so React can paint a
 * pending indicator before the heavy call runs) and defers `compute` to the
 * next idle slot via `scheduleIdleWork`, clearing it again once `compute`
 * returns. The pending state is a LOCAL busy-state affordance rendered by
 * the status bar's `StructuralOpSegment` (spec §7.3) — deliberately NOT a
 * shell notification (`state._notify`): §7.5 states progress notifications
 * are out of scope for the notification service, and a review of #2769
 * caught an earlier version of this helper raising one anyway (it also
 * double-toasted on success, since `applyMoveResult` already raises its own
 * "Move X to Y" notification with Undo).
 *
 * There is deliberately no re-check of `session.generation` (a counter every
 * content-mutating wasm call bumps, including a single keystroke in a
 * mounted editor view via `pushSource`/`updateDocument` —
 * `packages/wasm/src/index.ts`) against a value captured before scheduling.
 * An earlier version of this helper did that, on the theory it mirrored
 * #2761's staleness guard for the rename prompt — but unlike that prompt,
 * `compute` here is a thunk that calls the wasm op fresh at invocation time
 * against the session's THEN-current source, never against a snapshot
 * captured before the idle wait; and the op itself already refuses cleanly
 * when its target has moved out from under it (`error_json`, e.g. "source
 * knot not found"). A blanket generation check guards no hazard that exists
 * on this path — it only silently drops legitimate queued ops on completely
 * unrelated edits, including the routine one-keystroke-per-transaction
 * bumps every mounted editor view produces. Trust the op's own `result.ok`
 * refusal instead; see `dispatchSymbolAction`'s `case`s below.
 *
 * There is no widget instance to `cancelIdleWork` on unmount/close the way
 * `InlineNameInput`/`SymbolRenamePrompt` do — a context-menu click and a
 * drag-drop drop are one-shot, not an open editing surface — so there is no
 * staleness guard to run here at all; the op's own refusal is sufficient.
 *
 * Clears via `clearStructuralOpPending(description)`, not
 * `setStructuralOpPending(null)` (issue #2794): `structuralOpPending` has a
 * second writer (`applyRename` in `binder.ts`'s Binder rename/move), and
 * both are independent fire-and-forget `void` dispatches that can overlap —
 * an unconditional clear here could erase a Binder rename's still-live
 * indicator if this op happens to settle after that one started. The
 * compare-and-clear only removes the description THIS call set.
 */
async function runGatedStructuralOp(
  state: StudioState,
  description: string,
  compute: () => StructuralResult,
): Promise<StructuralResult> {
  state.setStructuralOpPending(description);
  try {
    await new Promise<void>((resolve) => {
      scheduleIdleWork(resolve);
    });
    return compute();
  } finally {
    state.clearStructuralOpPending(description);
  }
}

export async function dispatchSymbolAction(
  state: StudioState,
  applyMoveResult: StudioState["applyMoveResult"],
  action: ContextMenuAction,
): Promise<void> {
  if (action.type === "playFromHere") {
    state.openSession({ path: action.inkPath, label: action.label });
    return;
  }

  // Rename. Editor-origin renames run inline in the editor (#323/#324): resolve
  // the symbol's declaration offset from the outline and start the inline widget
  // in the mounted view. Graph-origin (and any case with no mounted editor view)
  // falls back to the modal prompt, which drives `performSymbolRename` (#305).
  if (action.type === "renameSymbol") {
    if (action.source === "editor") {
      const offset = symbolDeclarationOffset(
        state.outline,
        action.path,
        action.knot,
        action.stitch,
      );
      if (offset !== null && state._documents?.startInlineRenameAt(action.path, offset)) {
        return;
      }
    }
    state.openRenamePrompt({
      path: action.path,
      knot: action.knot,
      stitch: action.stitch,
      currentName: action.stitch ?? action.knot,
    });
    return;
  }

  const session = state._project?.getSession();
  if (!session) return;

  let result: StructuralResult;
  let description: string;
  switch (action.type) {
    case "reorderStitch":
      result = session.reorderStitch(action.path, action.knot, action.stitch, action.direction);
      description = `Reorder ${action.stitch} ${action.direction > 0 ? "down" : "up"}`;
      break;
    case "reorderKnot":
      result = session.reorderKnot(action.path, action.knot, action.direction);
      description = `Reorder ${action.knot} ${action.direction > 0 ? "down" : "up"}`;
      break;
    case "reorderStitches":
      result = session.reorderStitches(action.path, action.knot, action.order);
      description = `Reorder stitches in ${action.knot}`;
      break;
    case "reorderKnots":
      result = session.reorderKnots(action.path, action.order);
      description = `Reorder knots`;
      break;
    case "moveStitch": {
      description = `Move ${action.stitch} to ${action.destKnot}`;
      result = await runGatedStructuralOp(state, description, () =>
        // PAINT-PATH-DEFERRED move-stitch: gated (structural_result::gate_with_source)
        // — run off the paint path by runGatedStructuralOp above (#2767).
        session.moveStitch(action.path, action.srcKnot, action.stitch, action.destKnot),
      );
      break;
    }
    case "promoteStitch": {
      description = `Promote ${action.stitch} to knot`;
      result = await runGatedStructuralOp(state, description, () =>
        // PAINT-PATH-DEFERRED promote-stitch: gated (structural_result::gate_with_source)
        // — run off the paint path by runGatedStructuralOp above (#2767).
        session.promoteStitch(action.path, action.knot, action.stitch),
      );
      break;
    }
    case "demoteKnot": {
      description = `Demote ${action.knot} into ${action.destKnot}`;
      result = await runGatedStructuralOp(state, description, () =>
        // PAINT-PATH-DEFERRED demote-knot: gated (structural_result::gate_with_source)
        // — run off the paint path by runGatedStructuralOp above (#2767).
        session.demoteKnot(action.path, action.knot, action.destKnot),
      );
      break;
    }
    default:
      // File/folder actions are owned by the Binder and never reach here.
      return;
  }

  if (result.ok && result.path) {
    await applyMoveResult(result, description, [result.path]);
  }
}

/** How a knot/stitch names itself in a rename message — the symbol's current
 *  name when the request carries one, else the qualified `knot.stitch`. Shared
 *  by the success description and the failure notification so both halves of
 *  one rename name the same thing. */
function renameLabel(req: SymbolRenameRequest): string {
  return req.currentName ?? (req.stitch ? `${req.knot}.${req.stitch}` : req.knot) ?? "symbol";
}

/**
 * Report a refused rename through the shell's notification service —
 * error-severity, `binder`-sourced, "Rename X failed: …". Shared by both
 * rename surfaces (`performSymbolRename`'s modal path, #2528, and
 * `applyComputedRename`'s inline/F2 path, #2543) so the two cannot drift:
 * same severity, same source, same frame.
 *
 * The frame is "Rename X failed: <reason>", NOT "Cannot rename X: <reason>":
 * the op's most common refusal is literally "cannot rename this symbol",
 * which the latter turns into "Cannot rename hello: cannot rename this
 * symbol". Keep the frame and the op's own wording from colliding.
 */
function notifyRenameRefusal(state: StudioState, label: string, error: string | undefined): void {
  state._notify?.({
    severity: "error",
    source: "binder",
    message:
      error != null && error !== "" ? `Rename ${label} failed: ${error}` : `Rename ${label} failed`,
  });
}

/** The outcome of attempting a symbol rename. */
export interface SymbolRenameOutcome {
  /** True when the edits were applied (safe, or forced). */
  applied: boolean;
  /** The diagnostics the rename would introduce (the breakage report). */
  diagnostics: RenameDiagnostic[];
  /** An error from the rename op (symbol vanished, etc.), if any. Already
   *  reported to the user as an error notification before it is returned
   *  (#2528) — callers use it to decide control flow, not to surface it. */
  error?: string;
}

/**
 * Run a knot/stitch rename, safe-by-default (#305). Computes the rename and its
 * introduced-diagnostic breakage report; applies the edits when the rename is
 * safe or `force` is set, otherwise returns the report so the prompt can show
 * it. Used by `SymbolRenamePrompt`.
 *
 * A refused rename raises an error notification through the store's injected
 * notifier before returning (#2528); see the failure branch below.
 */
export async function performSymbolRename(
  state: StudioState,
  applyMoveResult: StudioState["applyMoveResult"],
  req: SymbolRenameRequest,
  newName: string,
  force: boolean,
): Promise<SymbolRenameOutcome> {
  const session = state._project?.getSession();
  if (!session) return { applied: false, diagnostics: [] };

  // Offset-based (F2) covers any symbol under the cursor; name-based (menu)
  // targets a knot/stitch. Both return the same safe-rename payload.
  const result =
    req.offset != null
      ? session.renameSymbolAt(req.path, req.offset, newName)
      : session.renameSymbol(req.path, req.knot ?? "", req.stitch ?? "", newName);
  if (!result.ok) {
    // Report the failure before returning it (#2528). `SymbolRenamePrompt`
    // closes on `outcome.error` exactly as it closes on success, so without a
    // notification here a rename that failed — "symbol not found" after the
    // knot was edited away, "file not loaded", "cannot rename this symbol" —
    // looks identical to one that worked: the prompt vanishes and nothing is
    // renamed. This is the same surface the *file* rename's failure path uses
    // (`applyRename` in studio-store's binder slice), and the same `source`
    // tag the success path's `applyMoveResult` toast carries, so both outcomes
    // of one rename report through one channel.
    //
    // Guarded by packages/brink-studio/src/__tests__/symbol-rename-error-notify.test.ts;
    // the invariant is recorded in docs/studio-shell-spec.md §7.5.
    notifyRenameRefusal(state, renameLabel(req), result.error);
    return { applied: false, diagnostics: [], error: result.error };
  }

  if (result.safe || force) {
    const oldName = req.currentName ?? req.stitch ?? req.knot;
    const label = renameLabel(req);
    await applyMoveResult(
      result,
      `Rename ${label} to ${newName}`,
      result.path ? [result.path] : [],
    );
    // Keep an open symbol view of the renamed knot/stitch aligned: re-key its
    // `path::oldName` tab to `path::newName` in place (#305).
    if (oldName !== undefined && oldName !== newName) {
      state.renameSymbolDocKey(req.path, oldName, newName);
    }
    return { applied: true, diagnostics: result.introduced_diagnostics };
  }

  return { applied: false, diagnostics: result.introduced_diagnostics };
}

/**
 * Apply an already-computed `StructuralResult` — the commit path for the
 * editor's inline rename (#323/#324). The inline badge computed `result` live;
 * here we apply its cross-file edits through `applyMoveResult` (one undoable
 * step) and re-key any open symbol tab from `currentName` to `newName`, exactly
 * like `performSymbolRename`'s apply branch but skipping the re-query.
 */
export async function applyComputedRename(
  state: StudioState,
  applyMoveResult: StudioState["applyMoveResult"],
  args: { path: string; currentName: string; newName: string; result: StructuralResult },
): Promise<void> {
  const { path, currentName, newName, result } = args;
  if (!result.ok) {
    // The op refused — there is nothing to apply (#2543). Without this branch
    // the refusal flowed into `applyMoveResult` and came back out as the
    // success toast ("Rename X to Y", with Undo) plus a re-keyed symbol tab,
    // asserting an edit that never happened.
    //
    // `isSafeRename` cannot catch this upstream: a refusal carries
    // `safe: true` with no introduced diagnostics (Rust's `error_json`), so
    // the editor's inline gate reads it as safe and commits. `ok` is the field
    // that says whether the operation happened; `safe` only ever described the
    // breakage of edits that were actually computed.
    //
    // Guarded by packages/brink-studio/src/__tests__/inline-rename-refusal.test.ts.
    notifyRenameRefusal(state, currentName, result.error);
    return;
  }
  await applyMoveResult(
    result,
    `Rename ${currentName} to ${newName}`,
    result.path ? [result.path] : [],
  );
  if (currentName !== newName) {
    state.renameSymbolDocKey(path, currentName, newName);
  }
}
