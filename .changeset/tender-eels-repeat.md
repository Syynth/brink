---
"@brink-lang/studio": patch
---

Inline (F2) rename no longer reports a REFUSED rename as a success (#2543).

The editor's inline rename commits through `applyComputedRename` →
`applyMoveResult`, and neither checked `result.ok`. A rename the op refuses —
"cannot rename this symbol" when the cursor is not on a declaration, "file not
loaded" when the file went away mid-rename — therefore reached the apply seam,
which pushed an undo entry, raised the confirming **info** toast ("Rename X to
Y") with an Undo button, and re-keyed the symbol's open tab, all for an edit
that never happened.

`isSafeRename` cannot catch this: a refusal carries `safe: true` with no
introduced diagnostics, so the editor's inline gate reads it as safe and
commits. `safe` describes the breakage of edits that were actually computed;
`ok` is the field that says whether the operation happened. The guard is now on
`ok`:

- `applyComputedRename` refuses an `ok: false` result and raises an
  error-severity `binder` notification carrying the op's own reason
  ("Rename hello failed: cannot rename this symbol") — the same channel the
  modal prompt's refusal path uses (#2528).
- `applyMoveResult` refuses an `ok: false` result at the seam, so no caller can
  turn a refusal into a toast plus an undo entry.

Successful renames are unchanged: edits apply, one informational toast with
Undo, symbol tab re-keyed.

Per-op refusal reporting for the remaining structural ops is tracked separately
in #2544 and is not changed here.
