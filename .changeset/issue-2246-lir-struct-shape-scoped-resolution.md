---
"@brink-lang/web": patch
---

Compiler: LIR lowering no longer skips the std-exclusion for a struct
shape name resolved as the sole candidate for its bare name (issue #2246).

`ShapeTable::resolve`'s "fast path" used to return a bucket's only
candidate unconditionally, bypassing the referrer-scoped, std-excluding
resolution every multi-candidate lookup already went through (issue
#2238) — so a struct name that only a mounted `story::std…` module
declares, with no project-side declaration of that name anywhere, would
silently resolve through with no import, the same "reach into std with
no import" class #2197/#2238 closed for every other bare-name lookup.
`resolve` now always routes through `decls::lookup_global`.

Separately, a struct construction literal's shape name (`Name#{…}` /
`Name { … }`) is a `RefKind::Struct` reference the analyzer already
resolves against the referrer's module scope — lowering now consumes
that recorded resolution directly (both in expression position and in a
`VAR`/`CONST` declaration default) instead of re-deriving it through
`ShapeTable`, removing a duplicate resolution implementation for this
reference kind.
