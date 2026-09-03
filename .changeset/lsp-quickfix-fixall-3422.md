---
"@brink-lang/web": patch
---

LSP quickfix code actions and `source.fixAll.brink` (milestone 7 of the
auto-fix epic) now honor suppression the same way the Problems panel does:
a diagnostic dropped by an inline suppression directive or leveled to
`[lints] allow` no longer offers a quickfix that claims to discharge
something the client never displayed. `source.fixAll.brink`'s whole-file
batch also abandons cleanly instead of shipping a partial multi-file edit
when one changed file's path can't be represented in the `WorkspaceEdit`,
and the batch's whole-project scratch analysis is now skipped entirely
when no registered fixer can produce anything at the requested tier.
