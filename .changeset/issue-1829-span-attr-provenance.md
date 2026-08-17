---
"@brink-lang/web": patch
---

Analyzer: `E165` now points at the exact undeclared attribute, not the
whole enclosing span (issue #1829).

`hir::SpanPart::attrs` now carries per-attribute `Provenance` (a new
`SpanAttr` type, `NodeClass::SpanAttr`), stamped from each attribute's
`SPAN_ATTR` syntax node during native lowering — the attribute-axis
counterpart of #1782/#1820's per-span fix. `E165` (undeclared attribute)
now anchors to that per-attribute range instead of falling back to the
whole enclosing span's range.

Consumer-visible effect: a span carrying several undeclared attributes now
gets one squiggle per attribute instead of several identical whole-span
squiggles, and repeating the same undeclared attribute name twice on one
span now produces two diagnostics with distinct ranges instead of two
byte-identical ones. Diagnostic *codes* and *message text* are unchanged;
only `range` narrows. `E164` and `E173` are unaffected: `E164` never had
this collapse (it is span-, not attribute-, scoped), and `E173` (a
*missing* required attribute) has no attribute node in source to point at,
so it stays span-ranged.

Analyzer-side only — `LinePart::Span` (the `.inkb`/`.inkl` wire shape)
keeps its flat `Vec<(String, String)>` attrs, since `E165` is emitted
during HIR analysis, before LIR lowering/codegen ever runs.
