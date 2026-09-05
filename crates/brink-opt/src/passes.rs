//! The resident passes, in the order `OptConfig::defaults` runs them.

use brink_format::{Opcode, StoryData};

use crate::peephole::{Instr, Labels, Rewrite, rewrite_story};
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
    fn try_at(&self, instrs: &[Instr], i: usize, labels: &Labels) -> Option<(usize, Vec<Opcode>)> {
        let Opcode::EmitLine(idx, slots) = instrs.get(i)?.op else {
            return None;
        };
        let next = instrs.get(i + 1)?;
        if !matches!(next.op, Opcode::EmitNewline) || labels.contains(next.offset) {
            return None;
        }
        Some((2, vec![Opcode::EmitLineNl(idx, slots)]))
    }
}
