//! Instruction-level rewriting with relocation — the machinery every
//! peephole pass shares (`docs/optimizer-peephole.md`).
//!
//! A pass expresses itself as a [`Rewrite`]: given the decoded instruction
//! list of one container and a position, it may replace a window of
//! instructions with a shorter sequence. Everything that makes such a
//! replacement legal on a real artifact lives here, once:
//!
//! - **Labels.** A byte offset that anything jumps to — a relative `Jump`,
//!   `JumpIfFalse` or `SequenceBranch` target, or an `AddressDef` inside the
//!   container — must survive as an instruction boundary. A window may begin
//!   at a label; it may never swallow one, so the rewrite is refused there.
//! - **Relocation.** Replacing a window changes the length of everything
//!   after it. Relative jumps are re-encoded against the new layout, the
//!   container's `AddressDef` byte offsets move with their instruction, and
//!   `DebugInfo` entries follow the instruction they annotated (an entry on
//!   a swallowed instruction lands on the window's replacement — the
//!   nearest thing the debugger can still point at).
//! - **Refusal to guess.** A container whose bytecode stops decoding is left
//!   exactly as it was: the VM will report the same error it always did.
//!
//! Determinism: containers and instructions are visited in program order,
//! and every decision is a pure function of the artifact.

use std::collections::{BTreeMap, BTreeSet};

use brink_format::{DefinitionId, Opcode, StoryData};

/// One decoded instruction and where it sat in the original bytecode.
#[derive(Debug, Clone)]
pub(crate) struct Instr {
    pub offset: usize,
    pub len: usize,
    pub op: Opcode,
}

impl Instr {
    fn end(&self) -> usize {
        self.offset + self.len
    }

    /// The absolute target of a relative jump, if this is one.
    pub(crate) fn jump_target(&self) -> Option<usize> {
        jump_rel(&self.op).map(|rel| relative_target(self.end(), rel))
    }
}

/// The relative operand of every jump-bearing opcode — the plain jumps and
/// the fused superinstructions that end in one. Every relative offset is
/// taken from the end of the instruction that carries it.
fn jump_rel(op: &Opcode) -> Option<i32> {
    match *op {
        Opcode::Jump(rel)
        | Opcode::JumpIfFalse(rel)
        | Opcode::SequenceBranch(rel)
        | Opcode::BinaryJumpIfFalse(_, rel)
        | Opcode::BinaryImmJumpIfFalse(_, _, rel)
        | Opcode::GetTempBinaryImmJumpIfFalse(_, _, _, rel)
        | Opcode::DuplicateBinaryImmJumpIfFalse(_, _, rel) => Some(rel),
        _ => None,
    }
}

/// `op` with its relative operand replaced. Callers pass an `op` for which
/// [`jump_rel`] is `Some`; anything else comes back unchanged.
fn with_jump_rel(op: &Opcode, rel: i32) -> Opcode {
    match *op {
        Opcode::Jump(_) => Opcode::Jump(rel),
        Opcode::JumpIfFalse(_) => Opcode::JumpIfFalse(rel),
        Opcode::SequenceBranch(_) => Opcode::SequenceBranch(rel),
        Opcode::BinaryJumpIfFalse(kind, _) => Opcode::BinaryJumpIfFalse(kind, rel),
        Opcode::BinaryImmJumpIfFalse(kind, imm, _) => Opcode::BinaryImmJumpIfFalse(kind, imm, rel),
        Opcode::GetTempBinaryImmJumpIfFalse(slot, kind, imm, _) => {
            Opcode::GetTempBinaryImmJumpIfFalse(slot, kind, imm, rel)
        }
        Opcode::DuplicateBinaryImmJumpIfFalse(kind, imm, _) => {
            Opcode::DuplicateBinaryImmJumpIfFalse(kind, imm, rel)
        }
        _ => op.clone(),
    }
}

/// `end + rel` in `usize` space; a relative offset always lands inside or
/// exactly at the end of the container that carries it.
fn relative_target(end: usize, rel: i32) -> usize {
    if rel >= 0 {
        end.saturating_add(rel.unsigned_abs() as usize)
    } else {
        end.saturating_sub(rel.unsigned_abs() as usize)
    }
}

/// Decode a container's whole bytecode. `None` if any instruction fails to
/// decode — the caller leaves such a container untouched.
pub(crate) fn decode_all(code: &[u8]) -> Option<Vec<Instr>> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < code.len() {
        let start = offset;
        let op = Opcode::decode(code, &mut offset).ok()?;
        out.push(Instr {
            offset: start,
            len: offset - start,
            op,
        });
    }
    Some(out)
}

/// The byte offsets that must remain instruction boundaries.
pub(crate) struct Labels(BTreeSet<usize>);

impl Labels {
    pub(crate) fn contains(&self, offset: usize) -> bool {
        self.0.contains(&offset)
    }

    /// Whether a window of `consumed` instructions starting at `instrs[i]`
    /// would swallow a label — a window may *begin* at one, never erase one.
    pub(crate) fn blocks_window(&self, instrs: &[Instr], i: usize, consumed: usize) -> bool {
        instrs[i + 1..i + consumed]
            .iter()
            .any(|instr| self.contains(instr.offset))
    }
}

/// One instruction of a replacement. A replacement that ends in a branch
/// names its target as an **absolute offset in the old code**; the engine
/// re-encodes it against the new layout, exactly as it does for the jumps
/// it kept.
pub(crate) enum Emit {
    Op(Opcode),
    Branch { op: Opcode, target: usize },
}

impl Emit {
    fn op(&self) -> &Opcode {
        match self {
            Self::Op(op) | Self::Branch { op, .. } => op,
        }
    }
}

/// A local rewrite. `try_at` looks at `instrs[i..]` and either returns the
/// number of instructions to replace together with their replacement, or
/// `None` to leave `instrs[i]` alone. A window may not swallow a label —
/// the engine refuses one that would, so a rewrite that has a shorter legal
/// window to fall back to should check `labels.blocks_window` itself and
/// offer that instead. An `Emit::Op` may not carry a relative jump; a
/// branch in a replacement is an `Emit::Branch` with its absolute target.
pub(crate) trait Rewrite {
    fn try_at(&self, instrs: &[Instr], i: usize, labels: &Labels) -> Option<(usize, Vec<Emit>)>;
}

/// Apply `rewrite` to every container of `story`, relocating as described
/// in the module docs. Returns the number of windows replaced.
pub(crate) fn rewrite_story(story: &mut StoryData, rewrite: &dyn Rewrite) -> usize {
    // `AddressDef`s grouped by owning container once, so the per-container
    // work below is linear in the container rather than in the whole table.
    let mut addresses_of: BTreeMap<DefinitionId, Vec<usize>> = BTreeMap::new();
    for (i, addr) in story.addresses.iter().enumerate() {
        addresses_of.entry(addr.container_id).or_default().push(i);
    }

    let mut replaced = 0;
    for (idx, container) in story.containers.iter_mut().enumerate() {
        let Some(instrs) = decode_all(&container.bytecode) else {
            continue;
        };
        let owned = addresses_of
            .get(&container.id)
            .map_or(&[][..], Vec::as_slice);
        let labels = labels_of(
            &instrs,
            owned
                .iter()
                .map(|&i| story.addresses[i].byte_offset as usize),
        );
        let (plan, windows_replaced) = plan_windows(&instrs, &labels, rewrite);
        if windows_replaced == 0 {
            continue;
        }
        replaced += windows_replaced;

        let layout = Layout::of(&instrs, &plan, container.bytecode.len());
        container.bytecode = emit(&container.bytecode, &instrs, &plan, &layout);

        // Relocate the tables that name byte offsets in this container.
        for &i in owned {
            let addr = &mut story.addresses[i];
            addr.byte_offset = offset_u32(layout.map(addr.byte_offset as usize));
        }
        if let Some(debug) = story.debug_info.as_mut()
            && let Some(table) = debug.containers.get_mut(idx)
        {
            for entry in &mut table.entries {
                entry.bytecode_offset = offset_u32(layout.map(entry.bytecode_offset as usize));
            }
        }
    }
    replaced
}

/// Decide, left to right, which original instructions each new window
/// replaces and what it becomes. Returns the plan and how many windows are
/// real rewrites (`consumed > 1` or a changed body).
fn plan_windows(instrs: &[Instr], labels: &Labels, rewrite: &dyn Rewrite) -> (Vec<Window>, usize) {
    let mut plan: Vec<Window> = Vec::with_capacity(instrs.len());
    let mut replaced = 0;
    let mut i = 0;
    while i < instrs.len() {
        match rewrite.try_at(instrs, i, labels) {
            Some((consumed, emits))
                if consumed > 0
                    && i + consumed <= instrs.len()
                    && !labels.blocks_window(instrs, i, consumed) =>
            {
                debug_assert!(
                    emits
                        .iter()
                        .all(|e| !matches!(e, Emit::Op(op) if jump_rel(op).is_some())),
                    "a replacement branch must be an Emit::Branch with its target"
                );
                plan.push(Window {
                    first: i,
                    consumed,
                    body: Body::Replace(emits),
                });
                replaced += 1;
                i += consumed;
            }
            _ => {
                plan.push(Window {
                    first: i,
                    consumed: 1,
                    body: Body::Keep,
                });
                i += 1;
            }
        }
    }
    (plan, replaced)
}

/// Where each window lands in the new code, and the old→new offset map.
struct Layout {
    /// `(old_start, old_end, new_start)` per window, in order.
    spans: Vec<(usize, usize, usize)>,
    old_len: usize,
    new_len: usize,
}

impl Layout {
    fn of(instrs: &[Instr], plan: &[Window], old_len: usize) -> Self {
        let mut spans = Vec::with_capacity(plan.len());
        let mut cursor = 0;
        for window in plan {
            let old_start = instrs[window.first].offset;
            let old_end = instrs[window.first + window.consumed - 1].end();
            spans.push((old_start, old_end, cursor));
            cursor += window.new_len(instrs);
        }
        Self {
            spans,
            old_len,
            new_len: cursor,
        }
    }

    /// An original instruction start maps to its window's new start (a
    /// swallowed instruction to the window that replaced it); the end of the
    /// old code maps to the end of the new.
    fn map(&self, old: usize) -> usize {
        if old >= self.old_len {
            return self.new_len;
        }
        // Spans are in ascending order by construction: the last span that
        // starts at or before `old` is the only one that can contain it.
        let i = self.spans.partition_point(|&(start, _, _)| start <= old);
        match i.checked_sub(1).map(|i| self.spans[i]) {
            Some((_, end, new_start)) if old < end => new_start,
            _ => self.new_len,
        }
    }
}

/// Encode the plan: kept instructions are copied verbatim from `bytecode`
/// (a kept relative jump is re-encoded against `layout`), replacements are
/// encoded fresh.
fn emit(bytecode: &[u8], instrs: &[Instr], plan: &[Window], layout: &Layout) -> Vec<u8> {
    let mut code = Vec::with_capacity(layout.new_len);
    for window in plan {
        match &window.body {
            Body::Keep => {
                let original = &instrs[window.first];
                match original.jump_target() {
                    Some(target_old) => {
                        let new_end = code.len() + original.len;
                        let rel = relative_of(new_end, layout.map(target_old));
                        with_jump_rel(&original.op, rel).encode(&mut code);
                    }
                    None => code.extend_from_slice(&bytecode[original.offset..original.end()]),
                }
            }
            Body::Replace(emits) => {
                for emit in emits {
                    match emit {
                        Emit::Op(op) => op.encode(&mut code),
                        Emit::Branch { op, target } => {
                            let new_end = code.len() + encoded_len(op);
                            let rel = relative_of(new_end, layout.map(*target));
                            with_jump_rel(op, rel).encode(&mut code);
                        }
                    }
                }
            }
        }
    }
    debug_assert_eq!(code.len(), layout.new_len);
    code
}

struct Window {
    first: usize,
    consumed: usize,
    body: Body,
}

/// What a window becomes: the original instruction, or the rewrite's
/// replacement sequence.
enum Body {
    Keep,
    Replace(Vec<Emit>),
}

impl Window {
    fn new_len(&self, instrs: &[Instr]) -> usize {
        match &self.body {
            Body::Keep => instrs[self.first].len,
            Body::Replace(emits) => emits.iter().map(|e| encoded_len(e.op())).sum(),
        }
    }
}

fn labels_of(instrs: &[Instr], addresses: impl Iterator<Item = usize>) -> Labels {
    let mut set: BTreeSet<usize> = instrs.iter().filter_map(Instr::jump_target).collect();
    set.extend(addresses);
    Labels(set)
}

fn encoded_len(op: &Opcode) -> usize {
    let mut buf = Vec::with_capacity(16);
    op.encode(&mut buf);
    buf.len()
}

/// `target - end` as the `i32` a relative jump carries.
fn relative_of(end: usize, target: usize) -> i32 {
    let delta = target as i128 - end as i128;
    i32::try_from(delta).unwrap_or(i32::MAX)
}

fn offset_u32(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {

    use brink_format::{
        AddressDef, ContainerDef, CountingFlags, DebugContainerTable, DebugEntry, DebugInfoSection,
        DefinitionId, DefinitionTag,
    };

    use brink_format::BinaryKind;

    use super::*;
    use crate::passes::{BinaryFusion, EmitLineNl, LeftOperandFold};

    fn id(n: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, n)
    }

    fn encode(ops: &[Opcode]) -> Vec<u8> {
        let mut buf = Vec::new();
        for op in ops {
            op.encode(&mut buf);
        }
        buf
    }

    fn story_with(code: Vec<u8>) -> StoryData {
        StoryData {
            containers: vec![ContainerDef {
                id: id(1),
                scope_id: id(1),
                name: None,
                bytecode: code,
                counting_flags: CountingFlags::empty(),
                path_hash: 0,
                param_count: 0,
                params: Vec::new(),
                local: false,
            }],
            line_tables: Vec::new(),
            variables: Vec::new(),
            list_defs: Vec::new(),
            list_items: Vec::new(),
            externals: Vec::new(),
            addresses: Vec::new(),
            address_paths: Vec::new(),
            name_table: Vec::new(),
            list_literals: Vec::new(),
            literal_pool: Vec::new(),
            struct_shapes: Vec::new(),
            private_defs: Vec::new(),
            alias_table: Vec::new(),
            effect_rows: Vec::new(),
            frame_shapes: Vec::new(),
            debug_info: None,
            line_variant_groups: Vec::new(),
            source_checksum: 0,
        }
    }

    fn ops_of(code: &[u8]) -> Vec<Opcode> {
        decode_all(code)
            .expect("decodes")
            .into_iter()
            .map(|i| i.op)
            .collect()
    }

    /// The instruction each relative jump lands on, in decoded form — the
    /// property relocation must preserve.
    fn jump_landings(code: &[u8]) -> Vec<Option<Opcode>> {
        let instrs = decode_all(code).expect("decodes");
        instrs
            .iter()
            .filter_map(Instr::jump_target)
            .map(|t| instrs.iter().find(|i| i.offset == t).map(|i| i.op.clone()))
            .collect()
    }

    #[test]
    fn fuses_a_line_with_its_newline_and_relocates_a_jump_across_it() {
        // push_bool; jump_if_false -> [past the line]; emit_line; emit_newline; nop
        //                                                                        ^ target
        let line = encode(&[Opcode::EmitLine(3, 0), Opcode::EmitNewline]);
        let code = encode(&[
            Opcode::PushBool(true),
            Opcode::JumpIfFalse(i32::try_from(line.len()).unwrap()),
            Opcode::EmitLine(3, 0),
            Opcode::EmitNewline,
            Opcode::Nop,
        ]);
        let mut story = story_with(code.clone());
        let before = jump_landings(&story.containers[0].bytecode);
        assert_eq!(before, vec![Some(Opcode::Nop)]);

        let replaced = rewrite_story(&mut story, &EmitLineNl);
        assert_eq!(replaced, 1);
        assert_eq!(
            ops_of(&story.containers[0].bytecode),
            vec![
                Opcode::PushBool(true),
                Opcode::JumpIfFalse(4),
                Opcode::EmitLineNl(3, 0),
                Opcode::Nop,
            ]
        );
        assert_eq!(
            jump_landings(&story.containers[0].bytecode),
            before,
            "the jump still lands on the same instruction"
        );

        // Idempotent: a second run finds nothing.
        let again = story.clone();
        assert_eq!(rewrite_story(&mut story, &EmitLineNl), 0);
        assert_eq!(story, again);
    }

    #[test]
    fn backward_jumps_are_relocated_too() {
        // emit_line; emit_newline; push_bool; jump_if_false -> [start]
        let mut story = story_with(encode(&[
            Opcode::EmitLine(0, 0),
            Opcode::EmitNewline,
            Opcode::PushBool(false),
            Opcode::JumpIfFalse(0), // patched below
        ]));
        let code = &mut story.containers[0].bytecode;
        // Target offset 0 from the end of the jump (the whole code length).
        let len = i32::try_from(code.len()).unwrap();
        let patched = encode(&[
            Opcode::EmitLine(0, 0),
            Opcode::EmitNewline,
            Opcode::PushBool(false),
            Opcode::JumpIfFalse(-len),
        ]);
        *code = patched;
        let before = jump_landings(&story.containers[0].bytecode);
        assert_eq!(before, vec![Some(Opcode::EmitLine(0, 0))]);

        assert_eq!(rewrite_story(&mut story, &EmitLineNl), 1);
        assert_eq!(
            jump_landings(&story.containers[0].bytecode),
            vec![Some(Opcode::EmitLineNl(0, 0))],
            "the jump lands on the fused instruction that replaced its target"
        );
    }

    #[test]
    fn a_label_on_the_newline_blocks_the_fusion() {
        let first = encode(&[Opcode::EmitLine(0, 0)]);
        let code = encode(&[Opcode::EmitLine(0, 0), Opcode::EmitNewline]);
        let mut story = story_with(code.clone());
        // Something addresses the newline itself (a gather label, say).
        story.addresses.push(AddressDef {
            id: id(7),
            container_id: id(1),
            byte_offset: u32::try_from(first.len()).unwrap(),
        });
        assert_eq!(rewrite_story(&mut story, &EmitLineNl), 0);
        assert_eq!(story.containers[0].bytecode, code, "untouched");

        // A jump into the newline blocks it just the same.
        let mut story = story_with(encode(&[
            Opcode::Jump(i32::try_from(first.len()).unwrap()),
            Opcode::EmitLine(0, 0),
            Opcode::EmitNewline,
        ]));
        assert_eq!(rewrite_story(&mut story, &EmitLineNl), 0);
    }

    #[test]
    fn addresses_and_debug_entries_follow_their_instructions() {
        let pair = encode(&[Opcode::EmitLine(1, 0), Opcode::EmitNewline]);
        let line_only = encode(&[Opcode::EmitLine(1, 0)]);
        let code = encode(&[
            Opcode::EmitLine(1, 0),
            Opcode::EmitNewline,
            Opcode::Nop,
            Opcode::EmitLine(2, 0),
            Opcode::EmitNewline,
        ]);
        let mut story = story_with(code);
        // A label on the Nop (after the first pair) and one on the second pair.
        story.addresses.push(AddressDef {
            id: id(7),
            container_id: id(1),
            byte_offset: u32::try_from(pair.len()).unwrap(),
        });
        story.addresses.push(AddressDef {
            id: id(8),
            container_id: id(1),
            byte_offset: u32::try_from(pair.len() + 1).unwrap(),
        });
        let entry = |off: usize| DebugEntry {
            bytecode_offset: u32::try_from(off).unwrap(),
            file_idx: 0,
            range_start: 0,
            range_len: 0,
            kind_token: 0,
            flags: 0,
        };
        story.debug_info = Some(DebugInfoSection {
            files: Vec::new(),
            containers: vec![DebugContainerTable {
                entries: vec![
                    entry(0),
                    entry(line_only.len()), // on the first newline: swallowed
                    entry(pair.len()),      // on the Nop
                    entry(pair.len() + 1),  // on the second pair
                ],
                locals: Vec::new(),
            }],
        });

        assert_eq!(rewrite_story(&mut story, &EmitLineNl), 2);
        let fused = encode(&[Opcode::EmitLineNl(1, 0)]);
        assert_eq!(
            story
                .addresses
                .iter()
                .map(|a| a.byte_offset)
                .collect::<Vec<_>>(),
            vec![
                u32::try_from(fused.len()).unwrap(),
                u32::try_from(fused.len() + 1).unwrap()
            ]
        );
        let entries: Vec<u32> = story.debug_info.as_ref().expect("debug").containers[0]
            .entries
            .iter()
            .map(|e| e.bytecode_offset)
            .collect();
        assert_eq!(
            entries,
            vec![
                0,
                0,
                u32::try_from(fused.len()).unwrap(),
                u32::try_from(fused.len() + 1).unwrap()
            ],
            "the swallowed newline's entry lands on the fused instruction"
        );
        // Every relocated offset is an instruction boundary.
        let bounds: BTreeSet<usize> = decode_all(&story.containers[0].bytecode)
            .expect("decodes")
            .iter()
            .map(|i| i.offset)
            .collect();
        for off in entries {
            assert!(
                bounds.contains(&(off as usize)),
                "offset {off} is a boundary"
            );
        }
    }

    #[test]
    fn an_undecodable_container_is_left_alone() {
        let mut code = encode(&[Opcode::EmitLine(1, 0), Opcode::EmitNewline]);
        code.push(0xFF); // no such opcode
        let mut story = story_with(code.clone());
        assert_eq!(rewrite_story(&mut story, &EmitLineNl), 0);
        assert_eq!(story.containers[0].bytecode, code);
    }

    #[test]
    fn fuses_compare_immediate_and_branch_into_one_relocated_instruction() {
        // get_temp 0; push_int 1; less_or_equal; jump_if_false -> [nop]; push_int 7; nop
        let skipped = encode(&[Opcode::PushInt(7)]);
        let mut story = story_with(encode(&[
            Opcode::GetTemp(0),
            Opcode::PushInt(1),
            Opcode::LessOrEqual,
            Opcode::JumpIfFalse(i32::try_from(skipped.len()).unwrap()),
            Opcode::PushInt(7),
            Opcode::Nop,
        ]));
        let before = jump_landings(&story.containers[0].bytecode);
        assert_eq!(before, vec![Some(Opcode::Nop)]);

        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        let ops = ops_of(&story.containers[0].bytecode);
        assert_eq!(ops[0], Opcode::GetTemp(0));
        assert!(
            matches!(
                ops[1],
                Opcode::BinaryImmJumpIfFalse(BinaryKind::LessOrEqual, 1, _)
            ),
            "{ops:?}"
        );
        assert_eq!(&ops[2..], &[Opcode::PushInt(7), Opcode::Nop]);
        assert_eq!(
            jump_landings(&story.containers[0].bytecode),
            before,
            "the fused branch lands where the plain one did"
        );

        let again = story.clone();
        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 0);
        assert_eq!(story, again);
    }

    #[test]
    fn a_label_on_the_branch_shortens_the_window_to_the_immediate() {
        // push_int 3; equal; jump_if_false -> [end]      with an address on the jump
        let prefix = encode(&[Opcode::PushInt(3), Opcode::Equal]);
        let mut story = story_with(encode(&[
            Opcode::PushInt(3),
            Opcode::Equal,
            Opcode::JumpIfFalse(0),
        ]));
        story.addresses.push(AddressDef {
            id: id(9),
            container_id: id(1),
            byte_offset: offset_u32(prefix.len()),
        });

        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        assert_eq!(
            ops_of(&story.containers[0].bytecode),
            vec![
                Opcode::BinaryImm(BinaryKind::Equal, 3),
                Opcode::JumpIfFalse(0),
            ]
        );
        let fused = encode(&[Opcode::BinaryImm(BinaryKind::Equal, 3)]);
        assert_eq!(
            story.addresses[0].byte_offset as usize,
            fused.len(),
            "the address still names the (kept) jump"
        );
    }

    #[test]
    fn a_compare_without_an_immediate_fuses_with_its_branch_alone() {
        // get_temp 0; get_temp 1; equal; jump_if_false -> [nop]; pop; nop  — backward-free
        let skipped = encode(&[Opcode::Pop]);
        let mut story = story_with(encode(&[
            Opcode::GetTemp(0),
            Opcode::GetTemp(1),
            Opcode::Equal,
            Opcode::JumpIfFalse(i32::try_from(skipped.len()).unwrap()),
            Opcode::Pop,
            Opcode::Nop,
        ]));
        let before = jump_landings(&story.containers[0].bytecode);
        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        let ops = ops_of(&story.containers[0].bytecode);
        assert!(
            matches!(ops[2], Opcode::BinaryJumpIfFalse(BinaryKind::Equal, _)),
            "{ops:?}"
        );
        assert_eq!(jump_landings(&story.containers[0].bytecode), before);
    }

    #[test]
    fn a_fused_branch_jumping_backward_over_a_shrunk_region_is_relocated() {
        // [L] emit_line; emit_newline; push_int 0; not_equal; jump_if_false -> L
        let body = encode(&[
            Opcode::EmitLine(0, 0),
            Opcode::EmitNewline,
            Opcode::PushInt(0),
            Opcode::NotEqual,
        ]);
        let jump = encode(&[Opcode::JumpIfFalse(0)]);
        let rel = -i32::try_from(body.len() + jump.len()).unwrap();
        let mut story = story_with(encode(&[
            Opcode::EmitLine(0, 0),
            Opcode::EmitNewline,
            Opcode::PushInt(0),
            Opcode::NotEqual,
            Opcode::JumpIfFalse(rel),
        ]));
        assert_eq!(
            jump_landings(&story.containers[0].bytecode),
            vec![Some(Opcode::EmitLine(0, 0))]
        );
        // Both passes, as the default set runs them: the region before the
        // branch shrinks and the branch itself is fused.
        assert_eq!(rewrite_story(&mut story, &EmitLineNl), 1);
        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        assert_eq!(
            ops_of(&story.containers[0].bytecode)[0],
            Opcode::EmitLineNl(0, 0)
        );
        assert_eq!(
            jump_landings(&story.containers[0].bytecode),
            vec![Some(Opcode::EmitLineNl(0, 0))],
            "the fused backward branch lands on the fused start of the loop"
        );
    }

    #[test]
    fn folds_the_local_read_into_the_fused_compare_and_branch() {
        // fib's test: get_temp 0; push_int 1; less_or_equal; jump_if_false -> [nop]; push_int 7; nop
        let skipped = encode(&[Opcode::PushInt(7)]);
        let mut story = story_with(encode(&[
            Opcode::GetTemp(0),
            Opcode::PushInt(1),
            Opcode::LessOrEqual,
            Opcode::JumpIfFalse(i32::try_from(skipped.len()).unwrap()),
            Opcode::PushInt(7),
            Opcode::Nop,
        ]));
        let before = jump_landings(&story.containers[0].bytecode);

        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        assert_eq!(rewrite_story(&mut story, &LeftOperandFold), 1);
        let ops = ops_of(&story.containers[0].bytecode);
        assert!(
            matches!(
                ops[0],
                Opcode::GetTempBinaryImmJumpIfFalse(0, BinaryKind::LessOrEqual, 1, _)
            ),
            "{ops:?}"
        );
        assert_eq!(&ops[1..], &[Opcode::PushInt(7), Opcode::Nop]);
        assert_eq!(jump_landings(&story.containers[0].bytecode), before);

        // Idempotent: nothing left to fold.
        assert_eq!(rewrite_story(&mut story, &LeftOperandFold), 0);
    }

    #[test]
    fn folds_the_local_read_into_a_fused_immediate_without_a_branch() {
        // n - 1 as a call argument: get_temp 0; push_int 1; subtract; call
        let mut story = story_with(encode(&[
            Opcode::GetTemp(0),
            Opcode::PushInt(1),
            Opcode::Subtract,
            Opcode::Nop,
        ]));
        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        assert_eq!(rewrite_story(&mut story, &LeftOperandFold), 1);
        assert_eq!(
            ops_of(&story.containers[0].bytecode),
            vec![
                Opcode::GetTempBinaryImm(0, BinaryKind::Subtract, 1),
                Opcode::Nop
            ]
        );
    }

    #[test]
    fn folds_a_duplicate_into_the_switch_arm_test() {
        // { x: - 1: ... }: duplicate; push_int 1; equal; jump_if_false -> [nop]; pop; nop
        let skipped = encode(&[Opcode::Pop]);
        let mut story = story_with(encode(&[
            Opcode::Duplicate,
            Opcode::PushInt(1),
            Opcode::Equal,
            Opcode::JumpIfFalse(i32::try_from(skipped.len()).unwrap()),
            Opcode::Pop,
            Opcode::Nop,
        ]));
        let before = jump_landings(&story.containers[0].bytecode);
        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        assert_eq!(rewrite_story(&mut story, &LeftOperandFold), 1);
        let ops = ops_of(&story.containers[0].bytecode);
        assert!(
            matches!(
                ops[0],
                Opcode::DuplicateBinaryImmJumpIfFalse(BinaryKind::Equal, 1, _)
            ),
            "{ops:?}"
        );
        assert_eq!(jump_landings(&story.containers[0].bytecode), before);
    }

    #[test]
    fn a_label_on_the_fused_operator_blocks_the_fold() {
        // Something jumps to the compare itself, so the get_temp before it
        // must stay a separate instruction.
        let mut story = story_with(encode(&[
            Opcode::GetTemp(0),
            Opcode::PushInt(1),
            Opcode::Subtract,
            Opcode::Nop,
        ]));
        assert_eq!(rewrite_story(&mut story, &BinaryFusion), 1);
        let get_temp_len = encode(&[Opcode::GetTemp(0)]).len();
        story.addresses.push(AddressDef {
            id: id(9),
            container_id: id(1),
            byte_offset: u32::try_from(get_temp_len).unwrap(),
        });
        assert_eq!(rewrite_story(&mut story, &LeftOperandFold), 0);
        assert_eq!(
            ops_of(&story.containers[0].bytecode),
            vec![
                Opcode::GetTemp(0),
                Opcode::BinaryImm(BinaryKind::Subtract, 1),
                Opcode::Nop
            ]
        );
    }
}
