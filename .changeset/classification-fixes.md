---
"@brink-lang/web": minor
"@brink-lang/editor": minor
---

Line-classification fixes (#478) — deliberate behavior changes to the
`line_contexts` contract and `LineInfo`:

- A choice line with an inline divert (`* [Go] -> hub`) now classifies as
  `choice` (was `divert`), so Tab/Enter smart-editing transitions work on
  it again.
- Every gather-label line — continuation labels, `LabeledBlock` labels,
  top-level labeled gathers — uniformly classifies as `gather` with
  `gather_continuation` weave at its sigil depth. Previously a labeled
  block with an inline divert showed `divert` while the visually identical
  continuation form showed `gather`.
- Choices inside conditional/sequence branches report their sigil depth
  (was 0), so depth-dependent transitions and gutter depth markers work
  inside arms.
- Blank lines inside a choice body inherit the body weave (element stays
  `blank`); the editor maps them to `ChoiceBody` so Tab works anywhere in
  the body — replacing the old single-shape TS post-pass, and covering
  deeper blank runs it missed.
