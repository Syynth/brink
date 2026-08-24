---
"@brink-lang/studio": minor
---

Binder v2, part 2 (#3038): the `.binder.json` order sidecar — placement
is authorship. Per-container display order (files and folders
interleave; the fallback is entry first, folders before files, then
alphabetical), drag-to-reorder for files and folders within their
container (folders reorder-only; a file's drop-into-folder move is
unchanged), an empty-folder registry so in-app folder creation can
render before any file exists, re-keying on rename/move and cleanup on
delete, and subtle indent guides in the tree. The sidecar is loaded and
written through the host FileProvider; it never enters the wasm session,
a corrupt file self-heals to the fallback, and hosts without persistence
keep working in-memory.
