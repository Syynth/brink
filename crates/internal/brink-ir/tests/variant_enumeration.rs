//! #3273 stage 1: `lir::enumerate_variant_contents` — the recognizer-side
//! enumeration of a content line's inline stateful alternatives into
//! whole-line variants. Drives the pure HIR API directly: no lowering
//! path calls it until the stage-2 flip (#3274), so these tests ARE its
//! consumer.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::{Block, Content, ContentPart, Sequence, SequenceBranch, SequenceType, Stmt};
use brink_ir::lir::{VARIANT_CAP, enumerate_variant_contents};
use brink_ir::{FileId, KindToken, NodeClass, Provenance};

fn ptr() -> Provenance {
    Provenance::new(
        FileId(0),
        rowan::TextRange::new(0.into(), 1.into()),
        KindToken::synthetic(NodeClass::Sequence),
    )
}

fn text_branch(s: &str) -> SequenceBranch {
    SequenceBranch {
        ptr: ptr(),
        body: Block {
            stmts: vec![Stmt::Content(Content {
                ptr: None,
                parts: vec![ContentPart::Text(s.to_string())],
                tags: vec![],
            })],
            ..Block::default()
        },
    }
}

fn seq(kind: SequenceType, branches: &[&str]) -> ContentPart {
    ContentPart::InlineSequence(Sequence {
        ptr: ptr(),
        kind,
        branches: branches.iter().map(|s| text_branch(s)).collect(),
        container_id: None,
        counter_id: None,
    })
}

fn line(parts: Vec<ContentPart>) -> Content {
    Content {
        ptr: None,
        parts,
        tags: vec![],
    }
}

fn texts(e: &brink_ir::lir::VariantEnumeration) -> Vec<String> {
    e.variants
        .iter()
        .map(|c| {
            c.parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text(s) => s.clone(),
                    other => panic!("expected pure text after substitution, got {other:?}"),
                })
                .collect::<String>()
        })
        .collect()
}

/// The #3271 shape: two stopping alternatives, row-major product with the
/// FIRST alternative varying slowest — the `LineVariantGroup` layout
/// contract verbatim.
#[test]
fn two_alternatives_enumerate_the_row_major_product() {
    let content = line(vec![
        ContentPart::Text("X ".into()),
        seq(SequenceType::STOPPING, &["a", "b"]),
        ContentPart::Text(" mid ".into()),
        seq(SequenceType::STOPPING, &["x", "y"]),
        ContentPart::Text(" end.".into()),
    ]);
    let e = enumerate_variant_contents(&content).unwrap().unwrap();
    assert_eq!(e.dims, vec![2, 2]);
    assert_eq!(e.alts.len(), 2);
    assert_eq!(
        texts(&e),
        vec![
            "X a mid x end.",
            "X a mid y end.",
            "X b mid x end.",
            "X b mid y end.",
        ],
        "first alternative slowest: (0,0) (0,1) (1,0) (1,1)"
    );
}

/// A `once` alternative gets one EXTRA variant — the exhausted empty
/// rendering — and its dim says so, so the line table and the runtime's
/// past-the-end index agree on layout.
#[test]
fn once_gets_an_exhausted_empty_variant() {
    let content = line(vec![
        ContentPart::Text("Hi".into()),
        seq(SequenceType::ONCE, &[" there", " again"]),
        ContentPart::Text("!".into()),
    ]);
    let e = enumerate_variant_contents(&content).unwrap().unwrap();
    assert_eq!(e.dims, vec![3], "2 branches + 1 exhausted");
    assert_eq!(e.alts[0].branch_count, 2, "branch_count stays authored");
    assert_eq!(texts(&e), vec!["Hi there!", "Hi again!", "Hi!"]);
}

/// Whitespace collapses at substitution seams exactly as the lift's
/// splice does (`extend_merging_text`'s rule): a branch ending in space
/// meeting a suffix starting with space yields ONE space.
#[test]
fn substitution_merges_whitespace_at_seams() {
    let content = line(vec![
        seq(SequenceType::CYCLE, &["One ", "Two "]),
        ContentPart::Text(" left.".into()),
    ]);
    let e = enumerate_variant_contents(&content).unwrap().unwrap();
    assert_eq!(texts(&e), vec!["One left.", "Two left."]);
}

/// Not variant lines (`Ok(None)` — keep the current path): no
/// alternatives at all; a shuffle-once combination; a structural branch;
/// an inline conditional anywhere on the line; glue.
#[test]
fn inadmissible_shapes_return_none() {
    // Plain text.
    assert!(
        enumerate_variant_contents(&line(vec![ContentPart::Text("Hi.".into())]))
            .unwrap()
            .is_none()
    );

    // shuffle|once combination.
    let combo = SequenceType::SHUFFLE | SequenceType::ONCE;
    assert!(
        enumerate_variant_contents(&line(vec![seq(combo, &["a", "b"])]))
            .unwrap()
            .is_none()
    );

    // A branch with structure (two statements — not a single content line).
    let structural = ContentPart::InlineSequence(Sequence {
        ptr: ptr(),
        kind: SequenceType::STOPPING,
        branches: vec![
            text_branch("a"),
            SequenceBranch {
                ptr: ptr(),
                body: Block {
                    stmts: vec![
                        Stmt::Content(Content {
                            ptr: None,
                            parts: vec![ContentPart::Text("b".into())],
                            tags: vec![],
                        }),
                        Stmt::EndOfLine,
                    ],
                    ..Block::default()
                },
            },
        ],
        container_id: None,
        counter_id: None,
    });
    assert!(
        enumerate_variant_contents(&line(vec![structural]))
            .unwrap()
            .is_none()
    );

    // An inline conditional alongside an alternative.
    let cond = ContentPart::InlineConditional(brink_ir::hir::Conditional {
        ptr: ptr(),
        kind: brink_ir::hir::CondKind::IfElse,
        branches: vec![],
    });
    assert!(
        enumerate_variant_contents(&line(vec![seq(SequenceType::STOPPING, &["a", "b"]), cond]))
            .unwrap()
            .is_none()
    );

    // Glue.
    assert!(
        enumerate_variant_contents(&line(vec![
            seq(SequenceType::STOPPING, &["a", "b"]),
            ContentPart::Glue
        ]))
        .unwrap()
        .is_none()
    );
}

/// The cap: an otherwise-admissible line whose product exceeds
/// [`VARIANT_CAP`] is an ERROR carrying the numbers, never a silent
/// fallback.
#[test]
fn cap_breach_is_an_error_with_the_numbers() {
    // 8 × 8 = 64 > 32.
    let eight: Vec<&str> = vec!["a", "b", "c", "d", "e", "f", "g", "h"];
    let content = line(vec![
        seq(SequenceType::CYCLE, &eight),
        ContentPart::Text(" ".into()),
        seq(SequenceType::CYCLE, &eight),
    ]);
    let err = enumerate_variant_contents(&content).unwrap_err();
    assert_eq!(err.product, 64);
    assert_eq!(err.cap, VARIANT_CAP);

    // 4 × 4 × 2 = 32 fits exactly.
    let four: Vec<&str> = vec!["a", "b", "c", "d"];
    let content = line(vec![
        seq(SequenceType::CYCLE, &four),
        seq(SequenceType::CYCLE, &four),
        seq(SequenceType::CYCLE, &["x", "y"]),
    ]);
    let e = enumerate_variant_contents(&content).unwrap().unwrap();
    assert_eq!(e.variants.len(), 32);
}

/// An interpolation rides through substitution untouched — variants can
/// be Template lines, not just Plain ones.
#[test]
fn interpolations_survive_substitution() {
    let content = line(vec![
        ContentPart::Text("Hello ".into()),
        seq(SequenceType::STOPPING, &["friend", "stranger"]),
        ContentPart::Text(", ".into()),
        ContentPart::Interpolation(brink_ir::hir::Expr::Int(42)),
        ContentPart::Text(".".into()),
    ]);
    let e = enumerate_variant_contents(&content).unwrap().unwrap();
    assert_eq!(e.variants.len(), 2);
    for v in &e.variants {
        assert!(
            v.parts
                .iter()
                .any(|p| matches!(p, ContentPart::Interpolation(_))),
            "interpolation preserved in every variant"
        );
    }
}
