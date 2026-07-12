//! The type universe (typed-mode-spec §2/§4) and its unification rule.
//!
//! `Ty` is deliberately small: `int`, `float`, `bool`, `string`, `divert`,
//! nominal `list<L>`, `array<T>`, `map<K, V>`, plus `Unknown` — no function
//! types (T1c), no structs (TM-4), no unions. v1 is monomorphic and
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
    /// Not (yet) resolved to a concrete type — legal in this slice (spec
    /// §2: "unresolved -> Unknown, which is LEGAL"). Acts as the join
    /// identity: `unify(Unknown, x) == x`.
    Unknown,
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
            Ty::Unknown => "Unknown".to_string(),
        }
    }

    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
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
                Ty::Unknown => 8,
            }
        }
        match (self, other) {
            (Ty::List(a), Ty::List(b)) => a.cmp(b),
            (Ty::Array(a), Ty::Array(b)) => a.cmp(b),
            (Ty::Map(k1, v1), Ty::Map(k2, v2)) => k1.cmp(k2).then_with(|| v1.cmp(v2)),
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
/// - Anything else is a genuine structural mismatch (e.g. `int` vs
///   `string`). This slice is advisory-only and never raises a diagnostic
///   for it (spec step 1: "essentially no new user-facing diagnostics") —
///   it degrades to `Unknown`, which is legal (spec §2) and exactly the
///   escape hatch TM-3 will later turn into a real error.
#[must_use]
pub fn unify(a: &Ty, b: &Ty) -> Ty {
    match (a, b) {
        (Ty::Unknown, x) | (x, Ty::Unknown) => x.clone(),
        (x, y) if x == y => x.clone(),
        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
        (Ty::Array(x), Ty::Array(y)) => Ty::Array(Box::new(unify(x, y))),
        (Ty::Map(k1, v1), Ty::Map(k2, v2)) => {
            Ty::Map(Box::new(unify(k1, k2)), Box::new(unify(v1, v2)))
        }
        _ => Ty::Unknown,
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
    fn structural_mismatch_degrades_to_unknown_not_error() {
        // Advisory-only: TM-1 never hard-errors on a real mismatch, it
        // reports Unknown (spec: "unresolved -> Unknown, which is LEGAL").
        assert_eq!(unify(&Ty::Int, &Ty::String), Ty::Unknown);
        assert_eq!(unify(&Ty::Bool, &Ty::Divert), Ty::Unknown);
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
}
