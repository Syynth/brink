---
"@brink-lang/studio": minor
---

Binder v2, part 3 (#3039): inline creation. Every container (and the
binder foot for the root) carries a 50/50 pair of dashed icon buttons —
new file / new folder — expanding in place to a full-width name input
(bare name, .ink implied for files) with inline validation (no paths,
duplicate check normalizing the extension). Folder creation goes through
the order sidecar's empty-folder registry, so a new folder renders
immediately and survives reloads — in-app folder creation exists at
last. The folder context menu gains "New folder here"; "New file here"
now opens that folder's own input (container implied, no seeded path
prefix). The caret discipline of the old New File input (#2571) carries
over unchanged.
