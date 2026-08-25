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
import type { ContextMenuAction } from "./BinderContextMenu.js";

/**
 * The result {@link runGatedStructuralOp} reports when `ProjectSession.
 * destroy()` raced its `deferGatedCall()` yield (issue #2794 review, Gap 1
 * follow-up): the op never ran — and never could, since the wasm `session`
 * handle `compute` closes over is already freed by the time the destroy
 * rejection lands. Shaped exactly like a genuine no-op refusal (`ok: false`,
 * `safe: true`, no diagnostics — see `StructuralResult.ok`'s doc comment on
 * why that pairing means "refused", not "succeeded vacuously"), so
 * `dispatchSymbolAction`'s `result.ok` check below skips `applyMoveResult`
 * the same way it does for any other refusal. It is also identity-checked
 * (`result === DESTROYED_DURING_DEFER_RESULT`) against `notifyStructuralRefusal`
 * (#2544): this shape is a user-initiated cancel (the project was closed or
 * switched out from under a pending op), not a refusal the op itself made, so
 * it must not surface as an error notification either — only a genuine
 * `ok: false` from `compute()` does.
 */
const DESTROYED_DURING_DEFER_RESULT: StructuralResult = {
  ok: false,
  safe: true,
  cross_file_edits: [],
  introduced_diagnostics: [],
};

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
 * next idle slot via `ProjectSession.deferGatedCall()` (`@brink-lang/editor`
 * — issue #2794 review; previously a bare `scheduleIdleWork` await rolled
 * here, see below), clearing the pending indicator again once `compute`
 * settles. The pending state is a LOCAL busy-state affordance rendered by
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
 * drag-drop drop are one-shot, not an open editing surface. That is about
 * component teardown, though, not `ProjectSession` teardown: issue #2794's
 * follow-up review found that an unmount within the idle window destroys the
 * session out from under this call's own deferred `compute()` the same way
 * it once could for `renameFile` — a real hazard `deferGatedCall` closes
 * (see below), not a staleness check this call gets to skip.
 *
 * `deferGatedCall`'s rejection (session destroyed mid-defer) is caught and
 * swallowed here into {@link DESTROYED_DURING_DEFER_RESULT} rather than left
 * to propagate: every caller of `dispatchSymbolAction` invokes it as a
 * fire-and-forget `void dispatchSymbolAction(...)` (`useSymbolMenuActions.ts`,
 * `Binder.tsx`), so an uncaught rejection here would surface as an unhandled
 * promise rejection with nothing to catch it, not a caught error a UI layer
 * reports. `compute()` itself is deliberately left outside this catch — a
 * refusal from the op itself is reported through `result.ok`, same as ever;
 * only the destroy race short-circuits before `compute()` would run at all.
 *
 * Clears via `clearStructuralOpPending(description)`, never by calling
 * `setStructuralOpPending` again with a null-ish value — `structuralOpPending`
 * has a second writer (`applyRename` in `binder.ts`'s Binder rename/move), and
 * both are independent fire-and-forget `void` dispatches that can overlap —
 * an unconditional clear here could erase a Binder rename's still-live
 * indicator if this op happens to settle after that one started. The
 * compare-and-clear only removes the description THIS call set.
 */
async function runGatedStructuralOp(
  state: StudioState,
  description: string,
  compute: () => StructuralResult | Promise<StructuralResult>,
): Promise<StructuralResult> {
  state.setStructuralOpPending(description);
  try {
    try {
      await state._project?.deferGatedCall();
    } catch {
      // ProjectSession.destroy() landed while this call was deferred (issue
      // #2794 review) — the session (and the wasm handle `compute` closes
      // over) is already freed. Swallow rather than rethrow; see the doc
      // comment above for why this must not reach `compute()` or propagate.
      return DESTROYED_DURING_DEFER_RESULT;
    }
    try {
      return await compute();
    } catch (error) {
      // W2e: an async compute rides the session facade; destroy() while it
      // is queued rejects it as cancelled — the same freed-session race the
      // catch above handles for the defer yield. Checked by name rather
      // than instanceof to avoid a runtime dependency on the editor
      // package from this UI package.
      if (error instanceof Error && error.name === "QueryDroppedError") {
        return DESTROYED_DURING_DEFER_RESULT;
      }
      throw error;
    }
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
      if (
        offset !== null &&
        (await state._documents?.startInlineRenameAt(action.path, offset)) === true
      ) {
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

  const project = state._project;
  const session = project?.getSession();
  if (!project || !session) return;

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
        // — run off the paint path by runGatedStructuralOp above (#2767);
        // rides the async session facade at interactive priority (W2e).
        project.structuralQuery<StructuralResult>("moveStitch", [
          action.path,
          action.srcKnot,
          action.stitch,
          action.destKnot,
        ]),
      );
      break;
    }
    case "promoteStitch": {
      description = `Promote ${action.stitch} to knot`;
      result = await runGatedStructuralOp(state, description, () =>
        // PAINT-PATH-DEFERRED promote-stitch: gated (structural_result::gate_with_source)
        // — run off the paint path by runGatedStructuralOp above (#2767);
        // rides the async session facade at interactive priority (W2e).
        project.structuralQuery<StructuralResult>("promoteStitch", [
          action.path,
          action.knot,
          action.stitch,
        ]),
      );
      break;
    }
    case "demoteKnot": {
      description = `Demote ${action.knot} into ${action.destKnot}`;
      result = await runGatedStructuralOp(state, description, () =>
        // PAINT-PATH-DEFERRED demote-knot: gated (structural_result::gate_with_source)
        // — run off the paint path by runGatedStructuralOp above (#2767);
        // rides the async session facade at interactive priority (W2e).
        project.structuralQuery<StructuralResult>("demoteKnot", [
          action.path,
          action.knot,
          action.destKnot,
        ]),
      );
      break;
    }
    default:
      // File/folder actions are owned by the Binder and never reach here.
      return;
  }

  if (result.ok) {
    if (result.path) {
      await applyMoveResult(result, description, [result.path]);
    }
    return;
  }
  if (result === DESTROYED_DURING_DEFER_RESULT) {
    // The session was torn down mid-defer (issue #2794 review) — the user
    // cancelled by closing/switching the project, not by hitting a real
    // refusal. Nothing was attempted, so nothing to report: nothing pending
    // is left uncleared either (`runGatedStructuralOp`'s `finally` already
    // cleared it before returning this sentinel).
    return;
  }
  // Refused (`ok: false`) — report it through the same channel every other
  // structural-op refusal on this surface uses (#2544). Before this branch
  // existed, all seven cases above (reorder/move/promote/demote) fell
  // straight through with no `else`: `applyMoveResult` never ran (correctly
  // — nothing was written), but nothing told the user why. `applyMoveResult`
  // itself already refuses a passed-in `!result.ok` at its own seam (binder.ts,
  // #2543) for exactly this reason — it has no idea what was attempted, only
  // this dispatcher does.
  notifyStructuralRefusal(state, description, result.error);
}

/** How a knot/stitch names itself in a rename message — the symbol's current
 *  name when the request carries one, else the qualified `knot.stitch`. Shared
 *  by the success description and the failure notification so both halves of
 *  one rename name the same thing. */
function renameLabel(req: SymbolRenameRequest): string {
  return req.currentName ?? (req.stitch ? `${req.knot}.${req.stitch}` : req.knot) ?? "symbol";
}

/**
 * Report a refused structural op through the shell's notification service —
 * error-severity, `binder`-sourced, "<description> failed: …". This is the
 * ONE reporting contract for `StructuralResult.ok === false` on this surface
 * (#2544): every refusal — rename or otherwise — routes through this
 * function so none of them can drift into a second style. `description`
 * already names the attempted op ("Rename X", "Move X to Y", "Promote X to
 * knot", …), so the frame just appends "failed: <reason>" rather than
 * re-stating the verb.
 */
function notifyStructuralRefusal(
  state: StudioState,
  description: string,
  error: string | undefined,
): void {
  state._notify?.({
    severity: "error",
    source: "binder",
    message:
      error != null && error !== ""
        ? `${description} failed: ${error}`
        : `${description} failed`,
  });
}

/**
 * Report a refused rename — the `notifyStructuralRefusal` frame, specialized
 * with the "Rename X" prefix. Shared by both rename surfaces
 * (`performSymbolRename`'s modal path, #2528, and `applyComputedRename`'s
 * inline/F2 path, #2543) so the two cannot drift: same severity, same
 * source, same frame.
 *
 * The frame is "Rename X failed: <reason>", NOT "Cannot rename X: <reason>":
 * the op's most common refusal is literally "cannot rename this symbol",
 * which the latter turns into "Cannot rename hello: cannot rename this
 * symbol". Keep the frame and the op's own wording from colliding.
 */
function notifyRenameRefusal(state: StudioState, label: string, error: string | undefined): void {
  notifyStructuralRefusal(state, `Rename ${label}`, error);
}

/** The outcome of attempting a symbol rename. */
export interface SymbolRenameOutcome {
  /** True when the edits were applied (safe, or forced). */
  applied: boolean;
  /** The diagnostics the rename would introduce (the breakage report). */
  diagnostics: RenameDiagnostic[];
  /** An error from the rename op (symbol vanished, etc.), or the "no active
   *  project session" case (#2544), if any. Already reported to the user as
   *  an error notification before it is returned (#2528) — callers use it to
   *  decide control flow, not to surface it. */
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
  const project = state._project;
  const session = project?.getSession();
  if (!project || !session) {
    // No bound session — carries neither `applied` nor `error` before this
    // fix (#2544). `run()` in `SymbolRenamePrompt` only treats
    // `outcome.applied || outcome.error` as a terminal outcome; an outcome
    // with neither fell through to the breakage-report branch with an EMPTY
    // report, rendering "would break 0 places" — asserting the rename is
    // unsafe when in truth no session was ever bound — with a live
    // **Force rename** button whose retry hits this same branch forever.
    // Setting `error` here routes it through the same notify+close path
    // every other refusal on this surface takes.
    const label = renameLabel(req);
    const error = "no active project session";
    notifyRenameRefusal(state, label, error);
    return { applied: false, diagnostics: [], error };
  }

  // Offset-based (F2) covers any symbol under the cursor; name-based (menu)
  // targets a knot/stitch. Both return the same safe-rename payload.
  // W2e: the safe-rename compute rides the async session facade at
  // interactive priority (compute-only — application follows separately).
  const result =
    req.offset != null
      ? await project.structuralQuery<StructuralResult>("renameSymbolAt", [
          req.path,
          req.offset,
          newName,
        ])
      : await project.structuralQuery<StructuralResult>("renameSymbol", [
          req.path,
          req.knot ?? "",
          req.stitch ?? "",
          newName,
        ]);
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
