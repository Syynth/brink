---
"@brink-lang/web": patch
---

`.inkb` format v7: the `EmitLineNl` superinstruction

The bytecode format gains `EmitLineNl` (opcode `0x6C`), the fused form of
`EmitLine` followed by `EmitNewline` that the post-compile optimizer
(`brink-opt`, `docs/optimizer-peephole.md`) emits. The compiler itself never
writes it, so artifacts produced by `compile` are unchanged in shape — but
every `.inkb` now carries version 7, and a stored v6 artifact handed to
`StoryRunner` or `linesTableOf` is rejected with an unsupported-version
error and must be recompiled. The program model renders the new opcode as
`emit_line_nl #idx slots`.
