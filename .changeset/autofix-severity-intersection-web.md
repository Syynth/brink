---
"@brink-lang/web": patch
---

Auto-fix surfaces now intersect the project's own severity resolution. A
diagnostic code set to `allow` in `brink.toml`'s `[lints]` table is
suppressed — it has no Problems row — but `getFixOffers`, `countFixes`,
`fixAll` and `fixesAtPath` read the raw per-file diagnostic list, so such a
code was still offered a Fix button, counted into "Fix all safe (N)", and
rewritten by the batch with nothing on screen to explain it. All four
queries now withdraw every code whose effective severity resolves to
"suppressed", before any other narrowing, so a caller naming the code
explicitly cannot get it back either.

`fixesAtPath` (the editor/Problems-row cursor menu) also now applies the
*other* suppression channel: an inline `// brink-disable`/`brink-expect`
directive or an `@[allow(…)]` scope withdraws a diagnostic from the menu the
same way it already withdrew it from `fixOffers`/`fixCount`/`fixAll` — the
cursor menu previously read the compilation's raw per-file diagnostics with
no suppression applied at all, so a line the author had explicitly silenced
could still offer a Fix action with no diagnostic on screen to explain it.
