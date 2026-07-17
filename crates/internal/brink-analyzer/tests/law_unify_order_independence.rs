//! Law: **`unify`/`unify_all` order-independence, including `Conflicted`
//! absorption** (issue #627's design note, `infer/ty.rs`'s module doc) —
//! issue #746 item 2's first half, the residue #738 left of #672 workstream
//! B's "inference laws".
//!
//! `infer/ty.rs`'s own `#[cfg(test)]` module already hand-picks a handful of
//! orderings to prove the #627 bug (a genuine `int`/`string` conflict
//! "self-healing" back to a concrete type depending on source order) stays
//! fixed. This suite generalizes that to arbitrary generated [`Ty`] values
//! and arbitrary permutations, proving the algebraic properties that make
//! order-independence *inevitable* rather than checking it case by case:
//!
//! - `unify` is **commutative**: `unify(a, b) == unify(b, a)`;
//! - `unify` is **associative**: `unify(unify(a, b), c) == unify(a, unify(b, c))`;
//! - `unify` is **idempotent**: `unify(a, a) == a`;
//! - `Unknown` is the **join identity**: `unify(Unknown, a) == a` in either
//!   position;
//! - `Conflicted` is **absorbing**: `unify(Conflicted, a) == Conflicted` in
//!   either position (the #627 lattice point itself);
//! - `unify_all` (the fold used to accumulate every observed use of a
//!   slot) is **invariant under reordering** — proven directly by swapping
//!   two arbitrary positions in a generated sequence and asserting the
//!   fold result is unchanged, which is what commutativity + associativity
//!   guarantee for any fold over any permutation, not just a single swap.
//!
//! Reproducibility (house determinism rule, `CLAUDE.md`): proptest's default
//! RNG is entropy-seeded per run, not fixed — generated cases differ run to
//! run. Reproducibility instead comes from `ProptestConfig::with_cases`
//! (a fixed, deterministic *count* of cases every run) and from proptest's
//! own failure-persistence file (`.proptest-regressions`), which pins the
//! exact seed of any failing case for replay. Set `PROPTEST_RNG_SEED` if
//! bit-for-bit seed reproducibility across every run — not just failures —
//! is ever required.

#![allow(clippy::unwrap_used)]

use brink_analyzer::{Ty, unify, unify_all};
use proptest::prelude::*;

/// A [`Ty`] value, bounded recursion (typed-mode-spec §2/§4): scalars plus
/// nominal `list`/`struct`/`handle` (string-keyed) leaves, `Unknown` and
/// `Conflicted` (the #627 lattice point this suite is about), extended with
/// `Array`/`Map`/`Fn` at bounded depth so the recursive `unify` arms
/// (`Ty::Array`, `Ty::Map`, `Ty::Fn`) are exercised, not just the flat
/// scalar lattice.
fn arb_ty() -> impl Strategy<Value = Ty> {
    let leaf = prop_oneof![
        Just(Ty::Int),
        Just(Ty::Float),
        Just(Ty::Bool),
        Just(Ty::String),
        Just(Ty::Divert),
        "[a-z]{1,4}".prop_map(Ty::List),
        "[a-z]{1,4}".prop_map(Ty::Struct),
        "[a-z]{1,4}".prop_map(Ty::Handle),
        Just(Ty::Unknown),
        Just(Ty::Conflicted),
    ];
    leaf.prop_recursive(3, 16, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|t| Ty::Array(Box::new(t))),
            (inner.clone(), inner.clone()).prop_map(|(k, v)| Ty::Map(Box::new(k), Box::new(v))),
            (prop::collection::vec(inner.clone(), 0..3), inner)
                .prop_map(|(params, ret)| Ty::Fn(params, Box::new(ret))),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn unify_is_commutative(a in arb_ty(), b in arb_ty()) {
        prop_assert_eq!(unify(&a, &b), unify(&b, &a));
    }

    #[test]
    fn unify_is_associative(a in arb_ty(), b in arb_ty(), c in arb_ty()) {
        prop_assert_eq!(unify(&unify(&a, &b), &c), unify(&a, &unify(&b, &c)));
    }

    #[test]
    fn unify_is_idempotent(a in arb_ty()) {
        prop_assert_eq!(unify(&a, &a), a);
    }

    #[test]
    fn unknown_is_join_identity(a in arb_ty()) {
        prop_assert_eq!(unify(&Ty::Unknown, &a), a.clone());
        prop_assert_eq!(unify(&a, &Ty::Unknown), a);
    }

    /// The #627 lattice point: `Conflicted` absorbs everything, including
    /// `Unknown` — the whole point being that a genuine conflict can never
    /// be masked by a later unconstrained observation, regardless of order.
    #[test]
    fn conflicted_is_absorbing(a in arb_ty()) {
        prop_assert_eq!(unify(&Ty::Conflicted, &a), Ty::Conflicted);
        prop_assert_eq!(unify(&a, &Ty::Conflicted), Ty::Conflicted);
    }

    /// `unify_all` (the accumulator every inferred slot folds its observed
    /// uses through) is invariant under swapping any two positions in the
    /// input sequence — the direct "order-independence" property #627 asks
    /// for, generalized past the hand-picked orderings already unit-tested
    /// in `infer/ty.rs`.
    #[test]
    fn unify_all_is_invariant_under_swap(
        tys in prop::collection::vec(arb_ty(), 1..8),
        i in 0usize..8,
        j in 0usize..8,
    ) {
        let i = i % tys.len();
        let j = j % tys.len();

        let baseline = unify_all(tys.iter().cloned());

        let mut swapped = tys.clone();
        swapped.swap(i, j);
        let after_swap = unify_all(swapped);

        prop_assert_eq!(baseline, after_swap);
    }
}
