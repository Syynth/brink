---
"@brink-lang/web": patch
---

`.inkb` format v8: the fused binary superinstructions

The bytecode format gains `BinaryImm` (`0x6D`), `BinaryJumpIfFalse` (`0x6E`)
and `BinaryImmJumpIfFalse` (`0x6F`), each carrying a `BinaryKind` operator
byte — the post-compile optimizer's fusion of a binary operator with the
`PushInt` immediate feeding it and/or the `JumpIfFalse` consuming it
(`docs/optimizer-peephole.md` §1). The compiler never writes them, so
`compile` output is unchanged in shape — but every `.inkb` now carries
version 8, and a stored v7 artifact handed to `StoryRunner` or
`linesTableOf` is rejected with an unsupported-version error and must be
recompiled. The program model renders the new opcodes as
`binary_imm kind=le 1`, `binary_jump_if_false kind=eq <rel>` and
`binary_imm_jump_if_false kind=le 1 <rel>`.
