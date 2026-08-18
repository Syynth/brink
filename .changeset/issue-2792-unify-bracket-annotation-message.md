---
"@brink-lang/web": patch
---

Issue #2792: the native `.brink` parser now reports the same targeted
message — `` expected `<` or end of type name, found L_BRACKET `` — at
every position that reads a type name when it's followed by `[` (`fn`/`flow`
params and return types, lambda params and returns, `let`, `var`/`const`,
and struct fields), instead of each position's own incidental "expected
NEXT_TOKEN" fallout (`expected R_PAREN, found L_BRACKET`, `expected PIPE,
found L_BRACKET`, `expected a braced body after the fn header`, and worse).
A lambda's own return annotation (`|y: int|: Option[int] { none }`) used to
produce **zero** diagnostics — the leftover `[int]` silently parsed as the
lambda's body, dropping the real one — and now fails loudly with the same
message. `[…]` is not the type-argument delimiter (the 2026-07-27
angle-bracket ruling retracted `Option[T]`; `[…]` is reserved for array
literals, #1490). Recovery (the parser-generated garbage each position
leaves after the diagnostic fires) is unchanged — #2792 scoped that out as
a separate, bigger design question.
