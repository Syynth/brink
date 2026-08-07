---
"@brink-lang/web": minor
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

Binder "Library" section for mounted `std/` files (issue #2343, part 2 of #2306's ruling "Mounted stdlib
presents as a read-only library node" — part 3, session-level read-only enforcement, shipped separately
in #2342). `list_files`/`project_outline`/`story_graph` (`@brink-lang/web`) switch from **excluding**
mounted stdlib files entirely (#2231's phantom-row fix) to **listing them flagged** (`mounted: boolean` on
`ProjectFile`/`FileOutline`/`StoryGraphNode`) — dropping the exclusion without adding a consumer that
renders the flag would reintroduce the exact phantom-row bug #2231/#2303 fixed, so this ships both
together. `EditorSession::remove_file` (`@brink-lang/web`) and `rename_file` now refuse a mounted path
(the delete/rename route gap #2343's review found: previously unreachable only because `list_files`
excluded the mount from the Binder) — `remove_file` gains a `boolean` return (previously `void`).

`@brink-lang/studio`'s Binder renders a visually distinct, collapsed-by-default "Library" section below
the project's own file tree: browsable (expand/collapse a folder tree, click/double-click to open a file
read-only) but with no drag, rename, delete, or new-file affordances. `@brink/studio-store`'s search slice
(internal) excludes mounted files from `runSearch`'s candidate list — "Excluded from save-all and
search/replace" per the ruling — and `ProjectSession.markAllSaved` (`@brink-lang/editor`) does the same for
`file.saveAll`. The binder slice's `applyMoveResult`/`undo` now surface an `applyEdit` refusal (a structural
move or undo landing on a mounted path) as a "skipped N read-only file(s)" warning instead of a silent
no-op behind a success toast.

A mounted file's CM6 view (`@brink-lang/editor`'s `DocumentSessions`) is now genuinely non-editable —
`EditorState.readOnly` + `EditorView.editable.of(false)`, the same pattern `conflict-view.ts` uses for its
"ON DISK" pane — rather than relying solely on the wasm-layer write refusal to make a keystroke silently
revert. `ProjectSession` gains a public `isReadOnly(path)` query for this. Navigation (goto-def/hover) into
a mounted file lands in the same read-only view via the existing open-file path — no special-casing needed.
