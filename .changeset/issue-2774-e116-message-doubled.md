---
"@brink-lang/web": patch
---

Fixed a doubled `E116` diagnostic message. `option_conditions.rs`'s
`check_condition` built its own `format!` that repeated
`DiagnosticCode::E116.title()`'s wording verbatim right after the title,
so an `Option[T]` truthiness condition (`if optionValue { ... }` instead
of `== none`/`== some(x)`) rendered the sentence twice in a row.

The message is now:

> an `Option[T]` has no truthiness — test `== none` / `== some(x)` in the
> condition (F27, docs/stdlib-spec.md §1.6)

This is observable through `@brink-lang/web` — the diagnostic renders in
the studio's Problems panel for both the db-direct and off-db analysis
roads.
