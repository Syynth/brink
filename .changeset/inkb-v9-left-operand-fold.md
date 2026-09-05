---
"@brink-lang/web": patch
---

`.inkb` format v9: the left-operand-fold superinstructions

The bytecode format gains `GetTempBinaryImm` (`0x70`),
`GetTempBinaryImmJumpIfFalse` (`0x71`) and `DuplicateBinaryImmJumpIfFalse`
(`0x74`) — the v8 fused binary forms with the instruction that produced
their left operand folded in (`docs/optimizer-peephole.md` §1). The
compiler never writes them, so `compile` output is unchanged in shape — but
every `.inkb` now carries version 9, and a stored v8 artifact handed to
`StoryRunner` or `linesTableOf` is rejected with an unsupported-version
error and must be recompiled. The program model renders them as
`get_temp_binary_imm <slot> kind=<op> <imm>`,
`get_temp_binary_imm_jump_if_false <slot> kind=<op> <imm> <rel>` and
`duplicate_binary_imm_jump_if_false kind=<op> <imm> <rel>`.
