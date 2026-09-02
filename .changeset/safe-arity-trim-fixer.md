---
"@brink-lang/web": patch
---

Two new `Safe` auto-fixers for `E031` (ordinary call over-arity) and
`E176` (divert-with-args over-arity): trim an over-supplied call/divert
site's excess arguments. The trim removes the *leading* excess arguments
and keeps the *trailing* ones — the classic calling convention these two
diagnostics cover binds the trailing supplied argument to the callee's
declared parameter, not the leading one — and is withheld entirely when
any of the leading (dropped) arguments could carry a side effect (a
nested call or an assignment). Offered fixes and `fix_all` results
reaching `EditorSession` now include these two codes.
