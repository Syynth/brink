---
"@brink-lang/studio": patch
---

Fix on save (`docs/autofix-spec.md` §7) now persists every file a batch
touched, not only the focused one. `file.save` (⌘S) narrows its host-save
write to the focused path — correct for an ordinary edit — but the
fix-on-save step running inside that same save can rewrite other files too
(a cross-file fix); those were staying staged and silently unpersisted
while the save reported success. `file.save` now checks
`runFixOnSave`'s own return (every path it actually wrote) and, when that
names more than the focused file, routes the write through the same
per-path confirm→retire algorithm `file.saveAll` already uses, narrowed to
exactly the touched set. A toast names the other file(s) written; the
focused file's own "Saved" notice, and fix-on-save's deliberate no-toast
rule for the file being saved, are unchanged.
