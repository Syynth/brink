//! `Rebase` must shift EVERY source range it carries — the property the
//! segment road's `assemble_lowered_file` depends on when it lifts a
//! fragment parsed at offset 0 into its place in the file.
//!
//! This is a whole-class guard, written after two impls were found that
//! silently skipped a field: `MapLiteral::entries` and
//! `StructLiteral::fields`, both `Vec<(A, B)>`. Every expression inside a
//! map or struct literal kept its segment-relative range forever, so a
//! `#fn(target)` inside a map resolved at a range that pointed into
//! unrelated text — off by exactly the segment's offset.
//!
//! Nothing caught it because the defect was self-consistent: the HIR and
//! the resolution map were built from the same un-rebased node, so they
//! agreed with each other at the wrong coordinate. It surfaced only when
//! something computed a range independently.
//!
//! Checking the Debug rendering rather than a hand-written visitor is
//! deliberate: a visitor would need the same per-field enumeration that
//! was wrong in the first place, so it would inherit the bug. `Debug` is
//! derived, so it sees fields the hand-written impl forgot.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::FileId;
use brink_ir::hir::rebase::Rebase as _;

/// A definition's Debug rendering before rebasing, and a thunk that
/// rebases its own copy and renders it again.
type Subject = (String, String, Box<dyn FnOnce() -> String>);

/// Exercises every container that holds expressions, especially the
/// tuple-carrying ones, nested so a skipped field cannot hide behind a
/// sibling that is handled.
const SRC: &str = "\
=== function double(x) ===
~ return x + x

VAR stored = 0

=== setup ===
~ stored = #{\"double\": #fn(double), \"nested\": #{\"inner\": #fn(double)}}
~ temp arr = #[1, 2, double(3)]
~ temp m = #{\"k\": arr[0]}
Set up {m}.
-> DONE

=== invoke ===
~ temp f = stored[\"double\"]
~ temp r = f(21)
Result {r}.
-> END
";

/// Every `range: A..B` in the Debug rendering, in order.
fn ranges(debug: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for part in debug.split("range: ").skip(1) {
        let head: String = part
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Some((a, b)) = head.split_once("..")
            && let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>())
        {
            out.push((a, b));
        }
    }
    out
}

#[test]
fn rebasing_shifts_every_range_the_hir_carries() {
    let file = FileId(0);
    let delta = rowan::TextSize::from(1000);

    let parse = brink_syntax::parse(SRC);
    let tree = parse.tree();
    let (root, top_level, _diags) = brink_ir::lower_top_level(file, &tree);
    let knots: Vec<_> = tree
        .knots()
        .map(|k| brink_ir::lower_single_knot(file, &k))
        .collect();

    // One tree per definition, so a failure names the definition it is in.
    let mut subjects: Vec<Subject> = Vec::new();
    for (i, (knot, _)) in knots.iter().enumerate() {
        let Some(knot) = knot.clone() else { continue };
        let before = format!("{knot:#?}");
        let mut moved = knot;
        subjects.push((
            format!("knot #{i}"),
            before,
            Box::new(move || {
                moved.rebase(delta, file);
                format!("{moved:#?}")
            }),
        ));
    }
    {
        let before = format!("{root:#?}");
        let mut moved = root;
        subjects.push((
            "root content".to_owned(),
            before,
            Box::new(move || {
                moved.rebase(delta, file);
                format!("{moved:#?}")
            }),
        ));
    }
    for (i, knot) in top_level.iter().enumerate() {
        let before = format!("{knot:#?}");
        let mut moved = knot.clone();
        subjects.push((
            format!("top-level knot #{i}"),
            before,
            Box::new(move || {
                moved.rebase(delta, file);
                format!("{moved:#?}")
            }),
        ));
    }

    let mut checked = 0usize;
    for (label, before, rebase) in subjects {
        let after = rebase();
        let (a, b) = (ranges(&before), ranges(&after));
        assert_eq!(
            a.len(),
            b.len(),
            "{label}: rebasing changed the tree's shape, not just its positions"
        );
        for (i, (o, n)) in a.iter().zip(b.iter()).enumerate() {
            // A node the passes SYNTHESIZE carries the provenance-free
            // `0..0`; it is not a position, so it does not shift.
            if *o == (0, 0) {
                assert_eq!(
                    *n,
                    (0, 0),
                    "{label}: range #{i} was provenance-free `0..0` and must stay `0..0`"
                );
                continue;
            }
            assert_eq!(
                (n.0, n.1),
                (o.0 + u32::from(delta), o.1 + u32::from(delta)),
                "{label}: range #{i} {o:?} was NOT shifted by {}, it became {n:?} — some \
                 `Rebase` impl is skipping the field that holds it",
                u32::from(delta)
            );
            checked += 1;
        }
    }
    assert!(
        checked > 50,
        "expected the fixture to carry many ranges, checked {checked}"
    );
}
