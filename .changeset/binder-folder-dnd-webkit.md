---
"@brink-lang/studio": patch
---

Binder: fix folder drag-reorder silently failing in WebKit (Tauri desktop
app / Safari). The row's drag handlers wired `onDragOver`/`onDrop` but had
no `onDragEnter` at all — WebKit's HTML5 drag-and-drop requires
`preventDefault()` on both `dragenter` and `dragover` for an element to
remain a valid drop target, while Chromium tolerates `dragover` alone, so
the reorder worked in the browser preview but never in the desktop app.
`onDragEnter` now runs the same accept/reject logic as `onDragOver` on
every Binder row (files, folders, knots, stitches) and the root drop zone;
rows also opt into `-webkit-user-drag: element` as a defensive measure
against WebKit's stricter interactive-children drag-start gating, scoped to
`.brink-binder-row[draggable="true"]` only — React renders `draggable={false}`
as the literal attribute, and an unscoped rule would have let WebKit's
presentational-hint cascade re-arm non-draggable rows (read-only
`FileProvider`, pre-seed window) as drag sources.
