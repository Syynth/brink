---
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

External deletion of an open file: keep the view, mark orphaned; ⌘S
recreates (issue #2371, 2026-08-07 decision). `mountStudio`'s
`onExternalFileChange` used to skip deletions entirely; it now calls
`DocumentSessions.markOrphaned`, which never touches the kept editor buffer
(no refresh, no auto-close) and recreates the file in the wasm session from
that buffer so IDE queries and a later save keep working. `FileChangeHub`
gains an `orphaned` path set (`isOrphaned`/`orphanedPaths`, mirroring the
existing `conflicted` tracking) — set by `noteOrphanRecreated` once a kept
buffer is confirmed to survive a deletion (`applyExternal(path, null)`
alone does not flag it, so a headless deletion with no open view never gets
permanently badged), cleared by a canonical save (`markSaved`, or a
write-through `flush()`) or by the path reappearing on disk. New
`ProjectSession.recreateOrphaned` (the
provider is deliberately not notified until a real save, so recreation
stays gated on ⌘S even for a provider whose `onFileChanged` is itself the
persistence step) and `isOrphaned`/`orphanedPaths` pass-throughs. New
`StudioApi.getOrphanedFiles()`, mirroring `getDirtyFiles()`, for a host to
render an orphaned-tab badge.
