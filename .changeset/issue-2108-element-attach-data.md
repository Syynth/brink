---
"@brink-lang/web": patch
---

`brink-runtime`: `OutputLine.element.data` is now populated for `attach =
StructName` convention handlers (issue #2108, the element output model
ruled 2026-08-03). An attaching convention (`cue`, `parenthetical`) consumes
its own claimed line — no `Step::Line`/event for it at all — and its
returned struct's fields merge into the run that follows, with every line
materialized while the run is open carrying a copy. `Element.kind` is
unchanged (`"narrative"` regardless); classifying `kind` itself for a
non-attach single-line handler (`heading`/`transition`) remains unbuilt.

Two new bytecode opcodes (`AttachElement`/`EndElementRun`) and two new
`OutputPart` variants carry this — the latter deliberately transient
(never reach the persisted `.brkt` transcript format, matching
`Checkpoint`'s existing precedent), so this is in-memory-only for now; a
save/resume story for `Element.data` has not been designed.

`brink-web` re-exports `OutputLine`/`Element` through the same marshal
legs #1684 built (`LineJs`/`ElementJs`, `@brink-lang/web`'s
`Line`/`SessionLine` TS types) — a `.brink` project using `@[convention(...,
attach = StructName)]` now sees non-empty `element.data` on the wasm
surface for the first time. The disassembler view (`program_model.rs`)
also gained the two new opcodes' mnemonics (`attach_element`/
`end_element_run`).
