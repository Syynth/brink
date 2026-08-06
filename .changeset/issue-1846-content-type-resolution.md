---
"@brink-lang/web": patch
---

`content` is now a resolvable type in the native type system (#1846):
`fn radio(chan: string, text: content)` — the ruled #1719 example — and
any other `content`-typed parameter or annotation no longer trips `E061`
("unknown type"). `content` is a distinct nominal leaf, deliberately not
coercible to or from `string` — the whole point of the type is that a
captured prose value stays translation-resident rather than silently
flattening to a plain string.

This is the type-resolution prerequisite only. The dispatch mechanism
that actually binds a captured run to a `content`-typed parameter (the
`@[element(args = "…", block)]` block-capture rewrite) is issue #1839's
scope and is not delivered here.
