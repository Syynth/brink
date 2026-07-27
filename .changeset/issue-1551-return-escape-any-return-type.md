---
"@brink-lang/web": patch
---

Issue #1551: `strict::check_def`'s return-value checks (Unknown/Conflicted
escape `E065`/`E066`, and a new fall-through check) now run for **any**
def carrying a declared, non-`void` return-type annotation — a
value-returning `flow`/nested `flow` (knot/stitch) — not just `is_function`
`fn`s. Declaring a return type on a flow (#1509/#1546) was previously
legal and completely unchecked.

New compile diagnostic `E150`: a def declares a non-`void` return type but
its body may fall through (reach the end, or only ever bare-`return`)
without ever executing a value-carrying `return <expr>`. This is the
checker error `docs/decision-log.md`'s 2026-07-22 implicit-end ruling
(item 3) promised but deferred: "a flow that declares a return type must
produce a value... falling through without a value is a checker error",
distinct from a runtime "ran out of content" — an implicit `-> DONE` is
never treated as satisfying a declared return value. Strict-mode-only
(`types = strict`, the brink dialect's own implicit default); `types =
gradual` is unaffected.

This also fixes a latent gap in the pre-existing `is_function` case: an
annotated `fn f(): int { … }` with no `return` anywhere previously
inferred as void via a blanket "never returns a value ⇒ void" shortcut and
skipped checking entirely, silently accepting a declared `int` the body
never produced. It is now `E150` too, the same as a flow/stitch.

The oracle corpus (vanilla ink, gradual-typed) is unaffected — no vanilla
`.ink` fixture declares a flow/stitch return type or exercises this
strict-only check.
