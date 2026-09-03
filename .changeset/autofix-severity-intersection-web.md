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
