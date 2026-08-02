//! NS-A6 `std::rand` draw-verb opcode implementations (`docs/stdlib-spec.md`
//! §7, ruled 2026-07-18; `docs/stdlib-sequencing.md` §2 Wave A6, issue
//! #1112).
//!
//! # The one RNG cell
//!
//! There is exactly one RNG state cell per story context: the
//! `(rng_seed, previous_random)` pair `ContextAccess` has always carried —
//! the same cell ink's frozen `RANDOM`/`SEED_RANDOM`/`LIST_RANDOM` ops use,
//! and the same pair `save.rs` has always round-tripped. The brink draw
//! verbs are a second *surface* over that cell, never a second cell: one
//! RNG, two surfaces, no drift. Its name in the effect-row `DefinitionId`
//! space is [`brink_format::DefinitionId::RNG_CELL`]; every op in this
//! module is an ordinary **write** to it (recorded by `vm.rs`'s
//! `note_effect_write` instrumentation under the `effect-trace` feature).
//!
//! # The pinned draw algorithm (stability contract)
//!
//! Draws are a pure function of RNG-state → (value, state′), and the exact
//! chain below is **pinned** — it is the ink-heritage discipline the oracle
//! already anchors, so changing any step is a story-visible break:
//!
//! 1. `seed = rng_seed wrapping_add previous_random`
//! 2. `draw = R::from_seed(seed).next_int()` — a fresh generator per draw,
//!    one `next_int` (non-negative `i32`), exactly like `Opcode::Random`
//! 3. `previous_random = draw` (the state′ write)
//!
//! On top of that per-draw primitive:
//!
//! - **`float()`** builds its `[0,1)` value from the draw's top 24 bits:
//!   `(draw >> 7) / 2²⁴`. Every such value is exactly representable in the
//!   f32 payload (24-bit significand), and the maximum is `(2²⁴−1)/2²⁴ <
//!   1`, so `1.0` is unreachable *by construction* — no rounding cliff at
//!   the top of the interval (a naive `draw / 2³¹` rounds to exactly `1.0`
//!   in f32 for large draws).
//! - **`chance(p)`** draws one `[0,1)` float `u` and returns `u < p`, with
//!   `p` clamped to `[0,1]` and NaN → `false` (F3, ruled 2026-07-19).
//!   `chance` always consumes exactly one draw — NaN and out-of-range `p`
//!   included — so its transcript-position effect on later draws never
//!   depends on the argument's value.
//! - **`pick(coll)`** draws one index `draw % len` (arrays and flags
//!   subsets); an **empty** collection returns `none` without consuming a
//!   draw (there is no index to draw — mirrors the frozen `ListRandom`,
//!   which also skips the draw on empty).
//! - **`shuffle`/`shuffled`** run a Fisher–Yates walk from the top:
//!   for `i` from `len−1` down to `1`, draw once, `j = draw % (i+1)`,
//!   swap `a[i]` and `a[j]` — `len−1` chained draws (zero for `len < 2`),
//!   each advancing the cell through the same primitive.
//! - **`seed(n)`** has no op here — it lowers to the frozen
//!   [`Opcode::SeedRandom`](brink_format::Opcode::SeedRandom) (writes
//!   `rng_seed = n`, `previous_random = 0`).
//!
//! Cross-platform determinism follows from the chain being integer-exact
//! until the final f32 division (itself exact — see above): same seed +
//! same `R` ⇒ identical transcript on every platform.

use alloc::sync::Arc;
use alloc::vec;

use brink_format::Value;

use crate::error::RuntimeError;
use crate::rng::StoryRng;
use crate::state::ContextAccess;
use crate::story::Flow;

/// One draw through the pinned chain (module docs): reads the cell, derives
/// the per-draw seed, advances `previous_random`, returns the raw
/// non-negative `i32` draw.
fn draw<R: StoryRng>(context: &mut (impl ContextAccess + ?Sized)) -> i32 {
    let seed = context.rng_seed().wrapping_add(context.previous_random());
    let next = context.next_random::<R>(seed);
    context.set_previous_random(next);
    next
}

/// One draw shaped into a uniform `[0,1)` f32: top 24 bits over 2²⁴ (module
/// docs — exact in f32, `1.0` unreachable).
fn draw_unit_float<R: StoryRng>(context: &mut (impl ContextAccess + ?Sized)) -> f32 {
    let d = draw::<R>(context);
    // `d` is non-negative (31 significant bits); keep the top 24.
    #[expect(clippy::cast_sign_loss)]
    let top = (d as u32) >> 7;
    #[expect(clippy::cast_precision_loss)]
    {
        top as f32 / 16_777_216.0
    }
}

/// `RandFloat`: `[]` → `Float` in `[0,1)`. One draw.
pub(crate) fn rand_float<R: StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
) {
    let f = draw_unit_float::<R>(context);
    flow.value_stack.push(Value::Float(f));
}

/// `RandChance`: `[p]` → `Bool`. `p` clamped to `[0,1]`, NaN → `false`
/// (F3); exactly one draw, unconditionally. Fault on a non-numeric `p`
/// (malformed question — same doctrine as the NS-A1 verbs).
pub(crate) fn rand_chance<R: StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
) -> Result<(), RuntimeError> {
    let p_val = flow.pop_value()?;
    #[expect(clippy::cast_precision_loss)]
    let p = match &p_val {
        Value::Float(f) => *f,
        Value::Int(n) => *n as f32,
        Value::Bool(b) => {
            // bool → int → float numeric promotion, consistent with the
            // arithmetic ops' ink-heritage coercion.
            if *b { 1.0 } else { 0.0 }
        }
        other => {
            return Err(RuntimeError::StdlibWrongType {
                verb: "chance",
                expected: "a number",
                found: super::collection_ops::type_name(other),
            });
        }
    };
    // The draw happens before interpreting `p` so `chance` always advances
    // the cell exactly once (module docs).
    let u = draw_unit_float::<R>(context);
    // NaN → false falls out of the comparison (`u < NaN` is false); the
    // clamp is only observable through `u < p`, and `u` is already in
    // `[0,1)`, so no explicit clamp is needed: p <= 0 → false, p >= 1 →
    // true, NaN → false. Written as the direct comparison to keep the
    // pinned semantics obvious.
    flow.value_stack.push(Value::Bool(u < p));
    Ok(())
}

/// `rand::int` over a range (NS-A5, `docs/stdlib-spec.md` §7): `[r]` →
/// `Int`. One uniform draw from the range's element sequence — inclusive/
/// exclusive per the range's own written form. Dispatched from `vm.rs`'s
/// `ConvertInt` arm when the operand is a `Range` (one value-directed
/// `int(x)` verb, two legs).
///
/// An **empty** range faults ([`RuntimeError::EmptyRangeDraw`]) without
/// consuming a draw — the F8 gradual-mode residual ("refinements are inert
/// in gradual, the fault is what remains"); under `types = strict` this
/// state is unrepresentable (the checker demanded `NonEmptyRange`
/// evidence, E117). Contrast `pick(range)` below: pick's empty answer is
/// absence (`none`), int's is a malformed question (fault) — the
/// refinement is exactly what separates the two.
///
/// Draw shaping: `value = start + (draw % len)` with `i64` arithmetic —
/// `len` can be up to 2³² (`i32::MIN..=i32::MAX`) while a draw carries 31
/// bits, so ranges longer than 2³¹ cannot reach every element (documented
/// pinned-algorithm trade, matching the frozen ink `RANDOM`'s modulo
/// shaping; game-scale dice never notice).
pub(crate) fn rand_int<R: StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
) -> Result<(), RuntimeError> {
    let v = flow.pop_value()?;
    let (Some((start, end, inclusive)), Some(len)) = (v.as_range(), v.range_len()) else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "int",
            expected: "a range",
            found: super::collection_ops::type_name(&v),
        });
    };
    if len == 0 {
        let range = if inclusive {
            alloc::format!("{start}..={end}")
        } else {
            alloc::format!("{start}..{end}")
        };
        return Err(RuntimeError::EmptyRangeDraw { range });
    }
    let d = draw::<R>(context);
    let offset = i64::from(d) % len;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "start + offset is an element of the range by construction, so it fits i32"
    )]
    let value = (i64::from(start) + offset) as i32;
    flow.value_stack.push(Value::Int(value));
    Ok(())
}

/// `RandPick`: `[coll]` → `Option[T]`. Uniform draw from an array, a
/// flags subset, or a range; empty → `none` (no draw). Fault on anything
/// else.
pub(crate) fn rand_pick<R: StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
) -> Result<(), RuntimeError> {
    let coll = flow.pop_value()?;
    let picked = match &coll {
        Value::Array(items) => {
            if items.is_empty() {
                Value::none()
            } else {
                let d = draw::<R>(context);
                #[expect(clippy::cast_sign_loss)]
                let idx = (d as usize) % items.len();
                Value::some(items[idx].clone())
            }
        }
        // Flags subsets (list values): mirror the frozen `ListRandom`
        // selection (draw over `items` in stored order, keep origins), but
        // absence is `none`, not an empty list.
        Value::List(lv) => {
            if lv.items.is_empty() {
                Value::none()
            } else {
                let d = draw::<R>(context);
                #[expect(clippy::cast_sign_loss)]
                let idx = (d as usize) % lv.items.len();
                Value::some(Value::List(Arc::new(brink_format::ListValue {
                    items: vec![lv.items[idx]],
                    origins: lv.origins.clone(),
                })))
            }
        }
        // Ranges (NS-A5, `docs/stdlib-spec.md` §7): a range is a closed-set
        // iterable, so pick draws one of its elements uniformly. Empty →
        // `none` without a draw — pick's emptiness is dynamic-content
        // *absence* (contrast `int(range)`, whose emptiness is a fault:
        // the refinement is exactly what separates the two verbs).
        Value::Range { start, .. } => {
            let len = coll.range_len().unwrap_or(0);
            if len == 0 {
                Value::none()
            } else {
                let d = draw::<R>(context);
                let offset = i64::from(d) % len;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "start + offset is an element of the range, so it fits i32"
                )]
                Value::some(Value::Int((i64::from(*start) + offset) as i32))
            }
        }
        other => {
            return Err(RuntimeError::StdlibWrongType {
                verb: "pick",
                expected: "an array, flags subset, or range",
                found: super::collection_ops::type_name(other),
            });
        }
    };
    flow.value_stack.push(picked);
    Ok(())
}

/// `RandShuffle`: `[a]` → `[a']`. Fisher–Yates from the top, `len−1` draws
/// (module docs). One op serves both `shuffle(a)` (RMW write-back at the
/// lowering) and `shuffled(a)` (functional). Fault on a non-array.
pub(crate) fn rand_shuffle<R: StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
) -> Result<(), RuntimeError> {
    let mut coll = flow.pop_value()?;
    if !matches!(coll, Value::Array(_)) {
        return Err(RuntimeError::StdlibWrongType {
            verb: "shuffle",
            expected: "an array",
            found: super::collection_ops::type_name(&coll),
        });
    }
    if let Some(items) = coll.array_make_mut() {
        for i in (1..items.len()).rev() {
            let d = draw::<R>(context);
            #[expect(clippy::cast_sign_loss)]
            let j = (d as usize) % (i + 1);
            items.swap(i, j);
        }
    }
    flow.value_stack.push(coll);
    Ok(())
}

/// `Collect(RandRoll)` (NS-A7, `docs/stdlib-spec.md` §8): `[w]` → `T` —
/// one weighted draw from a `Weighted[T]` table. **Total over any table
/// that exists**: construction is the validator (evidence-by-construction
/// — non-empty, positive int weights), so the walk below always lands on
/// an entry. Lives in `rand`'s namespace because its row writes the RNG
/// cell: one draw, offset into the total weight by the same modulo shaping
/// as `rand_int`, then an accumulating walk over the entries in
/// construction order (deterministic offset → entry mapping).
pub(crate) fn rand_roll<R: StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
) -> Result<(), RuntimeError> {
    let table = flow.pop_value()?;
    let Value::Weighted(w) = &table else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "roll",
            expected: "a weighted table",
            found: super::collection_ops::type_name(&table),
        });
    };
    // Total weight is ≥ 1 by construction; sum in i64 (a sum of i32
    // weights can exceed i32::MAX).
    let total = w.total_weight();
    let d = draw::<R>(context);
    let mut offset = i64::from(d) % total;
    for (weight, value) in &w.entries {
        offset -= i64::from(*weight);
        if offset < 0 {
            flow.value_stack.push(value.clone());
            return Ok(());
        }
    }
    // Unreachable: `offset < total` and the weights sum to `total`.
    // Guarded rather than asserted (panics are denied; malformed-bytecode
    // robustness) — fault loudly instead of returning garbage.
    Err(RuntimeError::WeightedMalformedTable {
        detail: "a draw walk that exhausted the table (corrupt total weight)",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;
    use crate::rng::{DotNetRng, FastRng};
    use crate::world::World;
    use alloc::vec::Vec;

    /// A `Flow` with nothing but an empty value stack — every op in this
    /// module reads/writes only `flow.value_stack` (same fixture shape as
    /// `collection_ops::tests`).
    fn test_flow() -> Flow {
        Flow {
            threads: Vec::new(),
            value_stack: Vec::new(),
            output: OutputBuffer::new(),
            pending_choices: Vec::new(),
            current_tags: Vec::new(),
            in_tag: false,
            skipping_choice: false,
            did_safe_exit: false,
            did_unsafe_yield: false,
            ran_out_of_content_cause: crate::RanOutOfContentCause::default(),
            exec_mode: crate::story::ExecMode::default(),
            pure_callback: crate::story::PureCallbackState::default(),
        }
    }

    /// A bare `World` — the RNG cell (`rng_seed` + `previous_random`) is
    /// all these tests touch.
    fn test_context() -> World {
        World::from_globals(Vec::new(), crate::world::ResolvedPolicy::all_world())
    }

    #[test]
    fn unit_float_is_in_half_open_interval_and_advances_the_cell() {
        let mut ctx = test_context();
        ctx.set_rng_seed(42);
        let mut prev_state = ctx.previous_random();
        for _ in 0..1000 {
            let f = draw_unit_float::<DotNetRng>(&mut ctx);
            assert!((0.0..1.0).contains(&f), "draw out of [0,1): {f}");
            assert_ne!(
                ctx.previous_random(),
                prev_state,
                "a draw must advance the cell"
            );
            prev_state = ctx.previous_random();
        }
    }

    #[test]
    fn draws_are_a_pure_function_of_state() {
        // Same cell state ⇒ same draw, on both built-in generators.
        let mut a = test_context();
        let mut b = test_context();
        a.set_rng_seed(7);
        b.set_rng_seed(7);
        for _ in 0..100 {
            assert_eq!(draw::<DotNetRng>(&mut a), draw::<DotNetRng>(&mut b));
            assert_eq!(a.previous_random(), b.previous_random());
        }
        a.set_rng_seed(7);
        a.set_previous_random(0);
        b.set_rng_seed(7);
        b.set_previous_random(0);
        for _ in 0..100 {
            assert_eq!(draw::<FastRng>(&mut a), draw::<FastRng>(&mut b));
        }
    }

    #[test]
    fn chance_extremes_and_nan() {
        let mut ctx = test_context();
        ctx.set_rng_seed(1);
        // p = 1 → always true (u ∈ [0,1) < 1).
        for _ in 0..50 {
            let mut flow = test_flow();
            flow.value_stack.push(Value::Float(1.0));
            rand_chance::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            assert_eq!(flow.pop_value().unwrap(), Value::Bool(true));
        }
        // p = 0 → always false.
        for _ in 0..50 {
            let mut flow = test_flow();
            flow.value_stack.push(Value::Float(0.0));
            rand_chance::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            assert_eq!(flow.pop_value().unwrap(), Value::Bool(false));
        }
        // NaN → false (F3), and the draw is still consumed.
        let before = ctx.previous_random();
        let mut flow = test_flow();
        flow.value_stack.push(Value::Float(f32::NAN));
        rand_chance::<DotNetRng>(&mut flow, &mut ctx).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(false));
        assert_ne!(ctx.previous_random(), before, "NaN chance must still draw");
        // Out-of-range p clamps by interpretation: p > 1 behaves as 1.
        let mut flow = test_flow();
        flow.value_stack.push(Value::Float(2.5));
        rand_chance::<DotNetRng>(&mut flow, &mut ctx).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(true));
        // p < 0 behaves as 0.
        let mut flow = test_flow();
        flow.value_stack.push(Value::Float(-3.0));
        rand_chance::<DotNetRng>(&mut flow, &mut ctx).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(false));
    }

    #[test]
    fn chance_wrong_type_faults() {
        let mut ctx = test_context();
        let mut flow = test_flow();
        flow.value_stack.push(Value::from("nope"));
        let err = rand_chance::<DotNetRng>(&mut flow, &mut ctx).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StdlibWrongType { verb: "chance", .. }
        ));
    }

    #[test]
    fn pick_empty_is_none_and_consumes_no_draw() {
        let mut ctx = test_context();
        ctx.set_rng_seed(9);
        let before = ctx.previous_random();
        let mut flow = test_flow();
        flow.value_stack.push(Value::array(vec![]));
        rand_pick::<DotNetRng>(&mut flow, &mut ctx).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::none());
        assert_eq!(
            ctx.previous_random(),
            before,
            "empty pick must not consume a draw"
        );
    }

    #[test]
    fn pick_draws_a_member() {
        let mut ctx = test_context();
        ctx.set_rng_seed(3);
        let items = vec![Value::Int(10), Value::Int(20), Value::Int(30)];
        for _ in 0..100 {
            let mut flow = test_flow();
            flow.value_stack.push(Value::array(items.clone()));
            rand_pick::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            let picked = flow.pop_value().unwrap();
            let Value::OptionVal(Some(inner)) = picked else {
                unreachable!("pick over non-empty must be some(_)");
            };
            assert!(items.contains(&inner), "picked a non-member: {inner:?}");
        }
    }

    #[test]
    fn pick_wrong_type_faults() {
        let mut ctx = test_context();
        let mut flow = test_flow();
        flow.value_stack.push(Value::Int(5));
        let err = rand_pick::<DotNetRng>(&mut flow, &mut ctx).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StdlibWrongType { verb: "pick", .. }
        ));
    }

    #[test]
    fn shuffle_is_a_permutation_and_seed_deterministic() {
        let original: Vec<Value> = (0..10).map(Value::Int).collect();

        let run = |seed: i32| -> Vec<Value> {
            let mut ctx = test_context();
            ctx.set_rng_seed(seed);
            let mut flow = test_flow();
            flow.value_stack.push(Value::array(original.clone()));
            rand_shuffle::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            let Value::Array(items) = flow.pop_value().unwrap() else {
                unreachable!("shuffle must return an array");
            };
            items.as_ref().clone()
        };

        let a = run(11);
        let b = run(11);
        let c = run(12);
        assert_eq!(a, b, "same seed must shuffle identically");
        // Permutation check: same multiset.
        let mut sorted = a.clone();
        sorted.sort_by_key(|v| if let Value::Int(n) = v { *n } else { -1 });
        assert_eq!(sorted, original);
        // Different seed: overwhelmingly a different order for 10 elements
        // (10! orders); equality would indicate the seed isn't reaching the
        // draw chain at all.
        assert_ne!(a, c, "distinct seeds produced identical shuffles");
    }

    #[test]
    fn shuffle_len_below_two_consumes_no_draw() {
        for arr in [vec![], vec![Value::Int(1)]] {
            let mut ctx = test_context();
            ctx.set_rng_seed(5);
            let before = ctx.previous_random();
            let mut flow = test_flow();
            flow.value_stack.push(Value::array(arr));
            rand_shuffle::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            assert_eq!(ctx.previous_random(), before);
        }
    }

    // ── NS-A5: `rand::int` over the inhabited range + pick's range leg ──

    #[test]
    fn rand_int_draws_within_bounds_both_forms() {
        let mut ctx = test_context();
        ctx.set_rng_seed(21);
        // Inclusive: every draw lands in [1, 6].
        for _ in 0..200 {
            let mut flow = test_flow();
            flow.value_stack.push(Value::range(1, 6, true));
            rand_int::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            let Value::Int(n) = flow.pop_value().unwrap() else {
                unreachable!("rand_int returns Int");
            };
            assert!((1..=6).contains(&n), "inclusive draw out of range: {n}");
        }
        // Exclusive: every draw lands in [0, 3).
        for _ in 0..200 {
            let mut flow = test_flow();
            flow.value_stack.push(Value::range(0, 3, false));
            rand_int::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            let Value::Int(n) = flow.pop_value().unwrap() else {
                unreachable!("rand_int returns Int");
            };
            assert!((0..3).contains(&n), "exclusive draw out of range: {n}");
        }
        // Single-element range: total, always that element, still draws.
        let before = ctx.previous_random();
        let mut flow = test_flow();
        flow.value_stack.push(Value::range(5, 5, true));
        rand_int::<DotNetRng>(&mut flow, &mut ctx).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(5));
        assert_ne!(ctx.previous_random(), before, "5..=5 still consumes a draw");
    }

    #[test]
    fn rand_int_consumes_exactly_one_draw() {
        let mut a = test_context();
        let mut b = test_context();
        a.set_rng_seed(9);
        b.set_rng_seed(9);
        let mut flow = test_flow();
        flow.value_stack.push(Value::range(1, 100, true));
        rand_int::<DotNetRng>(&mut flow, &mut a).unwrap();
        // One raw draw on the twin cell reaches the same state.
        draw::<DotNetRng>(&mut b);
        assert_eq!(a.previous_random(), b.previous_random());
    }

    #[test]
    fn rand_int_empty_range_faults_without_drawing() {
        // The F8 gradual-mode residual: `int(0..0)` is a turn-terminating
        // fault (a draw from nothing is a malformed question), and the
        // cell is untouched — a faulted draw must not advance the RNG.
        for r in [
            Value::range(0, 0, false),
            Value::range(5, 5, false),
            Value::range(7, 2, true),
        ] {
            let mut ctx = test_context();
            ctx.set_rng_seed(3);
            let before = ctx.previous_random();
            let mut flow = test_flow();
            flow.value_stack.push(r);
            let err = rand_int::<DotNetRng>(&mut flow, &mut ctx).unwrap_err();
            assert!(matches!(err, RuntimeError::EmptyRangeDraw { .. }));
            assert_eq!(ctx.previous_random(), before);
        }
        // The fault message carries the written form.
        let mut ctx = test_context();
        let mut flow = test_flow();
        flow.value_stack.push(Value::range(5, 2, true));
        let RuntimeError::EmptyRangeDraw { range } =
            rand_int::<DotNetRng>(&mut flow, &mut ctx).unwrap_err()
        else {
            unreachable!("empty range must fault EmptyRangeDraw");
        };
        assert_eq!(range, "5..=2");
    }

    #[test]
    fn rand_int_wrong_type_faults() {
        let mut ctx = test_context();
        let mut flow = test_flow();
        flow.value_stack.push(Value::Int(6));
        let err = rand_int::<DotNetRng>(&mut flow, &mut ctx).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StdlibWrongType { verb: "int", .. }
        ));
    }

    #[test]
    fn pick_range_draws_an_element_and_empty_is_none() {
        let mut ctx = test_context();
        ctx.set_rng_seed(13);
        for _ in 0..100 {
            let mut flow = test_flow();
            flow.value_stack.push(Value::range(10, 13, false));
            rand_pick::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            let Value::OptionVal(Some(inner)) = flow.pop_value().unwrap() else {
                unreachable!("pick over non-empty range must be some(_)");
            };
            let Value::Int(n) = *inner else {
                unreachable!("pick over a range yields ints");
            };
            assert!((10..13).contains(&n), "picked non-member: {n}");
        }
        // Empty range: absence (`none`), no draw — pick's emptiness is
        // dynamic-content absence, NOT the int() fault.
        let before = ctx.previous_random();
        let mut flow = test_flow();
        flow.value_stack.push(Value::range(4, 4, false));
        rand_pick::<DotNetRng>(&mut flow, &mut ctx).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::none());
        assert_eq!(ctx.previous_random(), before, "empty pick must not draw");
    }

    #[test]
    fn shuffle_wrong_type_faults() {
        let mut ctx = test_context();
        let mut flow = test_flow();
        flow.value_stack.push(Value::from("abc"));
        let err = rand_shuffle::<DotNetRng>(&mut flow, &mut ctx).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "shuffle",
                ..
            }
        ));
    }
    // ── NS-A7: rand::roll over Weighted[T] (docs/stdlib-spec.md §8,
    // #1113) ─────────────────────────────────────────────────────────────

    fn roll_table() -> Value {
        Value::weighted(vec![
            (3, Value::String("sword".into())),
            (1, Value::String("shield".into())),
        ])
    }

    #[test]
    fn roll_is_total_deterministic_and_advances_the_cell() {
        // Seeded replay: the same cell state draws the same entry, and
        // every roll advances the cell (a roll IS a draw).
        let mut a = test_context();
        let mut b = test_context();
        a.set_rng_seed(7);
        b.set_rng_seed(7);
        for _ in 0..200 {
            let before = a.previous_random();
            let mut fa = test_flow();
            let mut fb = test_flow();
            fa.value_stack.push(roll_table());
            fb.value_stack.push(roll_table());
            rand_roll::<DotNetRng>(&mut fa, &mut a).unwrap();
            rand_roll::<DotNetRng>(&mut fb, &mut b).unwrap();
            let va = fa.pop_value().unwrap();
            assert_eq!(va, fb.pop_value().unwrap(), "seeded replay identical");
            assert!(
                matches!(&va, Value::String(s) if s.as_ref() == "sword" || s.as_ref() == "shield"),
                "roll lands on an entry: {va:?}"
            );
            assert_ne!(a.previous_random(), before, "a roll must draw");
        }
    }

    #[test]
    fn roll_respects_weights_over_many_draws() {
        // Statistical sanity, not a distribution proof: over 4000 draws
        // from `{3: sword, 1: shield}` the 3-weight entry must clearly
        // dominate (the modulo walk gives it exactly 3/4 of the residue
        // classes). Deterministic under the pinned chain + fixed seed.
        let mut ctx = test_context();
        ctx.set_rng_seed(11);
        let mut swords = 0u32;
        for _ in 0..4000 {
            let mut flow = test_flow();
            flow.value_stack.push(roll_table());
            rand_roll::<DotNetRng>(&mut flow, &mut ctx).unwrap();
            if flow.pop_value().unwrap() == Value::String("sword".into()) {
                swords += 1;
            }
        }
        assert!(
            (2700..=3300).contains(&swords),
            "expected ~3000/4000 swords, got {swords}"
        );
    }

    #[test]
    fn roll_single_entry_table_is_the_identity_draw() {
        // A one-entry table always lands on it — totality by construction.
        let mut ctx = test_context();
        ctx.set_rng_seed(3);
        let mut flow = test_flow();
        flow.value_stack
            .push(Value::weighted(vec![(i32::MAX, Value::Int(9))]));
        rand_roll::<DotNetRng>(&mut flow, &mut ctx).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(9));
    }

    #[test]
    fn roll_wrong_type_faults() {
        let mut ctx = test_context();
        let mut flow = test_flow();
        flow.value_stack.push(Value::Int(3));
        let err = rand_roll::<DotNetRng>(&mut flow, &mut ctx).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "roll",
                expected: "a weighted table",
                found: "int",
            }
        );
    }
}
