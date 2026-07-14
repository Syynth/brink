---
"@brink-lang/web": patch
---

Fixed a fuzzer-discovered parser bug (PR #672 workstream C's new
`parse_lossless` fuzz target, which builds with debug-assertions on):
a `bump_assert` invariant inside the parser could fire on legitimately
reachable token sequences — e.g. an un-flushed `WHITESPACE` token
still sitting at the parse position when `conditional_with_expr_standalone`
dispatches into `expression()` on a `#fn(...)`/sigil-literal expression
inside a `MULTILINE_BLOCK` — crashing the parser with a
`debug_assert_eq!` panic in debug builds. In release builds (including
the shipped `@brink-lang/web` wasm), the same mismatch compiled away
silently: the parser consumed the unexpected token with no diagnostic
at all, corrupting the tree instead of erroring.

`bump_assert` now always emits a proper parse error on a mismatch, in
every build profile. Observable through `@brink-lang/web`: compiling
ink source that hits this token-position edge case no longer panics in
debug tooling, and — this is the real production-facing change — no
longer silently mis-parses in the shipped wasm build; it now returns a
normal `ok: false` result with a recovery-error diagnostic, like any
other malformed input.
