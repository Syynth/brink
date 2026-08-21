---
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

**Breaking:** `ProjectSession.renameFile` now resolves `Promise<RenameFileResult>`
instead of `Promise<string[]>` — a consumer doing `(await project.renameFile(a,
b)).length` or iterating the resolved value directly will break at runtime.
`packages/ink-editor/src/index.ts` also gains two new exported types,
`RenameFileResult` and `RenameDirResult`.

Surface the rename/move breakage gate at the Binder's rename call sites (issue #2918).

`ProjectSession.renameFile`/`renameDir` (`@brink-lang/editor`) run the same
safe-by-default breakage gate every other structural op does (#316): the
wasm `rename_file`/`rename_dir` ops already compute `safe` and
`introduced_diagnostics` correctly. But both methods used to resolve with
only the bare data a caller needed to apply the move (a referrer path list,
or `{ moved, referrers }`) — discarding the breakage-gate verdict entirely.
A move that broke a reference (a divert pointing at the renamed file, for
example) applied exactly like a clean one, with nothing anywhere telling the
user.

`renameFile` now resolves with `{ referrers, safe, introducedDiagnostics }`;
`renameDir` with `{ moved, referrers, safe, introducedDiagnostics }`. The
Binder's `applyRename`/`applyDirRename` (`studio-store`'s binder slice,
bundled into `@brink-lang/studio`) thread the verdict through to the same
`_notify` channel PR #2916 used for a refused move: a `safe: false` result
now raises a `warning`-severity "breaks N reference(s)" notification instead
of the unconditional `info` toast every rename got before. This is the
notification FLOOR, not a preflight gate — the move still applies (the undo
entry still gets pushed) exactly as it did before; the user is now told
about the breakage rather than discovering it later. The fuller "will break
N references" preflight/confirm pattern (#324) exists for the editor's
inline symbol rename, on a dedicated widget the Binder's type-a-new-name
tree rename has no analog of — building one is out of this fix's scope; see
issue #2918 for the follow-up.
