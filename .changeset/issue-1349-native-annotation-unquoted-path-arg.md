---
"@brink-lang/web": patch
---

#1349 (companion to the closed #1286): `brink-syntax-native`'s
annotation-arg grammar (`@[name(args)]`) gains an unquoted `::`-separated
module-path arg production — `@[was(story::old::path)]` now parses to a
`PATH` node (reusing `expr::path`'s existing `PATH`/`PATH_SEGMENT`
shape, exposed via `AnnotationArg::path`) instead of failing with
"unexpected token in annotation arguments". A single-segment path (no
`::`) is unaffected and still parses as the existing bare-ident arg.

Reachable through any `@brink-lang/web` session that parses a
`.brink`-extensioned file containing an `@[…]` annotation line whose
first arg is an unquoted `::`-path — the diagnostics for that specific
shape change (parse error → clean parse). `lower_native::module`'s
`@[was(...)]` lowering still only consumes the quoted-string arg form
(`hir.module.was` is unaffected); wiring the new unquoted-path shape into
that lowering pass is a follow-up, not done here.
