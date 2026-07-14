---
"@brink-lang/web": patch
---

RCA'd #680 ("`ref`-argument call co-occurring with a `temp` decl in the
same `~ { }` block resolves to the wrong global slot"): the `ref`-argument
call was a red herring — the actual defect is reading a T1b block-scoped
`temp` (`~ { … }`) from *outside* its own block. LIR lowering's fallback
for "temp not currently visible" (kept for inklecate-compat forward-
reference emulation of classic, non-block temps) previously caught this
case too, silently compiling to a phantom global id that was never
registered — a runtime-only `UnresolvedGlobal` fault with no compile
diagnostic.

Observable through editor diagnostics: referencing a block-scoped `temp`
(by value or by `ref` argument) after its `~ { … }`/`while`/`for`/`if`
block has already closed is now a real, non-suppressible compile error
(`E082`) instead of a silent runtime fault. A `ref`-argument call
co-occurring with a `temp` decl in the *same* block — the issue's literal
repro shape — was already correct and is unaffected.
