---
"@brink-lang/web": patch
---

`LineFlags::from_template` (`brink-format`) now walks past leading/trailing
empty-string `LinePart::Literal`s instead of inspecting only `parts.first()`/
`parts.last()` by position when computing `STARTS_WITH_WS`/`ENDS_WITH_WS`.
Previously such a part silently defeated the check, since an empty literal
neither starts nor ends with whitespace, even when a whitespace-carrying
part followed/preceded it. `Slot`/`Select` parts remain conservative by
design (their resolved content isn't known at compile time).

`LineFlags` is recomputed at `.inkb` decode time (`decode_line_entry`), not
stored on the wire, so this is a pure decode-time correctness fix — no
format version bump. No current runtime path consumes `STARTS_WITH_WS`/
`ENDS_WITH_WS` outside of tests yet, so this has no observable rendering
effect today.

Note: per `docs/prose-dialect-spec.md` §4.4, the future `LinePart::Span`
kind is a nested `{ name, attrs, children }` variant, not a zero-width
marker — correct flag computation for it will require recursing into
`children`, which this patch does not do. A second patch is required when
spans land; this fix only closes the empty-`Literal` case.
