---
"@brink-lang/web": patch
---

Retired the dormant `Opcode::SourceLocation` (ruled 2026-07-19, Q-R1): the lossy `line:col` debug carrier the brink compiler never emitted. The Program Explorer's disassembly (`program_model()`) no longer recognizes byte `0xFE` as `source_location LINE:COL` — a bytecode blob carrying that byte now disassembles as a decode error at that offset instead. No compiled program is affected (codegen never emitted this opcode), but the disassembler's behavior for arbitrary/malformed bytecode changed, so this ships as a changeset per house rule. Debug info's real replacement is a new strippable `SectionKind::DebugInfo` section (tag `0x11`), tracked separately under epic #452.
