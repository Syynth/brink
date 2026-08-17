---
"@brink-lang/editor": patch
---

Two internal correctness fixes with no observable behavior change, both found by PR #2548's review (#2557, #2558):

- `InlineNameInput.dispose()` (the shared widget behind F2 inline rename and extract-to-knot/function)
  now clears its two remaining deferred `setTimeout(…, 0)` handles — the post-mount focus timer and the
  breakage-report force-button focus timer — alongside the debounce timer and idle handle it already
  cleared, matching the class doc's "tears them all down" claim. Applied the same pattern to the two
  sibling sites with an identical unguarded post-teardown focus timer: `ExtractPrompt` (`extract-actions.ts`)
  and `InlineRename` (`rename.ts`). All three were latent, not live — the owning DOM is already detached
  by the time any of these timers fire, and `focus()` on a detached node is a no-op — but each timer is
  now cancelled on teardown so a future change to the callback can't turn the latent leak into a live one.
- `RenameQueryCache`'s cache-key separator is no longer a literal NUL byte. The old
  `` `${path}\x00${offset}\x00${newName}` `` made `rename.ts` register as a binary file to `grep`/`rg`
  without `-a`/`--text`, silently hiding the file's own lines (including this method's) from any
  repo-wide sweep. The key is now `JSON.stringify([path, offset, newName])` — provably collision-free
  (JSON.stringify of a fixed 3-element array is injective) and, unlike `\x00`, keeps the file plain
  greppable UTF-8 text.
