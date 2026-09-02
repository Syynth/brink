---
"@brink-lang/web": patch
---

New auto-fix surface: `getFixes` / `getFixesDoc` return the fixes for the
diagnostics under a cursor, and `applyFix` / `applyFixDoc` turn a chosen fix
into the sources to write (the same `StructuralResult` shape the structural
ops already return).

A `Fix { code, title, applicability, edits, caret? }` carries its own minimal
edits, which may span files — unlike a `CodeAction`, whose opaque `data` is
round-tripped through `resolveCodeAction`. The three diagnostic-keyed
quick-fixes that used to arrive as code actions (add-import for `E025`, the
`#fn(...)` creation-site trims for `E080`/`E081`, the `call`/`bind`
over-arity trim for `E063`) now arrive as fixes instead, so
`getCodeActions` no longer lists them and `CodeActionData` no longer has an
`AddImport` / `TrimFnLiteralArgs` / `BindFnLiteralRefArgs` /
`TrimValueCallArgs` variant.

One behavioral consequence: a fix is offered where its diagnostic is, so the
cursor must sit on the squiggle — previously the `call`/`bind` trim was
offered anywhere inside the call.
