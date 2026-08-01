---
"@brink-lang/web": patch
---

Compiler: a `@[element(claims = "…")]` handler's captured parameter
declaring a numeric/struct/generic/`fn` type now raises a targeted
diagnostic (`E171`, issue #1849) at the declaration, instead of silently
compiling and (depending on the handler's own body) either surfacing a
confusing whole-line `E063` type mismatch once a line is claimed, or
silently binding the wrong type.

`hir::lower_native::element::try_claim` binds every named capture as a
plain `Expr::String` literal, unconditionally, regardless of the
receiving parameter's declared type — so `@[element(claims = "^Take
(?<n>\d+)$")] fn take(n: int)` could never actually receive an `int`.
Numeric capture coercion is `docs/prose-dialect-spec.md` §3.5b's own
Deferred item — the underlying gap stays deferred, not built here — but
the silence around it is closed: `E171` fires at the mismatched
parameter's own type annotation, and a handler that fails this check is
never registered as a claiming handler at all (the same posture
`E160`/`E166`/`E167` already take), so the offending line is left
unclaimed rather than rewritten into a call that could never type-check.

`content`-typed captured params are exempted (not flagged) — the spec's
own ruled `fn radio(chan: string, text: content)` example and the
`tier1-native/annotations-element` golden fixture both declare one today
and compile clean; see `E171`'s own doc
(`docs/diagnostics/E171.md`) for why.

`brink-web` transitively depends on `brink-ir`'s native lowering
(`brink-db::lowered_query` dispatches `.brink`-extension files to native
parsing/lowering, non-optional), so this new diagnostic is wasm-observable
for `.brink` projects — a claiming handler with a numeric/struct/generic/
`fn`-typed captured param now reports `E171` in the editor instead of
compiling silently wrong or reporting a whole-line `E063`.
