---
"@brink-lang/web": patch
"@brink-lang/editor": patch
---

Session-level read-only enforcement for a mounted stdlib file (issue #2306, ruled 2026-08-06 "Mounted
stdlib presents as a read-only library node", part 3 of the ruling — built first per its own sequencing
note). #2231/PR #2303 mounted the stdlib into `EditorSession` and hid mounted files from
`list_files`/`project_outline`/`story_graph`, but a by-id route that resolves a file outside those three
listings — a doc handle opened via goto-def navigation into an inherited symbol, or a bulk TS-level caller
like project-wide search/replace — could still write through to the mounted copy and hand the edit to the
host to persist, silently forking the stdlib into the project.

`EditorSession` (`@brink-lang/web`) gains `is_read_only(path)`, and `update_document` now refuses (returns
`"null"`, the existing "did not apply" sentinel) when the handle's file currently resolves to a mounted
id — `open_document`/`open_fragment` still succeed on a mounted path, so it stays browsable/openable, only
writing through the handle is rejected. `update_file`/`update_source` are deliberately left unguarded:
they are the host's whole-file "this is the content now" API, and a real project file placed at a mounted
key must keep winning by construction-time ordering (the existing shadowing contract).

`EditorSessionHandle.isReadOnly` (`@brink-lang/web`) exposes the new query. `ProjectSession.applyEdit`
(`@brink-lang/editor`) — the shared seam every bulk-edit caller (search/replace, results-buffer edits,
binder undo) already routes through per issue #137 — now checks it before writing and returns `boolean`
(previously `void`) so a caller can react to a refusal instead of assuming success.
`ProjectSession.initialize()`/`addFile()`/the external-change handler are unaffected: they call
`session.updateFile` directly, exactly like a legitimate shadow write.

`@brink/studio-store`'s search slice (internal, not independently versioned) surfaces a refusal from the
three `applyEdit` callers (`replaceSearchMatch`, `replaceAllSearchMatches`, `applySearchRowEdit`) as a
"read-only" notification instead of silently continuing.

