---
"@brink-lang/editor": patch
---

`ProjectSession.renameFile` no longer runs its gated wasm call (`rename_file`, which runs the
same full-project breakage gate as the knot/stitch structural ops) synchronously on the paint
path (issue #2776, generalizing #2767/#722's remedy). The wasm call is now deferred to the next
idle slot via `scheduleIdleWork`, so under CPU contention a file/folder rename or move (the
Binder's inline rename, drag-move, and multi-select move all go through this method) no longer
blocks the main thread inline in the same frame as the triggering event. Callers that render a
busy indicator while awaiting `renameFile` (the studio's `applyRename` commits `structuralOpPending`
synchronously before the call) now get a real paint of it before the heavy work begins; callers
that don't render one see no behavior change beyond the deferral itself.
