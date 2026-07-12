---
"@brink-lang/web": patch
---

#585: a nested choice (or labeled gather block/conditional/sequence)
embedded inside an un-lifted inline conditional in a choice's own
display/bracket/inner text (e.g. `* Pick {x > 0: - true: * nested -> END
- else: text}`) is now a targeted, Error-severity compile error (`E059`),
replacing a `debug_assert!(false, …)` guard on the arm that handles it.

This is a real behavior change for real ink, not just defense-in-depth:
in a release build (including the shipped `@brink-lang/web` wasm), the
`debug_assert!` was compiled out, so `lower_stmt` silently returned `None`
and dropped the nested statement — `lower_to_program` still produced
`Some(program)` with no diagnostic, and `lir_query` treated that as a
successful compile. The web playground would silently accept this input
and produce a wrong story with the nested construct missing, with no
indication anything was lost. With `E059` now Error-severity, `lir_query`
gates on it (`program: None` once `lir_errors` is non-empty), so this same
input now fails to compile in the web build instead of silently
miscompiling. Sibling of #578's analogous `E057`/`E058` hardening
(`t1b-4-diagnostics-hardening.md`), which shipped the same kind of
changeset for the same reason.

#586's codegen backstop (out-of-loop `LogicBreak`/`LogicContinue` in
`brink-codegen-inkb`) is unaffected: that input is already rejected
non-suppressibly upstream by LIR lowering's `E057` before a `Program`
ever reaches codegen, so no valid compile path's observable behavior
changes — no changeset needed for that half.

Oracle corpus: unchanged, 5,577 passing episodes.
