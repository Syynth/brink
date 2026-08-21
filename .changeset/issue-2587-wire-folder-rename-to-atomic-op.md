---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Wire the Binder's folder rename to the atomic `rename_dir` op (issue #2587).

The Binder's folder-rename action (`renameFolder`, `packages/studio-store/src/slices/binder.ts`,
bundled into `@brink-lang/studio`) looped a per-file `renameFile` call over
every file under the folder — the exact pattern `rename_dir` (#314) was built
to replace, because a per-file loop computes each file's cross-file INCLUDE
edits independently, against whatever has already moved, rather than against
one pre-move snapshot. Concretely: a folder move that only changes the
directory prefix (every moved file keeps its own basename) left an outside
referrer's `INCLUDE` pointing at the old, now-nonexistent path, because a
same-basename rename never triggers the per-file op's basename-keyed
cross-file rewrite.

`ProjectSession` (`@brink-lang/editor`) gains `renameDir`, the directory
analog of `renameFile`: it calls the atomic wasm `rename_dir` op (unused by
any TS caller since #314 landed), applies every moved file's content plus
the outside referrers' rewrites from that one snapshot, and writes each
moved file through the provider (a provider write is inherently per-file —
the atomicity guarantee lives in the edit computation, not in these writes).
Deferred off the paint path via the same `deferGatedCall` yield `renameFile`
uses (#2776), since `rename_dir` runs the identical breakage gate.

`renameFolder` now calls `project.renameDir` instead of looping
`applyRename`. All-or-nothing failure semantics (a deliberate change from
the old loop's silently-skip-a-collision-and-move-the-rest behavior): a
partial directory move can only be computed by falling back to per-file
INCLUDE rewriting for the files that "succeed," which is exactly the
inconsistency #314 exists to prevent, so a collision now refuses the whole
move with one error notification and nothing moves. Undo gets a new
`rename-dir` entry kind that re-applies `renameDir` with the prefixes
swapped, so undoing a folder move gets the same single-snapshot consistency
guarantee the forward move does, instead of falling back to a per-file undo
loop.
