---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Two structural gaps in the paint-path-defer family (issue #2794, found by
#2788's adversarial re-review — "the enrolment family's gap, not this PR's").

`ProjectSession` (`@brink-lang/editor`): a gated call deferred via
`scheduleIdleWork` (today, `renameFile`) could outlive `destroy()` — an
unmount landing inside the deferral's idle window let the scheduled callback
fire anyway and call into a wasm handle `destroy()` had already freed. This
was contained (the throw surfaced as an ordinary error notification through
`applyRename`'s existing `catch`), not unreachable, but containment is not a
fix. `deferForGatedCall` (replacing a bare `scheduleIdleWork` await) now
tracks its idle handle and rejects the caller's `await` — instead of
resolving into a freed session — if `destroy()` runs first; `destroy()`
cancels every still-pending handle and rejects its caller before freeing the
wasm handle. One guard, meant to cover every gated call this class defers,
present or future.

`structuralOpPending` (`@brink/studio-store`, bundled into
`@brink-lang/studio`): two independent fire-and-forget writers
(`runGatedStructuralOp` for symbol-menu ops, `applyRename` for Binder
rename/move) both cleared this status-bar pending indicator unconditionally
in a `finally`. An overlapping Binder drag-move and symbol-menu op could
erase each other's still-live indicator, whichever settled last winning
regardless of which op was actually still running. `SymbolMenuSlice` gains
`clearStructuralOpPending(description)` — a compare-and-clear that only nulls
the field when the live value still equals the description the clearing
call itself set — and both writers now clear through it instead of calling
`setStructuralOpPending(null)` directly.
