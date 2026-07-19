//! The type universe (typed-mode-spec §2/§4) and its unification rule.
//!
//! `Ty` is deliberately small: `int`, `float`, `bool`, `string`, `divert`,
//! nominal `list<L>`, `array<T>`, `map<K, V>`, nominal structs (TM-4),
//! structural `fn(T…): R` function-value types (T1c), plus `Unknown` — no
//! unions. v1 is monomorphic and
//! unification-free of overloading/typeclasses, so the constraint lattice is
//! finite and every join terminates in O(depth) — see spec §2's "constraint
//! solving stays near-linear" ruling.
//!
//! ## Why `join`, not a union-find unifier
//!
//! A textbook HM implementation threads fresh unification variables through
//! a union-find structure so a variable can be equated with another variable
//! before either resolves to a concrete type. That machinery earns its keep
//! when the type language has polymorphism (a variable can specialize
//! differently at different call sites). Spec §2 rules that out for v1: "user
//! code is monomorphic ... every unification variable must resolve to a
//! concrete type per definition. No overloading, no typeclasses." With
//! exactly one openness axis (`Unknown`, which behaves as a bottom/identity
//! element) and no variable-to-variable equating ever required, a
//! monotonically-growing accumulator — start every local at `Unknown`, `join`
//! in the type implied by each use — reaches the same fixpoint a union-find
//! unifier would, without the extra bookkeeping. [`unify`] is that join.

use std::cmp::Ordering;

/// A type in the checker's universe (typed-mode-spec §2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    Float,
    Bool,
    String,
    /// A divert target value (`-> knot` used as a value, not a jump).
    Divert,
    /// A LIST value, nominal per the declaring `LIST` name (spec §2: "nominal
    /// per LIST declaration").
    List(String),
    /// `#[...]` array sigil literal element type.
    Array(Box<Ty>),
    /// `#{...}` map sigil literal key/value types.
    Map(Box<Ty>, Box<Ty>),
    /// A struct value, nominal per the declaring `STRUCT` name (TM-4b,
    /// docs/typed-mode-spec.md §6 — mirrors `List`'s "nominal per LIST
    /// declaration" precedent). Carries only the shape name in this slice —
    /// no field-type table is threaded through inference yet (TM-4c/codegen
    /// concern); a struct-typed slot is still *concrete* for E065/E066
    /// escape-checking purposes (`brink-analyzer::strict::classify`).
    Struct(String),
    /// A function value's type, `fn(T…): R` (T1c, docs/t1c-spec.md §4;
    /// typed-mode-spec §3 reserved the written form). The param row is
    /// **val-only by construction**: every `ref` param of the target is
    /// bound away at `#fn` creation (all refs must bind in the prefix,
    /// E080), so the type form carries no modes — a `Ty::Fn` row describes
    /// exactly the *remaining* (unbound) params a call through the value
    /// must supply.
    Fn(Vec<Ty>, Box<Ty>),
    /// A host-resource handle value, nominal per its manifest-declared kind
    /// name (T1d-2, docs/t1d-spec.md §3: `handle<K>` — mirrors `List`'s
    /// "nominal per LIST declaration" precedent, but the vocabulary lives in
    /// the external manifest, not ink source). Two handle types unify only
    /// when the kind names match exactly; a cross-kind pair is a genuine
    /// structural mismatch and joins to `Conflicted` (#627 lattice) — a
    /// binding declared `handle<AudioInstance>` and one declared
    /// `handle<Timer>` are as incompatible as `int` and `string`.
    Handle(String),
    /// `Option[T]` — the third compiler-known parameterized builtin
    /// (NS-A1, `docs/stdlib-spec.md` §1.4, ruled 2026-07-18), joining
    /// `Array`/`Map` in the static type language. A compiler-owned enum
    /// shape (`none` / `some(T)`), NOT user generics (#1090's door stays
    /// shut). Unifies pointwise on the element like `Array`; against any
    /// non-Option concrete type it is a genuine structural mismatch —
    /// the ruled `Option[T] ≠ T` strictness IS the `Conflicted` join
    /// (display-boundary forgiveness is Track B4 and deliberately absent
    /// from this lattice).
    Option(Box<Ty>),
    /// Not (yet) resolved to a concrete type — legal in this slice (spec
    /// §2: "unresolved -> Unknown, which is LEGAL"). Acts as the join
    /// identity: `unify(Unknown, x) == x`.
    Unknown,
    /// A genuine, irreconcilable type conflict was observed for this slot
    /// (e.g. used as both `int` and `string`) — #627 ruling. A distinct
    /// absorbing lattice point, *not* a synonym for `Unknown`:
    /// `unify(Conflicted, x) == Conflicted` for every `x` (including
    /// `Unknown`), so a conflict can never silently "heal" back to a
    /// concrete type depending on the order observations arrive in. Gradual
    /// mode (every consumer today) treats it exactly like `Unknown` —
    /// strict mode's TM-3 (#619) is the slice that reports it as a
    /// diagnostic; this lattice point only exists so that reporting can be
    /// order-independent when it lands.
    Conflicted,
}

/// `Unknown` is the natural default: every local/return slot starts here
/// before any use narrows it (see the module doc on why `join`-from-`Unknown`
/// stands in for a union-find unifier in this monomorphic, overload-free
/// universe).
impl Default for Ty {
    fn default() -> Self {
        Ty::Unknown
    }
}

impl Ty {
    /// Human-readable name, for future hover/diagnostic surfacing (TM-5).
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Ty::Int => "int".to_string(),
            Ty::Float => "float".to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::String => "string".to_string(),
            Ty::Divert => "divert".to_string(),
            Ty::List(name) => format!("list<{name}>"),
            Ty::Array(elem) => format!("array<{}>", elem.display()),
            Ty::Map(k, v) => format!("map<{}, {}>", k.display(), v.display()),
            Ty::Struct(name) => name.clone(),
            Ty::Fn(params, ret) => {
                let row = params
                    .iter()
                    .map(Ty::display)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({row}): {}", ret.display())
            }
            Ty::Handle(kind) => format!("handle<{kind}>"),
            Ty::Option(elem) => format!("Option[{}]", elem.display()),
            Ty::Unknown => "Unknown".to_string(),
            Ty::Conflicted => "Conflicted".to_string(),
        }
    }

    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    #[must_use]
    pub fn is_conflicted(&self) -> bool {
        matches!(self, Ty::Conflicted)
    }

    /// Gradual/advisory consumers' view (#627 ruling: "Conflicted like
    /// Unknown, zero behavior change today") — a slot that is either
    /// unconstrained or genuinely conflicted carries no usable concrete
    /// type for a consumer that isn't strict-mode's TM-3 conflict reporter.
    /// Strict mode (#619) is the one place these two must stay
    /// distinguished; every other consumer should read this instead of
    /// `is_unknown()`.
    #[must_use]
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Ty::Unknown | Ty::Conflicted)
    }

    #[must_use]
    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float)
    }
}

/// A stable ordering over `Ty` values, used only to keep generated
/// diagnostics/tests deterministic (never for typing decisions). Not derived
/// `Ord` because `Ty` intentionally has no natural total order over its
/// structural variants beyond "same shape, compare recursively".
impl PartialOrd for Ty {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ty {
    fn cmp(&self, other: &Self) -> Ordering {
        fn rank(t: &Ty) -> u8 {
            match t {
                Ty::Int => 0,
                Ty::Float => 1,
                Ty::Bool => 2,
                Ty::String => 3,
                Ty::Divert => 4,
                Ty::List(_) => 5,
                Ty::Array(_) => 6,
                Ty::Map(_, _) => 7,
                Ty::Struct(_) => 8,
                Ty::Fn(..) => 9,
                Ty::Handle(_) => 10,
                Ty::Option(_) => 11,
                Ty::Unknown => 12,
                Ty::Conflicted => 13,
            }
        }
        match (self, other) {
            (Ty::List(a), Ty::List(b))
            | (Ty::Struct(a), Ty::Struct(b))
            | (Ty::Handle(a), Ty::Handle(b)) => a.cmp(b),
            (Ty::Array(a), Ty::Array(b)) | (Ty::Option(a), Ty::Option(b)) => a.cmp(b),
            (Ty::Map(k1, v1), Ty::Map(k2, v2)) => k1.cmp(k2).then_with(|| v1.cmp(v2)),
            (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) => p1.cmp(p2).then_with(|| r1.cmp(r2)),
            _ => rank(self).cmp(&rank(other)),
        }
    }
}

/// Unify (join) two observed types for the same slot.
///
/// This is the coercion lattice of spec §4 applied bidirectionally during
/// inference (strict-mode narrowing is TM-3's job, not this one):
///
/// - `Unknown` is the identity — `unify(Unknown, x) == x` in either
///   position, which is what lets a still-unconstrained local absorb its
///   first real use without special-casing "first observation".
/// - `int -> float` is the one directional numeric coercion ink allows
///   (spec §4): joining `Int` and `Float` in either order produces `Float`.
/// - Two structurally equal types join to themselves (including recursively
///   for `array<T>`/`map<K, V>`, so `array<int>` joined with `array<float>`
///   is `array<float>`, not a hard mismatch).
/// - `Conflicted` is absorbing: joining it with anything (including
///   `Unknown`) stays `Conflicted` (#627 ruling). This is what makes
///   conflict detection order-independent — once a slot has seen a genuine
///   conflict, no later observation (concrete or `Unknown`) can heal it
///   back to a concrete type.
/// - Anything else is a genuine structural mismatch (e.g. `int` vs
///   `string`) and joins to `Ty::Conflicted` (#627 ruling — previously this
///   degraded to `Unknown`, which let a later observation silently absorb
///   and hide the conflict depending on source order). This slice is still
///   advisory-only and raises no new diagnostic for it (spec step 1:
///   "essentially no new user-facing diagnostics") — gradual/advisory
///   consumers read `Conflicted` exactly like `Unknown`
///   ([`Ty::is_unresolved`]); TM-3 (#619) is the slice that turns a
///   `Conflicted` slot into a real strict-mode error.
#[must_use]
pub fn unify(a: &Ty, b: &Ty) -> Ty {
    match (a, b) {
        (Ty::Unknown, x) | (x, Ty::Unknown) => x.clone(),
        (Ty::Conflicted, _) | (_, Ty::Conflicted) => Ty::Conflicted,
        (x, y) if x == y => x.clone(),
        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
        (Ty::Array(x), Ty::Array(y)) => Ty::Array(Box::new(unify(x, y))),
        // Option unifies pointwise on the element, exactly like Array
        // (NS-A1, docs/stdlib-spec.md §1.4). Option vs any non-Option
        // concrete type falls through to `Conflicted` below — the ruled
        // `Option[T] ≠ T` strictness lives in the lattice itself.
        (Ty::Option(x), Ty::Option(y)) => Ty::Option(Box::new(unify(x, y))),
        (Ty::Map(k1, v1), Ty::Map(k2, v2)) => {
            Ty::Map(Box::new(unify(k1, k2)), Box::new(unify(v1, v2)))
        }
        // Fn vs Fn unifies pointwise when the (val-only) rows agree on
        // arity (T1c ruling, docs/t1c-spec.md §4 / the #627 lattice) —
        // params and return join component-wise, so `fn(int): int` and
        // `fn(float): int` join to `fn(float): int` exactly like the
        // Array/Map elements above. Rows of different length are a genuine
        // structural mismatch and fall through to `Conflicted`.
        (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) if p1.len() == p2.len() => Ty::Fn(
            p1.iter().zip(p2).map(|(x, y)| unify(x, y)).collect(),
            Box::new(unify(r1, r2)),
        ),
        // Falls through for e.g. `(Ty::Handle(a), Ty::Handle(b))` with
        // `a != b` (T1d-2, #627 lattice): the `x == y` arm above already
        // handles same-kind handles, so two different kinds — as
        // structurally incompatible as `int` vs `string` — land here, same
        // as every other cross-nominal mismatch.
        _ => Ty::Conflicted,
    }
}

/// Fold `unify` over an iterator of observed types, starting from
/// `Ty::Unknown` (the identity). Used for collection-literal element joins
/// (spec §5: `#[1, 2.0]` is `array<float>`) and for folding multiple
/// observations of the same local/return slot across a body.
#[must_use]
pub fn unify_all(tys: impl IntoIterator<Item = Ty>) -> Ty {
    tys.into_iter().fold(Ty::Unknown, |acc, t| unify(&acc, &t))
}

/// Why an [`coalesce`] application is ill-typed (NS-A1, F19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoalesceError {
    /// The left operand is a concrete non-Option type — coalescing is only
    /// defined over an optional left-hand side (`Option[T] or …`).
    LeftNotOption(Ty),
    /// The element/right types are structurally irreconcilable
    /// (`Option[int] or "text"` — the join of `int` and `string` is
    /// `Conflicted`).
    Mismatch { element: Ty, fallback: Ty },
}

/// The `or`-coalescing TYPING rule (NS-A1; `docs/stdlib-phase-c-findings.md`
/// F19's recommendation, implemented as ruled by wave A1's scope):
///
/// - `(Option[T], T') → join(T, T')` — the Option-then-value form collapses
///   to the value type (the `x or default` 90% case; the `int -> float`
///   directional coercion applies inside the join, so
///   `Option[int] or 1.5` is `float`).
/// - `(Option[T], Option[U]) → Option[join(T, U)]` — the two-Option form
///   keeps optionality, which is what makes chaining work:
///   `a.get(k) or a.get(k2) or default` associates left, staying
///   `Option[V]` until the final non-Option fallback collapses it.
/// - Left-associative by construction: a chain is just repeated
///   application, so no explicit associativity machinery is needed.
/// - Gradual escape hatches: an `Unknown` left operand yields `Unknown`
///   (nothing is known to check — same posture as every other gradual
///   position); `Conflicted` anywhere stays `Conflicted` via the join.
///
/// **Typing only — no surface spelling.** The brink dialect has no `or`
/// coalescing operator to hang this on (`InfixOp::Or` is ink's boolean
/// `||`, oracle-frozen); the surface spelling is Track B1
/// (`docs/stdlib-sequencing.md` §3), which will consume exactly this rule.
pub fn coalesce(lhs: &Ty, rhs: &Ty) -> Result<Ty, CoalesceError> {
    match (lhs, rhs) {
        // Two-Option form: keep optionality, join elements.
        (Ty::Option(elem), Ty::Option(relem)) => Ok(Ty::Option(Box::new(unify(elem, relem)))),
        // Option-then-value form: collapse to the value type when the
        // element and fallback reconcile (join is not Conflicted).
        (Ty::Option(elem), _) => {
            let joined = unify(elem, rhs);
            if joined.is_conflicted() && !elem.is_conflicted() && !rhs.is_conflicted() {
                Err(CoalesceError::Mismatch {
                    element: (**elem).clone(),
                    fallback: rhs.clone(),
                })
            } else {
                Ok(joined)
            }
        }
        // Gradual: an unresolved left side types as itself (Unknown stays
        // Unknown, Conflicted stays Conflicted) — strict-mode escape
        // reporting owns surfacing those, same as every consumer.
        (Ty::Unknown, _) => Ok(Ty::Unknown),
        (Ty::Conflicted, _) => Ok(Ty::Conflicted),
        (other, _) => Err(CoalesceError::LeftNotOption(other.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_identity() {
        assert_eq!(unify(&Ty::Unknown, &Ty::Int), Ty::Int);
        assert_eq!(unify(&Ty::Int, &Ty::Unknown), Ty::Int);
        assert_eq!(unify(&Ty::Unknown, &Ty::Unknown), Ty::Unknown);
    }

    #[test]
    fn int_float_join_is_directional_to_float() {
        assert_eq!(unify(&Ty::Int, &Ty::Float), Ty::Float);
        assert_eq!(unify(&Ty::Float, &Ty::Int), Ty::Float);
    }

    #[test]
    fn equal_types_join_to_themselves() {
        assert_eq!(unify(&Ty::String, &Ty::String), Ty::String);
        assert_eq!(
            unify(&Ty::List("Weathers".into()), &Ty::List("Weathers".into())),
            Ty::List("Weathers".into())
        );
    }

    #[test]
    fn structural_mismatch_yields_conflicted_not_unknown() {
        // #627 ruling: a genuinely disjoint concrete pair is a distinct
        // absorbing lattice point, `Ty::Conflicted` — no longer degrades to
        // `Unknown` (which would let a later observation silently "heal"
        // the slot back to a concrete type, hiding the conflict).
        assert_eq!(unify(&Ty::Int, &Ty::String), Ty::Conflicted);
        assert_eq!(unify(&Ty::Bool, &Ty::Divert), Ty::Conflicted);
    }

    #[test]
    fn conflicted_absorbs_unknown_and_everything_else() {
        // `Conflicted JOIN anything = Conflicted` (#627 ruling) — it is a
        // stronger absorbing point than `Unknown`: `Unknown` is the join
        // *identity*, `Conflicted` is the join *absorber*.
        assert_eq!(unify(&Ty::Conflicted, &Ty::Unknown), Ty::Conflicted);
        assert_eq!(unify(&Ty::Unknown, &Ty::Conflicted), Ty::Conflicted);
        assert_eq!(unify(&Ty::Conflicted, &Ty::Int), Ty::Conflicted);
        assert_eq!(unify(&Ty::Int, &Ty::Conflicted), Ty::Conflicted);
        assert_eq!(unify(&Ty::Conflicted, &Ty::Conflicted), Ty::Conflicted);
    }

    #[test]
    fn conflict_detection_is_order_independent() {
        // #627 ruling: permuting the order observations are folded in must
        // not change whether a conflict is detected — a genuine int/string
        // conflict must not "self-heal" depending on which observation
        // arrives first (the bug this issue exists to close).
        let orderings: [&[Ty]; 6] = [
            &[Ty::Int, Ty::String, Ty::Int],
            &[Ty::String, Ty::Int, Ty::Int],
            &[Ty::Int, Ty::Int, Ty::String],
            &[Ty::String, Ty::Int, Ty::Int],
            &[Ty::Int, Ty::String, Ty::Int],
            &[Ty::String, Ty::String, Ty::Int],
        ];
        for ordering in orderings {
            assert_eq!(
                unify_all(ordering.iter().cloned()),
                Ty::Conflicted,
                "order {ordering:?} must detect the conflict"
            );
        }
    }

    #[test]
    fn conflict_detection_survives_unknown_interleaving_in_any_order() {
        // Interleaving `Unknown` observations (an unconstrained/unused
        // intermediate use) anywhere in the sequence must not mask the
        // conflict — `Unknown` is a true identity, never a reset.
        let orderings: [&[Ty]; 4] = [
            &[Ty::Unknown, Ty::Int, Ty::Unknown, Ty::String],
            &[Ty::Int, Ty::Unknown, Ty::String, Ty::Unknown],
            &[Ty::String, Ty::Unknown, Ty::Unknown, Ty::Int],
            &[Ty::Unknown, Ty::Unknown, Ty::String, Ty::Int],
        ];
        for ordering in orderings {
            assert_eq!(unify_all(ordering.iter().cloned()), Ty::Conflicted);
        }
    }

    #[test]
    fn array_and_map_join_recursively() {
        assert_eq!(
            unify(
                &Ty::Array(Box::new(Ty::Int)),
                &Ty::Array(Box::new(Ty::Float))
            ),
            Ty::Array(Box::new(Ty::Float))
        );
        assert_eq!(
            unify(
                &Ty::Map(Box::new(Ty::Int), Box::new(Ty::String)),
                &Ty::Map(Box::new(Ty::Unknown), Box::new(Ty::String))
            ),
            Ty::Map(Box::new(Ty::Int), Box::new(Ty::String))
        );
    }

    #[test]
    fn unify_all_folds_left_to_right_from_unknown() {
        assert_eq!(unify_all([Ty::Int, Ty::Float]), Ty::Float);
        assert_eq!(unify_all(Vec::<Ty>::new()), Ty::Unknown);
        assert_eq!(unify_all([Ty::Bool]), Ty::Bool);
    }

    // ─── T1c `Ty::Fn` (docs/t1c-spec.md §4) ────────────────────────────

    fn fn_ty(params: &[Ty], ret: Ty) -> Ty {
        Ty::Fn(params.to_vec(), Box::new(ret))
    }

    #[test]
    fn fn_unifies_pointwise_including_the_directional_numeric_join() {
        // Params and return join component-wise, exactly like Array/Map
        // elements — the int -> float directional coercion applies inside
        // the row too.
        assert_eq!(
            unify(&fn_ty(&[Ty::Int], Ty::Int), &fn_ty(&[Ty::Float], Ty::Int)),
            fn_ty(&[Ty::Float], Ty::Int)
        );
        assert_eq!(
            unify(
                &fn_ty(&[Ty::String], Ty::Int),
                &fn_ty(&[Ty::String], Ty::Float)
            ),
            fn_ty(&[Ty::String], Ty::Float)
        );
    }

    #[test]
    fn fn_unknown_row_slots_absorb_concrete_ones() {
        assert_eq!(
            unify(
                &fn_ty(&[Ty::Unknown], Ty::Unknown),
                &fn_ty(&[Ty::Int], Ty::Bool)
            ),
            fn_ty(&[Ty::Int], Ty::Bool)
        );
    }

    #[test]
    fn fn_arity_mismatch_is_conflicted() {
        assert_eq!(
            unify(&fn_ty(&[Ty::Int], Ty::Int), &fn_ty(&[], Ty::Int)),
            Ty::Conflicted
        );
    }

    #[test]
    fn fn_vs_other_concrete_is_conflicted_per_the_627_lattice() {
        assert_eq!(unify(&fn_ty(&[], Ty::Int), &Ty::Int), Ty::Conflicted);
        assert_eq!(unify(&Ty::String, &fn_ty(&[], Ty::Int)), Ty::Conflicted);
        assert_eq!(
            unify(&fn_ty(&[], Ty::Int), &Ty::Array(Box::new(Ty::Int))),
            Ty::Conflicted
        );
    }

    #[test]
    fn fn_conflict_inside_the_row_stays_inside_the_row() {
        // A disagreeing param slot conflicts *pointwise* — the row shape
        // survives, mirroring `Array(Conflicted)` for `#[1, "a"]`, and the
        // recursive strict-mode classify walk is what surfaces it.
        assert_eq!(
            unify(&fn_ty(&[Ty::Int], Ty::Int), &fn_ty(&[Ty::String], Ty::Int)),
            fn_ty(&[Ty::Conflicted], Ty::Int)
        );
    }

    #[test]
    fn fn_unify_is_order_independent() {
        // Extends the #627 order-independence property to `Fn` rows: every
        // permutation of observations must reach the same join.
        let a = fn_ty(&[Ty::Int, Ty::String], Ty::Int);
        let b = fn_ty(&[Ty::Float, Ty::String], Ty::Unknown);
        let c = fn_ty(&[Ty::Unknown, Ty::String], Ty::Float);
        let expected = fn_ty(&[Ty::Float, Ty::String], Ty::Float);
        let orderings: [[&Ty; 3]; 6] = [
            [&a, &b, &c],
            [&a, &c, &b],
            [&b, &a, &c],
            [&b, &c, &a],
            [&c, &a, &b],
            [&c, &b, &a],
        ];
        for ordering in orderings {
            assert_eq!(
                unify_all(ordering.iter().map(|t| (*t).clone())),
                expected,
                "order {ordering:?} must reach the same join"
            );
        }
    }

    #[test]
    fn fn_conflict_detection_is_order_independent() {
        // A genuine row conflict (int vs string in the same slot) must be
        // detected regardless of observation order, and must never heal.
        let a = fn_ty(&[Ty::Int], Ty::Int);
        let b = fn_ty(&[Ty::String], Ty::Int);
        let u = fn_ty(&[Ty::Unknown], Ty::Unknown);
        let expected = fn_ty(&[Ty::Conflicted], Ty::Int);
        let orderings: [[&Ty; 3]; 6] = [
            [&a, &b, &u],
            [&a, &u, &b],
            [&b, &a, &u],
            [&b, &u, &a],
            [&u, &a, &b],
            [&u, &b, &a],
        ];
        for ordering in orderings {
            assert_eq!(
                unify_all(ordering.iter().map(|t| (*t).clone())),
                expected,
                "order {ordering:?} must detect the row conflict"
            );
        }
    }

    #[test]
    fn fn_display_is_the_reserved_written_form() {
        assert_eq!(fn_ty(&[Ty::Int], Ty::Int).display(), "fn(int): int");
        assert_eq!(
            fn_ty(&[Ty::Int, Ty::String], Ty::Bool).display(),
            "fn(int, string): bool"
        );
        assert_eq!(fn_ty(&[], Ty::Float).display(), "fn(): float");
    }

    // ─── T1d-2 `Ty::Handle` (docs/t1d-spec.md §3) ──────────────────────

    #[test]
    fn handle_same_kind_unifies_to_itself() {
        let h = Ty::Handle("AudioInstance".to_string());
        assert_eq!(unify(&h, &h), h);
        assert_eq!(unify(&Ty::Unknown, &h), h);
        assert_eq!(unify(&h, &Ty::Unknown), h);
    }

    #[test]
    fn handle_cross_kind_is_conflicted_not_unknown() {
        // #627 lattice: a genuinely different handle kind is a structural
        // mismatch, exactly like `int` vs `string` — never a silent
        // `Unknown` degradation.
        let a = Ty::Handle("AudioInstance".to_string());
        let b = Ty::Handle("Timer".to_string());
        assert_eq!(unify(&a, &b), Ty::Conflicted);
        assert_eq!(unify(&b, &a), Ty::Conflicted);
    }

    #[test]
    fn handle_vs_other_concrete_type_is_conflicted() {
        let h = Ty::Handle("AudioInstance".to_string());
        assert_eq!(unify(&h, &Ty::Int), Ty::Conflicted);
        assert_eq!(unify(&Ty::String, &h), Ty::Conflicted);
        assert_eq!(unify(&h, &Ty::Array(Box::new(Ty::Int))), Ty::Conflicted);
    }

    #[test]
    fn handle_conflicted_absorbs_everything() {
        let h = Ty::Handle("AudioInstance".to_string());
        assert_eq!(unify(&Ty::Conflicted, &h), Ty::Conflicted);
        assert_eq!(unify(&h, &Ty::Conflicted), Ty::Conflicted);
    }

    #[test]
    fn handle_display_carries_the_kind_name() {
        assert_eq!(
            Ty::Handle("AudioInstance".to_string()).display(),
            "handle<AudioInstance>"
        );
    }

    // ─── NS-A1 `Ty::Option` (docs/stdlib-spec.md §1.4) ─────────────────

    fn opt(t: Ty) -> Ty {
        Ty::Option(Box::new(t))
    }

    #[test]
    fn option_unifies_pointwise_like_array() {
        assert_eq!(opt(Ty::Int).display(), "Option[int]");
        assert_eq!(unify(&opt(Ty::Int), &opt(Ty::Int)), opt(Ty::Int));
        // The int -> float directional join applies inside the element.
        assert_eq!(unify(&opt(Ty::Int), &opt(Ty::Float)), opt(Ty::Float));
        // Unknown element absorbs a concrete one.
        assert_eq!(unify(&opt(Ty::Unknown), &opt(Ty::String)), opt(Ty::String));
        assert_eq!(unify(&Ty::Unknown, &opt(Ty::Int)), opt(Ty::Int));
    }

    #[test]
    fn option_vs_bare_type_is_conflicted_the_ruled_strictness() {
        // `Option[T] ≠ T` — everywhere, no display-boundary forgiveness in
        // the lattice (that's Track B4, cut by position at a later layer).
        assert_eq!(unify(&opt(Ty::Int), &Ty::Int), Ty::Conflicted);
        assert_eq!(unify(&Ty::Int, &opt(Ty::Int)), Ty::Conflicted);
        assert_eq!(unify(&opt(Ty::String), &Ty::String), Ty::Conflicted);
        assert_eq!(
            unify(&opt(Ty::Int), &Ty::Array(Box::new(Ty::Int))),
            Ty::Conflicted
        );
    }

    #[test]
    fn option_nests_like_any_parameterized_builtin() {
        // Option[Option[int]] is a real type; joining it with Option[int]
        // conflicts pointwise in the element slot.
        let nested = opt(opt(Ty::Int));
        assert_eq!(nested.display(), "Option[Option[int]]");
        assert_eq!(unify(&nested, &nested), nested);
        assert_eq!(unify(&nested, &opt(Ty::Int)), opt(Ty::Conflicted));
    }

    #[test]
    fn option_element_conflict_stays_inside_the_element() {
        // Mirrors Array(Conflicted): the Option shape survives, the element
        // slot carries the conflict for the strict classify walk to find.
        assert_eq!(unify(&opt(Ty::Int), &opt(Ty::String)), opt(Ty::Conflicted));
    }

    // ─── NS-A1 `or`-coalescing typing (F19 — typing only, no spelling) ──

    #[test]
    fn coalesce_option_then_value_collapses_to_the_value_type() {
        assert_eq!(coalesce(&opt(Ty::Int), &Ty::Int), Ok(Ty::Int));
        // The directional numeric join applies.
        assert_eq!(coalesce(&opt(Ty::Int), &Ty::Float), Ok(Ty::Float));
        assert_eq!(coalesce(&opt(Ty::String), &Ty::String), Ok(Ty::String));
    }

    #[test]
    fn coalesce_two_options_keeps_optionality_for_chaining() {
        assert_eq!(coalesce(&opt(Ty::Int), &opt(Ty::Int)), Ok(opt(Ty::Int)));
        assert_eq!(coalesce(&opt(Ty::Int), &opt(Ty::Float)), Ok(opt(Ty::Float)));
    }

    #[test]
    fn coalesce_chains_left_associatively() {
        // a.get(k) or a.get(k2) or 0  ⟶  ((Option[int] or Option[int]) or int)
        let step1 = coalesce(&opt(Ty::Int), &opt(Ty::Int)).expect("chain step");
        assert_eq!(step1, opt(Ty::Int));
        assert_eq!(coalesce(&step1, &Ty::Int), Ok(Ty::Int));
    }

    #[test]
    fn coalesce_mismatched_fallback_is_an_error() {
        assert_eq!(
            coalesce(&opt(Ty::Int), &Ty::String),
            Err(CoalesceError::Mismatch {
                element: Ty::Int,
                fallback: Ty::String,
            })
        );
    }

    #[test]
    fn coalesce_non_option_left_is_an_error() {
        assert_eq!(
            coalesce(&Ty::Int, &Ty::Int),
            Err(CoalesceError::LeftNotOption(Ty::Int))
        );
        assert_eq!(
            coalesce(&Ty::Array(Box::new(Ty::Int)), &Ty::Int),
            Err(CoalesceError::LeftNotOption(Ty::Array(Box::new(Ty::Int))))
        );
    }

    #[test]
    fn coalesce_gradual_escapes() {
        // Unknown left: nothing to check — gradual posture.
        assert_eq!(coalesce(&Ty::Unknown, &Ty::Int), Ok(Ty::Unknown));
        // Unknown element/fallback join without erroring.
        assert_eq!(coalesce(&opt(Ty::Unknown), &Ty::Int), Ok(Ty::Int));
        assert_eq!(coalesce(&opt(Ty::Int), &Ty::Unknown), Ok(Ty::Int));
        // Conflicted stays absorbing, never "heals" into an error report
        // here (strict-mode escape reporting owns it).
        assert_eq!(coalesce(&Ty::Conflicted, &Ty::Int), Ok(Ty::Conflicted));
        assert_eq!(coalesce(&opt(Ty::Conflicted), &Ty::Int), Ok(Ty::Conflicted));
    }

    #[test]
    fn fn_composes_with_handle_typed_params() {
        // Ty::Fn composition with handle-typed params (T1d-2): the
        // pre-existing pointwise Fn unify needs no special-casing — each
        // row slot unifies via the generic `unify` recursion, so a
        // handle-typed param slot behaves exactly like any other Ty.
        let a = fn_ty(&[Ty::Handle("AudioInstance".to_string())], Ty::Bool);
        let b = fn_ty(&[Ty::Handle("AudioInstance".to_string())], Ty::Bool);
        assert_eq!(unify(&a, &b), a);

        let mismatched = fn_ty(&[Ty::Handle("Timer".to_string())], Ty::Bool);
        assert_eq!(unify(&a, &mismatched), fn_ty(&[Ty::Conflicted], Ty::Bool));
    }
}
