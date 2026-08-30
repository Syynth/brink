---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

The Program Explorer becomes one instrument with four views

Structure, Line tables, Disassembly and Size behind one segmented switch
(#3339), with a shared identity header and one execution thread through
all of them.

Structure: knot rows with size bars (bytecode + lines on a shared
scale), externals stating their contract (fallback vs host), totals in
the footer. Line tables: the compiled lines scoped as the compiler
scopes them, template slots and selects as chips reading like prose,
source cells as line-numbered links that convert byte offsets to the
editor's UTF-16 before revealing. Disassembly: every operand resolves —
emit_line to its line text (linking into the Line tables view), globals
to live values while paused, jumps to their landing offset, externals to
their binding contract — with per-instruction source provenance from the
DebugInfo section and stepi beside the code it steps. Size: a squarified
treemap of real on-disk section bytes, with an exact "shipping only"
re-flow showing what a release export strips.

New runner-free surfaces on `@brink-lang/web`: `linesTableOf`,
`sizeReportOf`, per-scope `byte_size`/`container_count` and anonymous
child containers (labeled by their real weave-label names) on the
program model, and per-instruction `src` provenance.
