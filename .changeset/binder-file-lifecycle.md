---
"@brink-lang/studio": minor
"@brink-lang/web": minor
---

Binder file lifecycle — manage whole files and folders directly in the binder.

- **Delete** files and folders from the context menu, with undo.
- **Rename** files and folders inline (F2 or the context menu). Every `INCLUDE` that points at a renamed or moved file is rewritten automatically, and `..`-relative include paths now resolve correctly across the toolchain.
- **Move** files by dragging onto a folder, drag a file back out to the project root, and multi-select to move several files at once — all undoable, with one "Moved N files" step.
- Renaming a file keeps its open editor tab in place (pin, split, and selection are preserved) instead of reopening it.

`@brink-lang/web` gains the `rename_file` session op, which computes the edit set for a file move: the re-keyed file content plus the referrer `INCLUDE` rewrites.
