---
"@brink-lang/web": patch
---

Native lambdas lower (#1685): a `|x| …` lambda in a `.brink` source no
longer disappears behind the blanket "construct not supported by this
lowering" diagnostic (E129). It lowers to a real HIR node — pipes with the
ruled colon return, optionally annotated params, single-expression or
braced-block bodies with the trailing expression as the value — so its body
is now analyzed, its params resolve as locals (hover/go-to-definition see
them), and a write to a captured binding is reported as the new compile
error E156 instead of passing unnoticed. Because a lambda has no runtime
representation yet, compiling one still fails, but with a targeted E052
naming the missing lifting step rather than E129. Ink sources are entirely
unaffected — ink's grammar cannot spell a lambda — and the oracle corpus is
unchanged.
