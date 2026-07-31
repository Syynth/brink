---
"@brink-lang/web": patch
---

Analyzer: `E164`/`E165` now point at the exact markup span, not the whole
content line (issue #1782).

`hir::SpanPart` carries its own `Provenance` (a new `NodeClass::Span`),
stamped from the span's `SPAN` syntax node during native lowering. Markup
vocabulary diagnostics (`E164` undeclared tag, `E165` undeclared attribute)
now anchor to that per-span range instead of falling back to the enclosing
content line's range (or, inside a choice's display text, the enclosing
choice's range).

Two consumer-visible effects: a content line with several undeclared spans
now gets one squiggle per span instead of several identical whole-line
squiggles, and repeating the same undeclared tag twice on one line now
produces two diagnostics with distinct ranges instead of two byte-identical
ones. Diagnostic *codes* and *message text* are unchanged; only `range`
narrows.

Analyzer-side only — `LinePart::Span` (the `.inkb`/`.inkl` wire shape) is
untouched, since `E164`/`E165` are emitted during HIR analysis, before LIR
lowering/codegen ever runs.
