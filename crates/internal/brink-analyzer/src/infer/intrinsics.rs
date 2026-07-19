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
                | "RANDOM"
                | "SEED_RANDOM"
                | "LIST_RANDOM"
        );
    IntrinsicEffects { faults, rng_write }
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
    )
}
