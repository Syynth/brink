# Optimizer pass: peephole (superinstructions)

**Status: LANDED (engine + two passes), 2026-09-05.** The catalogue's
`peephole` entry (`docs/optimizer-catalogue.md`), graduated per the per-pass
template. This document covers the shared rewriting engine
(`crates/brink-opt/src/peephole.rs`) and the rewrites that ride on it
(`crates/brink-opt/src/passes.rs`): `emit-line-nl` (pass 1, `.inkb` v7) and
`binary-fusion` (pass 2, `.inkb` v8). Later rewrites are additions to §1's
tables, not new documents.

## 0. Why superinstructions, and why here

The runtime spends roughly **450 instructions of host work per bytecode
instruction** on the three reference stories (`benchmarks/stories/crucible-8`,
`hanoi-10`, `tests/tier3/misc/TheIntercept`), and most of that is fetch,
decode and dispatch rather than the opcode's own body. Fewer bytecode
instructions for the same work is therefore the most direct lever the artifact
has, and the bigram histogram (`brink-loop --histogram`, #3575) says exactly
which pairs to fuse: `EmitLine → EmitNewline` is 8.9% of every instruction
TheIntercept executes and 3.6% of hanoi's.

A superinstruction is a **new opcode whose body is the two original bodies in
sequence**. It changes the artifact's instruction stream and nothing an
author or host can observe. It lives in the post-compile optimizer because
bytecode does not exist before codegen, and because the compiler's own
emission stays one-instruction-per-construct — the fused forms are an
optimizer's vocabulary, never something codegen writes.

## 1. Behavior

### The engine

A pass is a `Rewrite`: given one container's decoded instruction list and a
position, it may replace a **window** of instructions with a shorter sequence.
The engine does everything that makes such a replacement legal on a real
artifact, once, for every rewrite:

| Concern | What the engine does |
|---|---|
| **Labels** | Every byte offset anything jumps to — a relative `Jump`, `JumpIfFalse` or `SequenceBranch` target, or an `AddressDef.byte_offset` inside the container — must survive as an instruction boundary. A window may *begin* at a label; a window that would *swallow* one is refused. |
| **Relocation** | Replacing a window shifts everything after it. Kept relative jumps are re-encoded against the new layout (`relative_of(new_end, map(old_target))`); the container's `AddressDef` offsets move with their instruction; `DebugInfo` entries follow the instruction they annotated, and an entry on a swallowed instruction lands on the window's replacement — the nearest thing a debugger can still point at. A replacement that itself ends in a branch (`Emit::Branch`) names its target as an absolute offset in the old code and is re-encoded the same way. |
| **Refusal to guess** | A container whose bytecode stops decoding is left byte-for-byte as it was. The VM will report the same error it always did. |
| **Order** | Containers and instructions are visited in program order; non-overlapping windows are planned left to right, so every decision is a pure function of the artifact. |

Relative jump operands are relative to the **end** of the jump instruction
(`docs/format-spec.md`), which is what `relative_target`/`relative_of` encode.

### `emit-line-nl`

| | |
|---|---|
| Window | `EmitLine(idx, slots)` immediately followed by `EmitNewline`, with no label on the `EmitNewline` |
| Replacement | `EmitLineNl(idx, slots)` — opcode `0x6C`, `.inkb` **v7** |
| Runtime | `vm.rs` runs `emit_line` then `emit_newline` — the two original arms, factored into helpers so the fused arm cannot drift from the unfused pair |
| Metric | `opcodes` executed (the `Stats` counter every `brink-loop` run prints), and `bytecode_bytes` in `ArtifactStats` |

The label refusal is what keeps control flow that lands *between* the line
and its newline correct: an `AddressDef` at the `EmitNewline`'s offset, or a
`Jump`/`JumpIfFalse` whose target is that offset, leaves the pair unfused. The
engine's unit tests pin both shapes.

### `binary-fusion`

A binary operator fused with the `PushInt` that feeds its right operand
and/or the `JumpIfFalse` that consumes its result — the shape of every
`if x <= 1`, `{ x == 3: }` and `x - 1` in real stories. Longest window
first, at each position:

| Window | Replacement | Opcode |
|---|---|---|
| `PushInt(imm); op; JumpIfFalse(rel)` | `BinaryImmJumpIfFalse(kind, imm, rel′)` | `0x6F` |
| `PushInt(imm); op` | `BinaryImm(kind, imm)` | `0x6D` |
| `op; JumpIfFalse(rel)` | `BinaryJumpIfFalse(kind, rel′)` | `0x6E` |

`op` is any operator `BinaryKind` names (`add sub mul div mod eq ne gt ge lt
le` — exactly the operators with a plain two-operand opcode), carried as one
byte. The runtime arm is the constituent bodies in sequence through the
*same* helpers the plain opcodes use: `value_ops::binary_op` with
`Value::Int(imm)` as the right operand, and `jump_unless` — the tail of
`JumpIfFalse`, factored so the fused branch cannot drift from the plain one.
The only behavioural difference is that a fused branch's comparison result
never touches the value stack, which nothing can observe.

A label on the `JumpIfFalse` **shortens the window rather than blocking
it**: the immediate still fuses and the branch stays a separate instruction,
which is why the pass checks `Labels::blocks_window` itself instead of
leaving the refusal to the engine. Metric: `opcodes` executed, as for pass 1.

## 2. Constraints

The catalogue named three collisions. Each was checked against the format,
not assumed:

- **Choice indices** (#3527, observable). A choice's index is
  `pending_choices.len()` at the moment the VM pushes it — a count of choices
  produced, not a bytecode offset. The rewrite removes no choice instruction
  and reorders nothing, so the numbering is untouched.
- **Effect rows.** `EffectRowEntry` is keyed by `DefinitionId` and holds no
  bytecode offset, so relocation cannot invalidate a row, and a
  fused emit performs exactly the two original `note_effect_emit` observations
  in the same order.
- **Debug offsets.** Handled by relocation above. `DebugEntry.bytecode_offset`
  is per `DebugInfoSection.containers[idx]`, and `AddressDef.byte_offset` is
  per `container_id`; the engine relocates both tables per container.

Two further observables, both from `docs/optimizer-spec.md` §10's findings:

- **`.inkl` overlays** carry the `.inkb` header CRC as `base_checksum`, so
  *any* byte change invalidates every existing overlay. This pass changes
  bytes. Optimization precedes localization; that ordering is the rule, not
  something this pass can relax.
- **Line tables and `source_hash`** are untouched — `emit-line-nl` carries
  the `idx` operand through verbatim and `binary-fusion` never looks at a
  line table, so `line_identity_diff` is clean by construction and every
  translation still anchors.
- **Value semantics.** `BinaryImm` builds `Value::Int(imm)` and hands it to
  the same `binary_op` the unfused pair would have reached after `PushInt`,
  so int/float promotion, string comparison, list arithmetic and every error
  path are the unfused ones. The fusion is over *instruction boundaries*,
  never over the operator's semantics.

And the constraint the whole design sits under:

- **Hot patching.** The optimizer rewrites `StoryData` *before* linking. The
  linked `Program`'s operand rewrite (`LinkTables`, 2026-03-01 two-layer
  design) happens after, on whatever instruction stream it is given, and a
  re-link over a patched artifact sees the same `EmitLineNl` any fresh link
  does. Nothing in the fused form is link-state.

What the passes **do not** do, deliberately: fuse across a label
(correctness, above); touch a container they cannot decode; or rewrite the
compiler's emission. Codegen never writes `0x6C`–`0x6F`; `brink-format`'s
reader accepts them in any v8 artifact, so a hand-written `.inkt` may use
`emit_line_nl` or `binary_imm kind=le 1` directly (the operator is spelled
`kind=<mnemonic>`, a `kv_operand`, because a bare `add` would parse as a
trailing operand of the previous line and swallow the next instruction's
mnemonic — #3273's hazard).

## 3. Generator

The input shapes — **a line of prose followed by its newline**, and **a
comparison or arithmetic against an integer literal, usually under a
conditional** — are things every generated story already holds, so no new
knob was needed. The property is the standard pair, in
`brink-gen/tests/opt_equivalence.rs`:

1. traces and line identity agree between the optimized and unoptimized
   artifact, and the pass is idempotent and stable — the fence's four
   obligations, through the same `judge()` seam the corpus uses;
2. **the metric moved**: `changed` and `bytes_identical` must disagree on
   every case, and at least half of the generated cases must have been
   rewritten. That second half is what catches a pass that has silently
   stopped matching.

The corpus fence (`opt_corpus_fence.rs`) carries the same two-way check plus
a change floor per sweep (150 of tier1–3's 390 cases; 10 of tier1-native's
29). Measured on landing: 297 and 24 rewritten with pass 1 alone; 335 and 26
with pass 2 resident.

## 4. Measured

`brink-loop --opt` runs the resident passes over the artifact before linking
and prints each pass's report and the bytecode delta. Per-iteration Ir is
measured by differencing two iteration counts, so the compile and the
optimizer's own fixed cost drop out.

Pass 1 alone (`emit-line-nl`):

| Story | Fusions | Bytecode bytes | Opcodes executed | Ir per iteration |
|---|---|---|---|---|
| TheIntercept | 659 | 21 169 → 20 510 | 789 → 719 (−8.9%) | 707 737 → 697 607 (−1.4%) |
| hanoi-10 | 15 | 1 244 → 1 229 | 2 427 871 → 2 340 369 (−3.6%) | 1 308.8M → 1 291.1M (−1.4%) |
| crucible-8 | 55 | 3 482 → 3 427 | 466 239 → 465 862 (−0.1%) | 177.30M → 177.27M (−0.02%) |

Both passes resident (`emit-line-nl` + `binary-fusion`), against the
unoptimized artifact:

| Story | Fusions (p1 + p2) | Bytecode bytes | Opcodes executed | Ir per iteration |
|---|---|---|---|---|
| crucible-8 | 55 + 67 | 3 482 → 3 394 | 466 239 → 359 890 (−22.8%) | 178.57M → 149.98M (−16.0%) |
| hanoi-10 | 15 + 16 | 1 244 → 1 215 | 2 427 871 → 2 063 151 (−15.0%) | 1 314.7M → 1 220.8M (−7.1%) |
| TheIntercept | 659 + 55 | 21 169 → 20 482 | 789 → 697 (−11.7%) | 709 108 → 690 826 (−2.6%) |

Each fused instruction saves roughly 150–270 Ir — the dispatch overhead of
one instruction plus, for the branch forms, a push/pop pair that no longer
happens. Crucible is arithmetic-bound: `fib`'s `n <= 1`, `n - 1` and `n - 2`
are exactly the three windows, and its 106 349 removed dispatches per
iteration are 3 per call.

The optimizer's own cost is a one-off per artifact: about 10M Ir for
TheIntercept's 21 KB of bytecode with both passes, linear in the artifact
(addresses are grouped by container once, the old→new offset map is a binary
search, and a kept instruction is copied verbatim rather than re-encoded).

## 5. Next rewrites on the same engine

From the bigram histogram **after both passes**, in order of measured
frequency on the reference stories:

| Pair | Share | Fused form |
|---|---|---|
| `GetTemp → BinaryImm`, `GetTemp → BinaryImmJumpIfFalse` | crucible 9.7% each | fold the local read into the fused op: `GetTempBinaryImm(slot, kind, imm)` and its branch form — 3→1 and 4→1 on the original shapes |
| `Duplicate → BinaryImmJumpIfFalse` | hanoi 4.9% | the switch-style conditional (`{ x: - 1: … - 2: … }`); a `Duplicate`-folding branch form |
| `Call → DeclareTemp`, `EnterContainer → EmitNewline`, fragment wrapping | crucible 9.7% each, hanoi 5.2% + 15% | compiler-side shapes, not passes — see §6 |

Each is one `Rewrite` impl plus its opcode; the label and relocation rules
(including branches inside replacements) are already paid for.

## 6. Questions for the ruling

1. **Opcode budget.** Each fused form spends one of the unreserved opcode
   bytes and one `VERSION` bump. RULED 2026-09-05: acceptable — new opcodes
   may be added to the format as passes need them. Pass 2 spent three (v8),
   using a `BinaryKind` operand byte rather than one opcode per operator so
   the format's opcode table grows by families rather than by operators.
2. **Shapes that belong to the compiler.** `DeclareTemp` once per parameter
   on every `Call`, the six-instruction fragment wrapping around call slots,
   and a leading `EmitNewline` on every conditional-arm container are codegen
   choices, not instruction pairs; fusing them would paper over emission the
   compiler should not be doing. Open: which of these the compiler fixes, and
   in which order.
3. **`brink opt` subcommand.** Still deferred: `brink-cli` is publishable and
   `brink-opt` is `publish = false` (`docs/optimizer-spec.md` §10). The pass
   is real now, so hand-publishing the crate is worth doing; the subcommand
   lands with that.
