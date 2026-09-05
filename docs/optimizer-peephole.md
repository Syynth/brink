# Optimizer pass: peephole (superinstructions)

**Status: LANDED (engine + first rewrite), 2026-09-05.** The catalogue's
`peephole` entry (`docs/optimizer-catalogue.md`), graduated per the per-pass
template. This document covers the shared rewriting engine
(`crates/brink-opt/src/peephole.rs`) and the first rewrite that rides on it,
`emit-line-nl` (`crates/brink-opt/src/passes.rs`). Later rewrites — the
compare/branch fusions below — are additions to §1's table, not new
documents.

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
| **Relocation** | Replacing a window shifts everything after it. Kept relative jumps are re-encoded against the new layout (`relative_of(new_end, map(old_target))`); the container's `AddressDef` offsets move with their instruction; `DebugInfo` entries follow the instruction they annotated, and an entry on a swallowed instruction lands on the window's replacement — the nearest thing a debugger can still point at. |
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
- **Line tables and `source_hash`** are untouched — the rewrite carries the
  `idx` operand through verbatim, so `line_identity_diff` is clean by
  construction and every translation still anchors.

And the constraint the whole design sits under:

- **Hot patching.** The optimizer rewrites `StoryData` *before* linking. The
  linked `Program`'s operand rewrite (`LinkTables`, 2026-03-01 two-layer
  design) happens after, on whatever instruction stream it is given, and a
  re-link over a patched artifact sees the same `EmitLineNl` any fresh link
  does. Nothing in the fused form is link-state.

What the pass **does not** do, deliberately: fuse across a label (correctness,
above); touch a container it cannot decode; or rewrite the compiler's
emission. Codegen never writes `0x6C`; `brink-format`'s reader accepts it in
any v7 artifact, so a hand-written `.inkt` may use `emit_line_nl` directly.

## 3. Generator

The pass's input shape is **any line of prose followed by its newline**,
which every generated story already holds, so no new knob was needed. The
property is the standard pair, in `brink-gen/tests/opt_equivalence.rs`:

1. traces and line identity agree between the optimized and unoptimized
   artifact, and the pass is idempotent and stable — the fence's four
   obligations, through the same `judge()` seam the corpus uses;
2. **the metric moved**: `changed` and `bytes_identical` must disagree on
   every case, and at least half of the generated cases must have been
   rewritten. That second half is what catches a pass that has silently
   stopped matching.

The corpus fence (`opt_corpus_fence.rs`) carries the same two-way check plus
a change floor per sweep (150 of tier1–3's 390 cases; 10 of tier1-native's
29). Measured on landing: 297 and 24 rewritten.

## 4. Measured

`brink-loop --opt` runs the resident passes over the artifact before linking
and prints each pass's report and the bytecode delta. Per-iteration Ir is
measured by differencing two iteration counts, so the compile and the
optimizer's own fixed cost drop out.

| Story | Fusions | Bytecode bytes | Opcodes executed | Ir per iteration |
|---|---|---|---|---|
| TheIntercept | 659 | 21 169 → 20 510 | 789 → 719 (−8.9%) | 707 737 → 697 607 (−1.4%) |
| hanoi-10 | 15 | 1 244 → 1 229 | 2 427 871 → 2 340 369 (−3.6%) | 1 308.8M → 1 291.1M (−1.4%) |
| crucible-8 | 55 | 3 482 → 3 427 | 466 239 → 465 862 (−0.1%) | 177.30M → 177.27M (−0.02%) |

Each fused pair saves roughly 150–200 Ir — the dispatch overhead of one
instruction, which is the whole point. Crucible is arithmetic-bound and
barely emits prose; its pairs are the next rewrites' targets.

The optimizer's own cost is a one-off per artifact: about 6.5M Ir for
TheIntercept's 21 KB of bytecode, linear in the artifact (addresses are
grouped by container once, the old→new offset map is a binary search, and a
kept instruction is copied verbatim rather than re-encoded).

## 5. Next rewrites on the same engine

From the bigram histogram, in order of measured frequency on the reference
stories:

| Pair | Share | Fused form |
|---|---|---|
| `GetTemp → PushInt` | crucible 14.9% | needs a two-operand form |
| `PushInt → {Subtract, LessOrEqual, Equal}` | crucible 7.4% each, hanoi 5.4% | compare/arith-with-immediate |
| `{LessOrEqual, Equal} → JumpIfFalse` | crucible 7.4%, hanoi 5.4% | compare-and-branch |
| `Call → DeclareTemp`, `EnterContainer → EmitNewline` | crucible 7.5% each | compiler-side shapes, not passes — see §6 |

Each is one `Rewrite` impl plus its opcode; the label and relocation rules are
already paid for.

## 6. Questions for the ruling

1. **Opcode budget.** Each fused form spends one of the unreserved opcode
   bytes and one `VERSION` bump. RULED 2026-09-05: acceptable — new opcodes
   may be added to the format as passes need them.
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
