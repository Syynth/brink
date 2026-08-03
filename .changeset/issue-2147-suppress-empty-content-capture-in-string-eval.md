---
"@brink-lang/web": patch
---

Issue #2147 (gap 1 of #2091's follow-through review): the empty-`content`/
Fragment blank-line suppression PR #2140 added to
`brink-runtime::output::{resolve_lines, take_first_line}` did not extend to
`OutputBuffer::end_capture`'s string-capture path (`resolve_parts`) — the
`Opcode::EndStringEval` resolution an unrecognized choice display or any
`~ temp x = "..."` string-eval capture rides. A blank line contributed
purely by an empty `content`/Fragment interpolation inside a captured
string still rendered, inconsistent with the streaming/batch path.

`resolve_parts` now applies the same per-line suppression: a line within
the captured text is dropped entirely (not left behind as a blank line) when
it resolves fully empty and at least one of its parts interpolated a
`Value::FragmentRef` that itself rendered empty — same invariant, same
scope boundary (a non-`FragmentRef` empty slot still keeps its blank line)
as the existing `resolve_lines`/`take_first_line` fix.
