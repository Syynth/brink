//! #3273 stage 1, end to end: a hand-built `lir::Program` carrying
//! `StmtKind::EmitLineVariants` → `brink_codegen_inkb::emit` →
//! `brink_runtime::link` → run. Proves the whole stack — the enumerated
//! line table + group records, the TouchVisit/ShuffleIndexOf advance, the
//! combo fold, and the switch — behaves as ink's alternative semantics
//! demand (#3271's shape: alternatives on one line co-advance).
//!
//! Hand-built because no lowering path constructs the statement until the
//! stage-2 flip (#3274); this test IS the stage-1 consumer. All runs are
//! straight-line bodies — nothing here can hang.

#![expect(clippy::unwrap_used, reason = "test harness")]

use brink_format::{DefinitionId, DefinitionTag};
use brink_ir::lir;
use brink_ir::{NodeClass, Provenance, TextRange};
use brink_runtime::{DotNetRng, Step, Story};

fn ptr() -> Provenance {
    Provenance::synthetic(NodeClass::Sequence, TextRange::empty(0.into()))
}

fn plain(text: &str) -> lir::ContentEmission {
    lir::ContentEmission {
        line: lir::RecognizedLine::Plain(text.to_string()),
        metadata: lir::LineMetadata {
            source_hash: brink_format::content_hash(text),
            slot_info: Vec::new(),
            source_location: None,
        },
        tags: Vec::new(),
    }
}

fn stub_alt(id: DefinitionId) -> lir::Container {
    lir::Container {
        id,
        provenance: ptr(),
        name: None,
        kind: lir::ContainerKind::Sequence,
        params: Vec::new(),
        body: Vec::new(),
        children: Vec::new(),
        counting_flags: brink_format::CountingFlags::VISITS,
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
        local: false,
    }
}

fn program(body: Vec<lir::Stmt>, alts: Vec<lir::Container>) -> lir::Program {
    lir::Program {
        root: lir::Container {
            id: DefinitionId::new(DefinitionTag::Address, 1),
            provenance: ptr(),
            name: None,
            kind: lir::ContainerKind::Root,
            params: Vec::new(),
            body,
            children: alts,
            counting_flags: brink_format::CountingFlags::empty(),
            temp_slot_count: 0,
            labeled: false,
            inline: false,
            is_function: false,
            local: false,
        },
        globals: Vec::new(),
        lists: Vec::new(),
        list_items: Vec::new(),
        externals: Vec::new(),
        name_table: Vec::new(),
        struct_shapes: Vec::new(),
        private_defs: Vec::new(),
        aliases: Vec::new(),
        file_paths: std::collections::BTreeMap::new(),
    }
}

fn run(data: &brink_format::StoryData) -> String {
    let (prog, line_tables) = brink_runtime::link(data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(prog), line_tables);
    let mut out = String::new();
    for _ in 0..64 {
        match story.continue_single().unwrap() {
            Step::Line(line) => out.push_str(&line.text),
            Step::Done | Step::End | Step::Suspended => return out,
            Step::Choices(_) => unreachable!("no choices in these fixtures"),
        }
    }
    unreachable!("fixture did not terminate");
}

/// Two stopping alternatives on one line, viewed three times (#3271's
/// exact shape). Ink's documented semantics: both advance on EVERY view —
/// `a x`, then `b y`, then `b y`. (Main's cartesian clones give `b x` on
/// view 2 — the conformance bug this machinery exists to fix.)
#[test]
fn two_stopping_alternatives_co_advance() {
    let alt_a = DefinitionId::new(DefinitionTag::Address, 100);
    let alt_b = DefinitionId::new(DefinitionTag::Address, 101);
    let emission = lir::VariantLineEmission {
        alts: vec![
            lir::VariantAltEmission {
                container_id: alt_a,
                kind: brink_ir::SequenceType::STOPPING,
                branch_count: 2,
            },
            lir::VariantAltEmission {
                container_id: alt_b,
                kind: brink_ir::SequenceType::STOPPING,
                branch_count: 2,
            },
        ],
        dims: vec![2, 2],
        variants: vec![
            plain("a x\n"),
            plain("a y\n"),
            plain("b x\n"),
            plain("b y\n"),
        ],
    };
    // Three views = the same statement three times: each execution IS a
    // line viewing, advancing the shared containers.
    let body = (0..3)
        .map(|_| lir::Stmt::new(lir::StmtKind::EmitLineVariants(emission.clone()), ptr()))
        .collect();
    let data =
        brink_codegen_inkb::emit(&program(body, vec![stub_alt(alt_a), stub_alt(alt_b)])).unwrap();

    // Three statements = three groups, each dims [2,2] over its own run.
    assert_eq!(data.line_variant_groups.len(), 3);
    for (i, group) in data.line_variant_groups.iter().enumerate() {
        assert_eq!(group.dims, vec![2, 2]);
        assert_eq!(
            group.base,
            u32::try_from(i * 4).unwrap(),
            "consecutive 4-entry runs"
        );
    }

    // The output buffer trims the story-final newline — assert on lines.
    assert_eq!(run(&data), "a x\nb y\nb y");
}

/// A `once` alternative exhausts into its EXTRA variant — the empty
/// rendering at dim position N — while its stopping neighbor keeps
/// advancing normally.
#[test]
fn once_exhausts_into_the_extra_variant() {
    let alt_once = DefinitionId::new(DefinitionTag::Address, 100);
    let emission = lir::VariantLineEmission {
        alts: vec![lir::VariantAltEmission {
            container_id: alt_once,
            kind: brink_ir::SequenceType::ONCE,
            branch_count: 2,
        }],
        dims: vec![3],
        variants: vec![plain("Hi there!\n"), plain("Hi again!\n"), plain("Hi!\n")],
    };
    let body = (0..4)
        .map(|_| lir::Stmt::new(lir::StmtKind::EmitLineVariants(emission.clone()), ptr()))
        .collect();
    let data = brink_codegen_inkb::emit(&program(body, vec![stub_alt(alt_once)])).unwrap();
    assert_eq!(
        run(&data),
        "Hi there!\nHi again!\nHi!\nHi!",
        "views 3+ pin to the exhausted empty variant"
    );
}

/// A cycle wraps.
#[test]
fn cycle_wraps() {
    let alt = DefinitionId::new(DefinitionTag::Address, 100);
    let emission = lir::VariantLineEmission {
        alts: vec![lir::VariantAltEmission {
            container_id: alt,
            kind: brink_ir::SequenceType::CYCLE,
            branch_count: 2,
        }],
        dims: vec![2],
        variants: vec![plain("tick\n"), plain("tock\n")],
    };
    let body = (0..5)
        .map(|_| lir::Stmt::new(lir::StmtKind::EmitLineVariants(emission.clone()), ptr()))
        .collect();
    let data = brink_codegen_inkb::emit(&program(body, vec![stub_alt(alt)])).unwrap();
    assert_eq!(run(&data), "tick\ntock\ntick\ntock\ntick");
}

/// A shuffle alternative: over one full loop every variant appears exactly
/// once, and the draw is deterministic for a fixed story.
#[test]
fn shuffle_draws_a_deterministic_permutation() {
    let alt = DefinitionId::new(DefinitionTag::Address, 100);
    let emission = lir::VariantLineEmission {
        alts: vec![lir::VariantAltEmission {
            container_id: alt,
            kind: brink_ir::SequenceType::SHUFFLE,
            branch_count: 4,
        }],
        dims: vec![4],
        variants: vec![plain("p\n"), plain("q\n"), plain("r\n"), plain("s\n")],
    };
    let body = (0..4)
        .map(|_| lir::Stmt::new(lir::StmtKind::EmitLineVariants(emission.clone()), ptr()))
        .collect();
    let data = brink_codegen_inkb::emit(&program(body, vec![stub_alt(alt)])).unwrap();
    let out = run(&data);
    let mut lines: Vec<&str> = out.lines().collect();
    assert_eq!(run(&data), out, "deterministic");
    lines.sort_unstable();
    assert_eq!(lines, vec!["p", "q", "r", "s"], "full loop = permutation");
}

/// Asymmetric dims (stopping×2 by cycle×3) pin the row-major fold:
/// variant (i, j) must land at `i * 3 + j` — a radix slip that symmetric
/// fixtures can't see selects a wrong-but-in-range leaf here.
#[test]
fn asymmetric_dims_pin_the_row_major_fold() {
    let alt_a = DefinitionId::new(DefinitionTag::Address, 100);
    let alt_b = DefinitionId::new(DefinitionTag::Address, 101);
    let emission = lir::VariantLineEmission {
        alts: vec![
            lir::VariantAltEmission {
                container_id: alt_a,
                kind: brink_ir::SequenceType::STOPPING,
                branch_count: 2,
            },
            lir::VariantAltEmission {
                container_id: alt_b,
                kind: brink_ir::SequenceType::CYCLE,
                branch_count: 3,
            },
        ],
        dims: vec![2, 3],
        variants: vec![
            plain("v00\n"),
            plain("v01\n"),
            plain("v02\n"),
            plain("v10\n"),
            plain("v11\n"),
            plain("v12\n"),
        ],
    };
    // Views 0..5: stopping index min(k,1), cycle index k % 3.
    let body = (0..5)
        .map(|_| lir::Stmt::new(lir::StmtKind::EmitLineVariants(emission.clone()), ptr()))
        .collect();
    let data =
        brink_codegen_inkb::emit(&program(body, vec![stub_alt(alt_a), stub_alt(alt_b)])).unwrap();
    assert_eq!(
        run(&data),
        "v00\nv11\nv12\nv10\nv11",
        "(0,0) (1,1) (1,2) (1,0) (1,1)"
    );
}
