//! The resident passes, in the order `OptConfig::defaults` runs them.

use brink_format::{BinaryKind, Opcode, StoryData};

use crate::peephole::{Emit, Instr, Labels, Rewrite, rewrite_story};
use crate::{Pass, PassOutcome};

/// Fuse `EmitLine` immediately followed by `EmitNewline` into
/// `EmitLineNl` — the single most common instruction pair on real stories
/// (`docs/optimizer-peephole.md`). One dispatch instead of two per line of
/// prose, with the runtime executing the two original bodies in sequence.
pub struct EmitLineNl;

impl EmitLineNl {
    pub const NAME: &'static str = "emit-line-nl";
}

impl Pass for EmitLineNl {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn run(&self, story: &mut StoryData) -> PassOutcome {
        let fused = rewrite_story(story, self);
        PassOutcome::changed("lines fused with their newline", fused)
    }
}

impl Rewrite for EmitLineNl {
    fn try_at(&self, instrs: &[Instr], i: usize, _labels: &Labels) -> Option<(usize, Vec<Emit>)> {
        let Opcode::EmitLine(idx, slots) = instrs.get(i)?.op else {
            return None;
        };
        matches!(instrs.get(i + 1)?.op, Opcode::EmitNewline)
            .then(|| (2, vec![Emit::Op(Opcode::EmitLineNl(idx, slots))]))
    }
}

/// Fuse a binary operator with the `PushInt` that feeds its right operand
/// and/or the `JumpIfFalse` that consumes its result
/// (`docs/optimizer-peephole.md` §1) — the shape of every `if x <= 1`,
/// `{ x == 3: }` and `x - 1` in real stories. Longest window first:
///
/// | window | replacement |
/// |---|---|
/// | `PushInt(imm); op; JumpIfFalse` | `BinaryImmJumpIfFalse(kind, imm, ·)` |
/// | `PushInt(imm); op` | `BinaryImm(kind, imm)` |
/// | `op; JumpIfFalse` | `BinaryJumpIfFalse(kind, ·)` |
///
/// where `op` is any operator `BinaryKind` names. A label on the
/// `JumpIfFalse` shortens the window rather than blocking it: the
/// immediate can still fuse, the branch stays a separate instruction.
pub struct BinaryFusion;

impl BinaryFusion {
    pub const NAME: &'static str = "binary-fusion";
}

impl Pass for BinaryFusion {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn run(&self, story: &mut StoryData) -> PassOutcome {
        let fused = rewrite_story(story, self);
        PassOutcome::changed(
            "binary operators fused with an immediate or a branch",
            fused,
        )
    }
}

impl Rewrite for BinaryFusion {
    fn try_at(&self, instrs: &[Instr], i: usize, labels: &Labels) -> Option<(usize, Vec<Emit>)> {
        let first = instrs.get(i)?;
        if let Opcode::PushInt(imm) = first.op {
            let kind = BinaryKind::of_opcode(&instrs.get(i + 1)?.op)?;
            if labels.blocks_window(instrs, i, 2) {
                return None;
            }
            if let Some(jump) = instrs.get(i + 2)
                && let Opcode::JumpIfFalse(_) = jump.op
                && !labels.blocks_window(instrs, i, 3)
            {
                let target = jump.jump_target()?;
                let op = Opcode::BinaryImmJumpIfFalse(kind, imm, 0);
                return Some((3, vec![Emit::Branch { op, target }]));
            }
            return Some((2, vec![Emit::Op(Opcode::BinaryImm(kind, imm))]));
        }
        let kind = BinaryKind::of_opcode(&first.op)?;
        let jump = instrs.get(i + 1)?;
        let Opcode::JumpIfFalse(_) = jump.op else {
            return None;
        };
        let target = jump.jump_target()?;
        let op = Opcode::BinaryJumpIfFalse(kind, 0);
        Some((2, vec![Emit::Branch { op, target }]))
    }
}

/// Fold the instruction that produced a fused binary operator's *left*
/// operand into the operator (`docs/optimizer-peephole.md` §1). Runs after
/// [`BinaryFusion`], on its output — the order in `OptConfig::defaults` is
/// load-bearing:
///
/// | window | replacement |
/// |---|---|
/// | `GetTemp(slot); BinaryImmJumpIfFalse(kind, imm, ·)` | `GetTempBinaryImmJumpIfFalse(slot, kind, imm, ·)` |
/// | `GetTemp(slot); BinaryImm(kind, imm)` | `GetTempBinaryImm(slot, kind, imm)` |
/// | `Duplicate; BinaryImmJumpIfFalse(kind, imm, ·)` | `DuplicateBinaryImmJumpIfFalse(kind, imm, ·)` |
///
/// On the original shapes that is `n - 1` in one instruction instead of
/// three and `if n <= 1` in one instead of four. The runtime's fused arms
/// read the temp through the same `read_temp` as `GetTemp` and peek the
/// stack exactly as `Duplicate` + pop would have.
pub struct LeftOperandFold;

impl LeftOperandFold {
    pub const NAME: &'static str = "left-operand-fold";
}

impl Pass for LeftOperandFold {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn run(&self, story: &mut StoryData) -> PassOutcome {
        let fused = rewrite_story(story, self);
        PassOutcome::changed("left operands folded into a fused operator", fused)
    }
}

impl Rewrite for LeftOperandFold {
    fn try_at(&self, instrs: &[Instr], i: usize, _labels: &Labels) -> Option<(usize, Vec<Emit>)> {
        let first = instrs.get(i)?;
        let next = instrs.get(i + 1)?;
        let emit = match (&first.op, &next.op) {
            (Opcode::GetTemp(slot), Opcode::BinaryImmJumpIfFalse(kind, imm, _)) => Emit::Branch {
                op: Opcode::GetTempBinaryImmJumpIfFalse(*slot, *kind, *imm, 0),
                target: next.jump_target()?,
            },
            (Opcode::GetTemp(slot), Opcode::BinaryImm(kind, imm)) => {
                Emit::Op(Opcode::GetTempBinaryImm(*slot, *kind, *imm))
            }
            (Opcode::Duplicate, Opcode::BinaryImmJumpIfFalse(kind, imm, _)) => Emit::Branch {
                op: Opcode::DuplicateBinaryImmJumpIfFalse(*kind, *imm, 0),
                target: next.jump_target()?,
            },
            _ => return None,
        };
        Some((2, vec![emit]))
    }
}
