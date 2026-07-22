//! The ONE intrinsic effect table (issue #1128).
//!
//! `infer::body::InferPass::infer_intrinsic` harvests each unresolved
//! stdlib-intrinsic call's effect atoms — the faulting set (NS-A2's fault
//! dimension) and the RNG-cell write set (NS-A6's "every draw is an ordinary
//! write") — and `await_purity` consults the same facts for an unresolved
//! call sitting *directly in* an `await` condition (the wake-gate gap
//! NS-A6's build disclosed: `await chance(0.5)` / `await pop(a)` escaped
//! E105 because the purity walk only judged resolved callees' rows). Both
//! consumers read this module so there is exactly one list — no second copy
//! to drift.
//!
//! Membership notes (the audits live on `infer_intrinsic`'s original
//! comments, summarized here):
//!
//! - **Faults**: every intrinsic whose VM op has at least one
//!   turn-terminating fault path — the conversion faults (`int`/`float`,
//!   and the classic uppercase `INT`/`FLOAT` builtins since #955),
//!   `char_at`'s OOB/wrong-type faults, the NS-A1 `StdlibWrongType`/
//!   `NotOrderable` verbs, the collection ops' `NotIndexable`/key-domain
//!   faults, and the NS-A6 rand verbs' wrong-type faults. NOT in the set:
//!   `string`/`some` (total over every value), `seed` (the frozen
//!   `SeedRandom` op coerces, ink-heritage leniency), the nullary `float()`
//!   rand draw (no argument, no fault path), and `call`/`bind` (their
//!   dispatch-fault marking lives at `check_value_call`/`check_bind_value`).
//!   NS-A5 adds `non_empty` (wrong-typed argument faults, the
//!   malformed-question doctrine).
//! - **RNG writes** (draws): the brink draw verbs (`chance`/`pick`/
//!   `shuffle`/`shuffled`, nullary `float()`) plus `seed` and the frozen ink
//!   spellings (`RANDOM`/`SEED_RANDOM`/`LIST_RANDOM`) — one cell
//!   ([`brink_format::DefinitionId::RNG_CELL`]), two surfaces, one entry.
//!   NS-A5's `int(range)` draw leg is deliberately NOT here: it is
//!   type-directed (range argument → draw, else pure conversion), which a
//!   name+arity table cannot express — `infer_intrinsic`'s `"int"` arm
//!   records that write inline. `await_purity` still rejects
//!   `await int(…)` through this table's fault bit (`int` faults in every
//!   shape), so the wake gate has no gap.

/// Effect facts for one intrinsic call shape. `arg_count` participates
/// because `float` is two different call shapes: the nullary rand draw
/// (an RNG write, no fault path) vs the unary conversion (a fault path,
/// no draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntrinsicEffects {
    /// The call can raise a turn-terminating fault (NS-A2 fault dimension).
    pub faults: bool,
    /// The call writes the RNG state cell (NS-A6: draws are writes).
    pub rng_write: bool,
}

/// The effect facts for an unresolved single-segment call named `name` with
/// `arg_count` arguments — the intrinsic-dispatch shadow-fallback shape (a
/// resolved def always wins resolution first, so a caller must check the
/// resolution map before consulting this table). A name outside the table
/// returns all-`false`.
pub(crate) fn intrinsic_effects(name: &str, arg_count: usize) -> IntrinsicEffects {
    let is_rand_float_draw = name == "float" && arg_count == 0;
    let faults = !is_rand_float_draw
        && matches!(
            name,
            "len"
                | "keys"
                | "values"
                | "contains"
                | "push"
                | "insert"
                | "remove"
                | "int"
                | "float"
                | "char_at"
                | "find"
                | "index_of"
                | "min"
                | "max"
                | "first"
                | "last"
                | "pop"
                | "get"
                | "contains_value"
                | "clear"
                | "chance"
                | "pick"
                | "shuffle"
                | "shuffled"
                | "non_empty"
                // NS-A4 ordering verbs (issue #1110, stdlib-spec §4b):
                // `sort`/`sorted` fault on wrong container type,
                // unorderable elements, and (dev mode) NaN comparands —
                // rows are mode-independent, so the bit is the
                // conservative union (prod never fires the NaN leg).
                // `sort_by`/`sorted_by` do NOT inherit the float fault
                // (F14) but keep the bit for their own paths: comparator
                // dispatch faults, non-int returns, `⊕cmp`, detected
                // inconsistency.
                | "sort"
                | "sorted"
                | "sort_by"
                | "sorted_by"
                // NS-A7 collections+ (issue #1113, stdlib-spec §8):
                // `weighted` faults on a computed non-positive/non-int
                // weight (the construction-fault residual of the E120
                // split); `roll` on a non-Weighted operand (total over any
                // table that exists — discharged below when the operand
                // type is provable); the heap verbs on wrong container
                // types, unorderable elements, and (dev, `heap_push`
                // only) the §4b NaN entry check — conservative-union
                // bits, mode-independent rows.
                | "weighted"
                | "roll"
                | "heap_push"
                | "heap_pop"
                | "heap_peek"
                | "INT"
                | "FLOAT"
                // NS-A8 tower (issue #1114): constructors fault on
                // non-numeric lanes / wrong-kind columns, verbs on wrong
                // operand kinds (`StdlibWrongType`) — all otherwise pure
                // (no draws, no writes, no emits).
                | "vec2"
                | "vec3"
                | "vec4"
                | "quat"
                | "mat2"
                | "mat3"
                | "mat4"
                | "dot"
                | "cross"
                | "clamp"
                | "lerp"
        );
    let rng_write = is_rand_float_draw
        || matches!(
            name,
            "chance"
                | "pick"
                | "shuffle"
                | "shuffled"
                | "seed"
                // NS-A7: `roll(w)` is a draw — one RNG-cell write, like
                // every draw (it lives in `std::rand` for exactly this
                // reason).
                | "roll"
                | "RANDOM"
                | "SEED_RANDOM"
                | "LIST_RANDOM"
        );
    IntrinsicEffects { faults, rng_write }
}

/// NS-A4 / **F29(a)** (ruled by delegation 2026-07-19, stdlib-spec §4b):
/// whether an intrinsic call's fault charge is **discharged by local type
/// evidence** — the call is provably total given the arguments' inferred
/// types, so the *refined* faults bit (`EffectRow::faults_refined`) skips
/// it while the conservative bit keeps it.
///
/// The audit, verb by verb (anything not listed keeps its charge):
///
/// - **Wrong-type-only faults** discharge when the container/argument type
///   provably matches: `len` (array/map/string), `keys`/`values`/`clear`/
///   `contains_value` (map), `contains`/`index_of`/`first`/`last`/`pop`/
///   `push` (array — `push` appends at `len`, always in bounds), `find`
///   (two strings), `get` (map with a scalar key type).
/// - **`min`/`max` (one-arg) and `sort`/`sorted`** additionally require a
///   provably NaN-free *element* type — int/string/bool, NOT float: §4b
///   makes float orderings carry `faults` unconditionally
///   (mode-independent rows; the dev-mode NaN fault is real). Nested
///   arrays are conservatively kept (the recursive roster check belongs
///   to a finer rung).
/// - **Never discharged** (value-dependent or parse-domain faults):
///   `insert`/`remove` (OOB), `char_at` (OOB), `int`/`float`/`INT`/
///   `FLOAT` (parse/domain), `chance`/`pick`/`shuffle`/`shuffled`
///   (wrong-type but rand-coupled — kept simple), `non_empty`, the tower
///   family, `sort_by`/`sorted_by` (comparator dispatch + `⊕cmp`).
pub(crate) fn intrinsic_fault_discharged(name: &str, arg_tys: &[super::Ty]) -> bool {
    use super::Ty;
    let scalar_orderable = |t: &Ty| matches!(t, Ty::Int | Ty::String | Ty::Bool);
    match name {
        "len" => matches!(
            arg_tys.first(),
            Some(Ty::Array(_) | Ty::Map(_, _) | Ty::String)
        ),
        "keys" | "values" | "clear" | "contains_value" => {
            matches!(arg_tys.first(), Some(Ty::Map(_, _)))
        }
        "get" => matches!(
            arg_tys.first(),
            Some(Ty::Map(k, _)) if scalar_orderable(k)
        ),
        "contains" | "index_of" | "first" | "last" | "pop" | "push" => {
            matches!(arg_tys.first(), Some(Ty::Array(_)))
        }
        "find" => matches!(
            (arg_tys.first(), arg_tys.get(1)),
            (Some(Ty::String), Some(Ty::String))
        ),
        "min" | "max" if arg_tys.len() == 1 => matches!(
            arg_tys.first(),
            Some(Ty::Array(elem)) if scalar_orderable(elem)
        ),
        // NS-A7 (stdlib-spec §8): `heap_push`/`heap_pop` follow the `sort`
        // rule exactly (their sift comparisons need a provably NaN-free
        // orderable element — int/string/bool, NOT float: `heap_push`'s
        // dev NaN entry fault is real, and `heap_pop` conservatively keeps
        // float's charge with the rest of the §4b "float orderings carry
        // faults" roster) — arms merged per clippy match_same_arms.
        "sort" | "sorted" | "heap_push" | "heap_pop" => matches!(
            arg_tys.first(),
            Some(Ty::Array(elem)) if scalar_orderable(elem)
        ),
        // NS-A7: `roll` over a provable `Weighted[T]` is total BY
        // CONSTRUCTION — the whole point of the evidence-by-construction
        // shape (its only fault is a wrong-typed operand). `heap_peek`
        // never compares — any provable array discharges it.
        "roll" => matches!(arg_tys.first(), Some(Ty::Weighted(_))),
        "heap_peek" => matches!(arg_tys.first(), Some(Ty::Array(_))),
        _ => false,
    }
}

/// Whether the intrinsic named `name` returns `Option[…]` — the NS-A1
/// absence-shaped verbs plus the `some` constructor and NS-A6's `pick`.
/// Consulted by `option_conditions` (F27/E116) for a direct
/// intrinsic call in condition position; the authoritative per-verb typing
/// rules (element narrowing included) remain `infer_intrinsic`'s arms —
/// this is only the Option-ness bit, kept next to the effect table so the
/// intrinsic facts live in one module.
pub(crate) fn intrinsic_returns_option(name: &str) -> bool {
    matches!(
        name,
        "find"
            | "index_of"
            | "min"
            | "max"
            | "first"
            | "last"
            | "pop"
            | "get"
            | "pick"
            | "some"
            | "non_empty"
            // NS-A7: the heap's absence-shaped verbs (empty → none).
            | "heap_pop"
            | "heap_peek"
    )
}
