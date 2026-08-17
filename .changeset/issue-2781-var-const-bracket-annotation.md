---
"@brink-lang/web": patch
---

Issue #2781: the native `.brink` parser now reports `` expected `<` or end
of type name, found L_BRACKET `` (surfacing as `E037`) when a `var`/`const`
type annotation is followed by `[` — e.g. `var x: Option[int] = none` —
instead of silently reinterpreting the rest of the line as narrative prose
and dropping the initializer. `[…]` is not the type-argument delimiter (the
2026-07-27 angle-bracket ruling retracted `Option[T]`; `[…]` is reserved for
array literals, #1490). `fn`/`flow` params, return types, and lambda params
already failed loudly on this input (#2780); `var`/`const` annotation
position was the one remaining silent-drop gap this closes.
