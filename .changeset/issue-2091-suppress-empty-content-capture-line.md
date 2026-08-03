---
"@brink-lang/web": patch
---

Issue #2091: an empty `content`/Fragment interpolation no longer renders
its own blank output line. This covers TWO distinct call sites that
produce a `Value::FragmentRef` in this position, and the fix cannot
distinguish between them — both are suppressed identically:

- a `block`-capturing handler (issues #1838/#1839) whose captured run is
  empty — most commonly a cue immediately followed by a parenthetical,
  where `hir::lower_native::element::capture_block`'s terminator ends the
  run at zero interior lines; and
- an ordinary **display-position call-composition** slot — e.g. a line
  whose only content is `{ f() }` — where `brink-codegen-inkb::content::
  emit_slot_expr`'s `BeginFragment`…`EndFragment` wrapping (emitted for
  *every* template slot whose expr is a function call, both dialects) also
  produces a `Value::FragmentRef`, and `f` emitted no side-effect text and
  returned an empty value.

Either way, interpolating that fragment alone on a template line used to
still consume a visible blank line between real content, both in
`continue_single`'s streaming `Line`-at-a-time API and in
`continue_maximally`/`flush_lines`'s batch form.

Fixed at the output-resolution layer
(`brink-runtime::output::resolve_lines`/`take_first_line`), not at the
line table: a resolved line is suppressed only when its text comes out
empty, it carries no tags, *and* at least one of its parts interpolated a
`content`-typed value that itself rendered empty (not necessarily
"captured nothing" — a captured line that itself renders empty, or a
call-composition fragment whose function simply returned `""`, both reach
the same state). The compiled line-table entry a suppressed line's
`LineRef` points at is untouched — present-but-empty, not omitted or
renumbered — so locale hot-swap (which matches a swapped-in line vector to
the transcript by index) keeps working unchanged.

Deliberately scoped to exactly this case: a line that resolves empty for
any other reason — a literal blank line, or a self-closing inline markup
span (`<pause/>`) with no children — still renders its pre-existing blank
beat (see the `inline-markup-point-marker` fixture, issue #1716), which
this issue explicitly treats as a separate, already-settled question.

`tests/tier1-native/conventions-screenplay-preset/`'s golden fixture
(from issue #1720/PR #2081, whose `expected.txt` had pinned the stray
blank line as-is) is updated to reflect the corrected output.
