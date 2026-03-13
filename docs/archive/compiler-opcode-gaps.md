# Finding: Compiler Opcode Gaps

**Date:** 2026-03-12
**Area:** brink-codegen-inkb, brink-ir (LIR)
**Severity:** Moderate — may cause episode failures for stories using these constructs

## Summary

Three opcodes are emitted by the converter but never by the compiler. The runtime handles all three correctly, so any story exercising these paths will work via the converter pipeline but may fail or produce different behavior through the compiler pipeline.

## Gaps

### 1. `GotoIf` (0x43) — Conditional divert

**What it does:** Pops a value from the stack; if truthy, performs a goto to the target container. Used for conditional diverts in ink (e.g., `{condition: -> target}`).

**Converter:** Emits `GotoIf(id)` in `codegen.rs:331` when a conditional divert follows an expression evaluation.

**Compiler:** Never emits this opcode. The compiler likely lowers conditional diverts to a different pattern (e.g., `JumpIfFalse` over a `Goto`), which is semantically equivalent but structurally different. This could cause `.inkt` dump mismatches even when behavior is correct — needs verification.

**Impact:** If the compiler's alternative lowering is correct, this is a non-issue for behavior but will cause structural divergence in dumps. If the alternative lowering has edge cases (e.g., stack state differences), it could cause real failures.

### 2. `ThreadStart` (0x58) — Thread lifecycle marker

**What it does:** Nothing — the runtime treats it as a no-op (vm.rs:124). It's a structural marker in the bytecode stream.

**Converter:** Emits `ThreadStart` in `codegen.rs:263` when processing the `ControlCommand::Thread` JSON command.

**Compiler:** The LIR has a `Stmt::ThreadStart` but the codegen translates it to `ThreadCall(id)` (container.rs:48-54), which is the actual thread-forking opcode. The `Opcode::ThreadStart` marker is never emitted.

**Impact:** Low — since the runtime no-ops `ThreadStart`, omitting it is behaviorally invisible. However, it causes structural divergence in `.inkt` dumps.

### 3. `TurnIndex` (0x82) — Current turn number intrinsic

**What it does:** Pushes the current turn index (0-based) onto the stack. Used by `TURNS()` in ink.

**Converter:** Emits `TurnIndex` in `codegen.rs:259` when processing `ControlCommand::Turn`.

**Compiler:** Never emits this opcode. If any test case uses the `TURNS()` function, it will fail through the compiler pipeline.

**Impact:** Any story using `TURNS()` will fail. Need to check whether this is exercised in the episode corpus.

## Recommendations

1. **TurnIndex** is the most likely to cause real failures — check the corpus for `TURNS()` usage and implement emission in the compiler if needed.
2. **GotoIf** — verify that the compiler's alternative lowering is behaviorally equivalent. If it is, this is cosmetic. If not, add `GotoIf` emission.
3. **ThreadStart** — lowest priority. Could add it for dump-compatibility but it's behaviorally invisible.
