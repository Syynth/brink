---
"@brink-lang/web": patch
---

Fix #2903: an index-operand postfix (`a[0]++`, `m["k"]++`) compiled clean
and silently never mutated anything, on both the `~ { … }` block surface
and the classic-line surface — the sibling gap PR #2900's review found next
to #2894's bare-variable fix.

An `Index` operand is neither `Path` nor `FieldAccess`, so the
field-projection guard `try_lower_postfix_stmt` already had never matched
it, and `lower_assign_target`'s bare-`Path`-only match fell through to
`None` — the postfix value was computed and discarded, the same
silent-drop shape #2894 fixed for a bare variable, just on an index target.

An index-operand postfix now routes through `lower_indexed_assignment`,
the same take/mutate/write-back RMW discipline `a[0] += 1` already uses —
proven correct for both a list index and a map key before relying on it.
A struct-field-projected index root (`p.items[0]++`) still refuses with
the same non-suppressible `E074` issue #2121 established for `p.items[0]
= v`, rather than silently misrouting; a plain field-operand postfix
(`a.count++`) keeps refusing with the identical E074 issue #2185/#2897
established, unaffected by this fix.

The RMW sequence this routes to can splice several `lir::Stmt`s, which the
classic-line statement dispatcher's single-`Option<Stmt>` return can't
express — `mod.rs`'s top-level classic-line dispatch and `content.rs`'s
`lower_inline_block` (choice-text inline conditionals/sequences) now
intercept an index-operand postfix with their own multi-statement-splicing
arm before it can reach that truncating fallback, mirroring the existing
`try_lower_indexed_assignment` precedent for `~ a[i] = v`.
