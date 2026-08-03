---
"@brink-lang/web": patch
---

Fix #2122: `if get(bag) as b { b.items = […] }` and `if get(bag) as b { push(b.items, 1) }` — an as-binding's struct-field write/mutator — used to compile clean and silently mutate the (supposedly immutable) binding, instead of raising the `E148` every other write shape (plain/compound assignment, indexed-assignment root, bare in-place mutator, `ref`-argument passing) already raises.

`lower_single_level_field_write` and `lower_field_mutator` (`crates/internal/brink-ir/src/lir/lower/blocks.rs`) each resolve a `Param`/`Temp` root's slot themselves (`ctx.temp_slot(&head_name)`, the *head* of a two-segment `p.field` path) rather than routing through `stmts::lower_assign_target` — the choke point that already refuses a write to an `as`-binding slot for every other shape. Their root is the head of a two-segment path, not the whole assignment target `lower_assign_target` resolves, so calling that function directly is not a drop-in substitute; instead, the E148-diagnosing logic is now factored into a shared `stmts::reject_as_binding_write` helper that both functions call at their own root-resolution site, alongside `lower_assign_target`'s own (refactored) call to the same helper.

This PR does **not** address this issue's other named gap — `CONST` roots are still not rejected on any assignment path (not just the two mutator/field-write functions the issue names: plain `CONST c = 1 … c = 5` is also silently accepted today, with no diagnostic anywhere in the compiler). Fixing that needs a new `DiagnosticCode` that was not pre-assigned for this item, so it is reported back to the issue rather than a code being self-allocated.
