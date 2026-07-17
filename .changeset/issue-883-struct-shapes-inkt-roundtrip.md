---
"@brink-lang/web": patch
---

Fixed a `.inkt` dump-parity bug (#883, the #742/#871 class): the
`struct_shapes` section (TM-4 struct/record shape declarations) was fully
round-tripped through the binary `.inkb` format but silently dropped
entirely by the `.inkt` textual dump — neither written nor read, despite
the module doc's claim that every `StoryData` field is represented. A
compiled story containing `STRUCT` declarations now shows its
`struct_shapes` section in the `.inkt` debug view (`program_inkt()`,
surfaced in brink-studio's compiled-output panel) instead of it vanishing.

Also added a structural exhaustiveness guard to `brink-format`'s
`proptest_inkt` suite: a match over every `Opcode`/`Value` variant with no
wildcard arm, so a future variant added to either enum without matching
generator coverage fails to compile instead of silently escaping fuzz
coverage — the mechanical fix for this recurring bug class (tracked from
#397).
