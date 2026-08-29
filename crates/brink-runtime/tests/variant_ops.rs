//! #3273 stage 1: VM semantics of the two line-variant-group opcodes,
//! [`Opcode::TouchVisit`] and [`Opcode::ShuffleIndexOf`], driven over
//! hand-assembled bytecode — no lowering path emits them until the
//! stage-2 flip (#3274), so hand assembly IS the consumer proof (a
//! registry entry with no live caller is exactly the #3193 hazard).
//!
//! Programs are hand-built `StoryData`; every run is a straight-line
//! bytecode body ending in `Done`, so nothing here can hang.

#![expect(
    clippy::unwrap_used,
    clippy::cast_possible_wrap,
    reason = "test harness"
)]

use brink_format::{
    ContainerDef, CountingFlags, DefinitionId, DefinitionTag, LineContent, LineEntry, NameId,
    Opcode, ScopeLineTable, StoryData,
};
use brink_runtime::{DotNetRng, Step, Story};

fn def_id(tag: DefinitionTag, n: u64) -> DefinitionId {
    DefinitionId::new(tag, n)
}

/// Assemble opcodes into a bytecode vec.
fn assemble(ops: &[Opcode]) -> Vec<u8> {
    let mut buf = Vec::new();
    for op in ops {
        op.encode(&mut buf);
    }
    buf
}

/// A story with one root container running `root_ops`, plus `alts` stub
/// containers (never entered — identity, visit state, and `path_hash`
/// carriers, exactly the shape stage 2's shared alternatives compile to).
fn story_with(root_ops: &[Opcode], alts: &[(DefinitionId, i32)]) -> StoryData {
    let root_id = def_id(DefinitionTag::Address, 1);
    let mut containers = vec![ContainerDef {
        id: root_id,
        scope_id: root_id,
        name: Some(NameId(0)),
        bytecode: assemble(root_ops),
        counting_flags: CountingFlags::empty(),
        path_hash: 0,
        param_count: 0,
        params: vec![],
        local: false,
    }];
    for (id, path_hash) in alts {
        containers.push(ContainerDef {
            id: *id,
            scope_id: root_id,
            name: None,
            bytecode: Vec::new(),
            counting_flags: CountingFlags::VISITS,
            path_hash: *path_hash,
            param_count: 0,
            params: vec![],
            local: false,
        });
    }
    StoryData {
        containers,
        line_tables: vec![ScopeLineTable {
            scope_id: root_id,
            lines: vec![LineEntry {
                content: LineContent::Plain(String::new()),
                flags: brink_format::LineFlags::from_plain(""),
                source_hash: 0,
                audio_ref: None,
                slot_info: Vec::new(),
                source_location: None,
            }],
        }],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        address_paths: vec![],
        name_table: vec!["root".to_string()],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        private_defs: vec![],
        alias_table: vec![],
        effect_rows: vec![],
        frame_shapes: Vec::new(),
        debug_info: None,
        line_variant_groups: Vec::new(),
        source_checksum: 0,
    }
}

/// Run a straight-line story to its terminal, returning all emitted text.
fn run(data: &StoryData) -> String {
    let (program, line_tables) = brink_runtime::link(data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut out = String::new();
    // Bounded: a straight-line body cannot yield more steps than opcodes.
    for _ in 0..64 {
        match story.continue_single().unwrap() {
            Step::Line(line) => out.push_str(&line.text),
            Step::Done | Step::End | Step::Suspended => return out,
            Step::Choices(_) => unreachable!("no choices in these fixtures"),
        }
    }
    unreachable!("fixture did not terminate");
}

/// Emit `n` as its own line: value then newline.
fn emit_target_touch(alt: DefinitionId) -> [Opcode; 4] {
    [
        Opcode::PushDivertTarget(alt),
        Opcode::TouchVisit,
        Opcode::EmitValue,
        Opcode::EmitNewline,
    ]
}

/// TouchVisit hands back the PRE-increment count — 0 on the first view —
/// and each touch advances it: the "how many times has this alternative
/// been viewed" index, recorded without ever entering the container.
#[test]
fn touch_visit_returns_pre_increment_and_advances() {
    let alt = def_id(DefinitionTag::Address, 100);
    let mut ops = Vec::new();
    for _ in 0..3 {
        ops.extend(emit_target_touch(alt));
    }
    ops.push(Opcode::Done);
    let out = run(&story_with(&ops, &[(alt, 7)]));
    assert_eq!(out, "0\n1\n2\n", "three touches = view indices 0, 1, 2");
}

/// Two DISTINCT alternatives advance independently — the whole point of
/// shared per-construct state (#3271: today's clones advance per-branch
/// instead, which is the conformance bug).
#[test]
fn touch_visit_state_is_per_container() {
    let a = def_id(DefinitionTag::Address, 100);
    let b = def_id(DefinitionTag::Address, 101);
    let mut ops = Vec::new();
    ops.extend(emit_target_touch(a));
    ops.extend(emit_target_touch(b));
    ops.extend(emit_target_touch(a));
    ops.push(Opcode::Done);
    let out = run(&story_with(&ops, &[(a, 1), (b, 2)]));
    assert_eq!(out, "0\n0\n1\n", "a: 0 then 1; b: 0 — independent counters");
}

/// A malformed operand (not a DivertTarget) pushes 0 and records nothing,
/// mirroring `VisitCount`'s tolerance: malformed bytecode degrades, never
/// panics.
#[test]
fn touch_visit_tolerates_a_non_target_operand() {
    let alt = def_id(DefinitionTag::Address, 100);
    let ops = [
        Opcode::PushInt(42),
        Opcode::TouchVisit,
        Opcode::EmitValue,
        Opcode::EmitNewline,
        // A real touch after the malformed one still starts at 0 — the
        // malformed touch recorded nothing.
        Opcode::PushDivertTarget(alt),
        Opcode::TouchVisit,
        Opcode::EmitValue,
        Opcode::EmitNewline,
        Opcode::Done,
    ];
    let out = run(&story_with(&ops, &[(alt, 0)]));
    assert_eq!(out, "0\n0\n");
}

/// Emit the shuffle index of `alt` for a given `seq_count`, as a line.
fn emit_shuffle_index(alt: DefinitionId, seq_count: i32, num_elements: i32) -> [Opcode; 6] {
    [
        Opcode::PushInt(seq_count),
        Opcode::PushInt(num_elements),
        Opcode::PushDivertTarget(alt),
        Opcode::ShuffleIndexOf,
        Opcode::EmitValue,
        Opcode::EmitNewline,
    ]
}

/// The selection is a permutation: over one full loop (`seq_count` 0..n
/// with n elements), every index appears exactly once — the same
/// partial-Fisher–Yates contract as `Sequence(Shuffle)`.
#[test]
fn shuffle_index_of_yields_a_permutation() {
    let alt = def_id(DefinitionTag::Address, 100);
    const N: i32 = 4;
    let mut ops = Vec::new();
    for seq_count in 0..N {
        ops.extend(emit_shuffle_index(alt, seq_count, N));
    }
    ops.push(Opcode::Done);
    let out = run(&story_with(&ops, &[(alt, 1234)]));
    let mut seen: Vec<i32> = out.lines().map(|l| l.parse::<i32>().unwrap()).collect();
    seen.sort_unstable();
    assert_eq!(seen, vec![0, 1, 2, 3], "one loop visits each branch once");
}

/// Determinism: the same story (same seed, same path_hash) picks the same
/// permutation every run.
#[test]
fn shuffle_index_of_is_deterministic() {
    let alt = def_id(DefinitionTag::Address, 100);
    let mut ops = Vec::new();
    for seq_count in 0..4 {
        ops.extend(emit_shuffle_index(alt, seq_count, 4));
    }
    ops.push(Opcode::Done);
    let data = story_with(&ops, &[(alt, 1234)]);
    assert_eq!(run(&data), run(&data));
}

/// The seed is the NAMED container's `path_hash` — two alternatives with
/// different hashes may draw different permutations, and two with the
/// SAME hash always draw the same one. The current-container form cannot
/// make that distinction (both alternatives on a line share the line's
/// container), which is why this opcode exists (#3273).
#[test]
fn shuffle_index_of_seeds_by_the_named_containers_hash() {
    let a = def_id(DefinitionTag::Address, 100);
    let b = def_id(DefinitionTag::Address, 101);
    let c = def_id(DefinitionTag::Address, 102);
    const N: i32 = 8;
    // One full loop for each of a (hash 111), b (hash 222), c (hash 111).
    let mut ops = Vec::new();
    for alt in [a, b, c] {
        for seq_count in 0..N {
            ops.extend(emit_shuffle_index(alt, seq_count, N));
        }
    }
    ops.push(Opcode::Done);
    let out = run(&story_with(&ops, &[(a, 111), (b, 222), (c, 111)]));
    let nums: Vec<i32> = out.lines().map(|l| l.parse().unwrap()).collect();
    let (pa, rest) = nums.split_at(N as usize);
    let (pb, pc) = rest.split_at(N as usize);
    assert_eq!(pa, pc, "equal path_hash => identical permutation");
    assert_ne!(
        pa, pb,
        "distinct path_hash must decorrelate the permutations (with 8 \
         elements, identical draws would be a 1-in-40320 seed collision — \
         if this ever flakes, the seeding is broken, not the test)"
    );
}

/// An id that resolves to no container seeds with hash 0 — degrade, not
/// panic, same posture as every other malformed-bytecode path.
#[test]
fn shuffle_index_of_tolerates_an_unknown_target() {
    let ghost = def_id(DefinitionTag::Address, 999);
    let mut ops = Vec::new();
    ops.extend(emit_shuffle_index(ghost, 0, 3));
    ops.push(Opcode::Done);
    let out = run(&story_with(&ops, &[]));
    let n: i32 = out.trim().parse().unwrap();
    assert!((0..3).contains(&n), "index still in range, got {n}");
}
