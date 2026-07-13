---
"@brink-lang/editor": patch
---

HIR overlay now also refreshes when a view MOUNTS after a compile was
already delivered (#518, follow-up to #494/#502). The 0.9.1 fix refreshed
mounted views on `deliverCompile`, but for a slot without a view the refresh
was dropped, not queued — so in the mount-after-initial-compile order (an
external embedder's passive-load sequence: `ProjectSession.initialize()` →
`triggerCompile()` → the framework commits the editor mount afterwards) the
overlay showed whatever its `StateField` last held and nothing ever
repainted it: a passive load never compiles again, and a remount reuses the
cached `EditorState`, so the field's `create()` never re-runs and a value
cached blank at unmount persisted until the first keystroke.

`DocumentSessions.mountView` now self-serves the missed refresh: when a
compile has already been delivered (`lastCompileDelivered`), it dispatches
`refreshHirOverlayEffect` to the freshly mounted view — after the slot's
wasm handle is (re)opened, so the projection read is live at mount time.
The overlay's refresh trigger set is now
{compile-deliver} ∪ {view-mount-after-a-deliver}, covering both mount
orders and cached-state remounts.

Also documents (hir-overlay.ts, editor-consumer-guide) that a host-dispatched
`refreshHirOverlayEffect` is matched by object identity, so it must come from
the same module instance of `@brink-lang/editor` that built the view's
extensions — a bundler-duplicated copy produces an effect the field silently
ignores, which can make host-side refresh workarounds appear to "read empty".
