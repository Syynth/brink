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
use std::collections::BTreeSet;

use brink_format::DefinitionId;

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
    ///
    /// The third component is the **effect row riding the type**
    /// (`docs/effects-spec.md` §5, issue #1680 step 3) — see [`FnRow`].
    /// It is joined by [`unify`] alongside the params and the return, and
    /// is deliberately **not** part of assignability: see [`assignable`].
    Fn(Vec<Ty>, Box<Ty>, FnRow),
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
    /// An integer range value (NS-A5, `docs/stdlib-spec.md` §7 — F7 ruled
    /// 2026-07-19: ranges are a real Value kind). The `non_empty` flag is
    /// the language's **first value refinement**: `true` is checker-minted
    /// EVIDENCE that the range denotes at least one element — a refined
    /// *view* over the same runtime value, never a second value kind. The
    /// checker mints it in exactly two places (the closed-refinement
    /// doctrine): a range literal with provably-inhabited bounds, and the
    /// `some` payload of `non_empty(r)`. Under gradual typing the flag is
    /// inert (F8 — the runtime fault is the residual); under `types =
    /// strict` `rand::int` demands it (E117).
    Range {
        /// Checker-minted inhabitedness evidence (`NonEmptyRange`, the S2
        /// spelling). Joins with `&&` in `unify`: evidence survives only
        /// if EVERY observed source carries it.
        non_empty: bool,
    },
    /// Not (yet) resolved to a concrete type — legal in this slice (spec
    /// `Weighted[T]` — the weighted table builtin (NS-A7,
    /// `docs/stdlib-spec.md` §8): compiler-known and parameterized like
    /// `Option`/`Array`, NOT user generics. Unifies pointwise on the value
    /// element; against any other concrete type it is a genuine structural
    /// mismatch (`Conflicted`). v1 is construct-and-roll — the only
    /// producer is the `weighted(…)` intrinsic, the only consumer `roll`.
    Weighted(Box<Ty>),
    /// A numeric-tower value kind (NS-A8, `docs/tower-mini-spec.md`):
    /// `vec2`/`vec3`/`vec4`/`quat`/`mat2`/`mat3`/`mat4` — seven closed
    /// compiler-known kinds carried by one variant (they behave identically
    /// in the lattice: nominal scalar-like leaves; no coercion into or out
    /// of them, so a tower-vs-anything-else join is `Conflicted`).
    Tower(TowerTy),
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

/// The effect row riding a [`Ty::Fn`] — `docs/effects-spec.md` §5 ("rows ride
/// the unifier — the heap answer"), issue #1680 step 3.
///
/// **What a row is, concretely.** §7 rules that *"the runtime never computes a
/// row … a live fn value is a token; its row is a **table lookup**"* in the
/// shipped `DefinitionId → row` table. So the thing a *type* has to carry is
/// not a computed [`EffectRow`](super::EffectRow) but the **set of in-project
/// creation targets** whose fn values may inhabit the slot — the keys that
/// table is looked up by. §6.1a is what makes that the right (and the only
/// acyclic) choice: *"§6.1 fixes every fn value's row at its creation site and
/// creation sites are **syntactic**: `#fn(g)` names `g` literally, and `bind`
/// copies from an already-known value rather than naming a new target."* A
/// target set is therefore structural evidence, never an inferred row, so
/// growing it onto `Ty` cannot put an inferred row inside the
/// `call_graph → scc_membership → solve_scc` fixpoint the way Fork A's
/// rejected shape would have.
///
/// **The lattice.** `unknown` is the top element (the conservative-total floor
/// of §3): *"some fn value from a source this slot's type cannot name may
/// reach here"*. [`FnRow::join`] is set union with `unknown` absorbing, which
/// is exactly §5's *"a cell or collection's element type accumulates the join
/// of every fn value assigned into it, through copies, parameters, returns,
/// and nesting"*. That absorption is on the `unknown` **row**, not on every
/// untraceable-looking write: a write typed plain `Ty::Unknown` (an
/// unresolved reference, or an unregistered `EXTERNAL`'s return) unifies
/// through `Ty::Unknown`'s own identity arm, not through this lattice at
/// all, and so leaves the other operand's row untouched rather than
/// poisoning it to `unknown`.
///
/// **Not yet read by the effect walk.** `def_effect_atoms` deliberately runs
/// the body walk with empty globals and empty signatures (§6.1a's acyclicity
/// is load-bearing on that), so a `#fn` literal types as `Unknown` there and
/// this row is invisible to effect inference as currently constructed.
/// Wiring §6 mechanism 3 — the heap — means deciding *which stratum* reads
/// the type-carried row, which the 2026-07-28 sitting did not settle; see
/// `docs/effects-spec.md` §6.1c.
///
/// Represented as an `Option<Box<…>>` so the common `Ty::Fn` stays one
/// pointer wider rather than three words wider — `Ty` is copied constantly
/// through the join.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[expect(
    clippy::box_collection,
    reason = "the box is the point: it keeps `Ty::Fn` one pointer wider \
              instead of three words wider, and the unknown top element — \
              which is what almost every `Ty::Fn` carries — costs nothing \
              at all through the `Option` niche"
)]
pub struct FnRow(Option<Box<BTreeSet<DefinitionId>>>);

impl FnRow {
    /// The top element: nothing is known about where the values inhabiting
    /// this slot were created. Also [`Default`], so every `Ty::Fn` built
    /// without explicit creation evidence is conservative by construction.
    #[must_use]
    pub fn unknown() -> Self {
        FnRow(None)
    }

    /// The row of a fn value created *here*, at a `#fn(target, …)` literal
    /// naming `target` — the one syntactic creation form (§6.1a).
    #[must_use]
    pub fn of_target(target: DefinitionId) -> Self {
        FnRow(Some(Box::new(BTreeSet::from([target]))))
    }

    /// The empty row: this slot provably holds no in-project fn value. Only
    /// reachable through a join that started here; never minted directly.
    #[must_use]
    pub fn empty() -> Self {
        FnRow(Some(Box::new(BTreeSet::new())))
    }

    /// `true` when this row is the top element ([`FnRow::unknown`]).
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        self.0.is_none()
    }

    /// The creation targets, or `None` when the row is the top element.
    /// A caller that wants the conservative reading must treat `None` as
    /// *"every target"*, never as *"no targets"*.
    #[must_use]
    pub fn targets(&self) -> Option<&BTreeSet<DefinitionId>> {
        self.0.as_deref()
    }

    /// Least upper bound: union of the target sets, with the top element
    /// absorbing. Monotone over a finite per-project lattice, so folding it
    /// through [`unify`] terminates for the same reason the rest of the join
    /// does.
    #[must_use]
    pub fn join(&self, other: &FnRow) -> FnRow {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => FnRow(Some(Box::new(a.union(b).copied().collect()))),
            _ => FnRow::unknown(),
        }
    }
}

/// The seven numeric-tower kinds (NS-A8) carried by [`Ty::Tower`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TowerTy {
    Vec2,
    Vec3,
    Vec4,
    Quat,
    Mat2,
    Mat3,
    Mat4,
}

impl TowerTy {
    /// The global type name (`vec2` … `mat4`) — also the constructor verb.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            TowerTy::Vec2 => "vec2",
            TowerTy::Vec3 => "vec3",
            TowerTy::Vec4 => "vec4",
            TowerTy::Quat => "quat",
            TowerTy::Mat2 => "mat2",
            TowerTy::Mat3 => "mat3",
            TowerTy::Mat4 => "mat4",
        }
    }

    /// Resolve a tower type name (`vec2` … `mat4`) to its kind.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "vec2" => Some(TowerTy::Vec2),
            "vec3" => Some(TowerTy::Vec3),
            "vec4" => Some(TowerTy::Vec4),
            "quat" => Some(TowerTy::Quat),
            "mat2" => Some(TowerTy::Mat2),
            "mat3" => Some(TowerTy::Mat3),
            "mat4" => Some(TowerTy::Mat4),
            _ => None,
        }
    }
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
            // The effect row is deliberately absent: `display` renders the
            // *written* type language (what an annotation can spell, what a
            // diagnostic quotes back), and there is no surface syntax for a
            // row. Rendering it here would churn every `E063`/`E066` message
            // for a component the author never wrote.
            Ty::Fn(params, ret, _) => {
                let row = params
                    .iter()
                    .map(Ty::display)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({row}): {}", ret.display())
            }
            Ty::Handle(kind) => format!("handle<{kind}>"),
            Ty::Option(elem) => format!("Option[{}]", elem.display()),
            Ty::Range { non_empty: false } => "range".to_string(),
            Ty::Range { non_empty: true } => "NonEmptyRange".to_string(),
            Ty::Tower(kind) => kind.name().to_string(),
            Ty::Weighted(elem) => format!("Weighted[{}]", elem.display()),
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
                Ty::Range { .. } => 12,
                Ty::Tower(_) => 13,
                Ty::Weighted(_) => 14,
                Ty::Unknown => 15,
                Ty::Conflicted => 16,
            }
        }
        match (self, other) {
            (Ty::List(a), Ty::List(b))
            | (Ty::Struct(a), Ty::Struct(b))
            | (Ty::Handle(a), Ty::Handle(b)) => a.cmp(b),
            (Ty::Array(a), Ty::Array(b))
            | (Ty::Option(a), Ty::Option(b))
            | (Ty::Weighted(a), Ty::Weighted(b)) => a.cmp(b),
            (Ty::Range { non_empty: a }, Ty::Range { non_empty: b }) => a.cmp(b),
            (Ty::Tower(a), Ty::Tower(b)) => a.cmp(b),
            (Ty::Map(k1, v1), Ty::Map(k2, v2)) => k1.cmp(k2).then_with(|| v1.cmp(v2)),
            // The effect row participates so that `cmp` stays consistent
            // with the derived `PartialEq` — two `Ty::Fn`s that differ only
            // in their row are *not* equal, and an `Ordering::Equal` for an
            // unequal pair would break any `BTreeMap`/`BTreeSet` keyed by
            // `Ty`.
            (Ty::Fn(p1, r1, e1), Ty::Fn(p2, r2, e2)) => {
                p1.cmp(p2).then_with(|| r1.cmp(r2)).then_with(|| e1.cmp(e2))
            }
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
        // Weighted unifies pointwise on the value element, exactly like
        // Array/Option (NS-A7, docs/stdlib-spec.md §8); against any other
        // concrete type it falls through to `Conflicted` below.
        (Ty::Weighted(x), Ty::Weighted(y)) => Ty::Weighted(Box::new(unify(x, y))),
        // Ranges unify with ranges; the refinement evidence joins with
        // `&&` (NS-A5): a slot is `NonEmptyRange` only if EVERY observed
        // source carries the evidence — one possibly-empty assignment
        // demotes the join to plain `range`, exactly the sound direction
        // (evidence can be lost at a join, never fabricated). The `x == y`
        // arm above already handles the equal-flag cases.
        (Ty::Range { non_empty: a }, Ty::Range { non_empty: b }) => Ty::Range {
            non_empty: *a && *b,
        },
        (Ty::Map(k1, v1), Ty::Map(k2, v2)) => {
            Ty::Map(Box::new(unify(k1, k2)), Box::new(unify(v1, v2)))
        }
        // Fn vs Fn unifies pointwise when the (val-only) param rows agree on
        // arity (T1c ruling, docs/t1c-spec.md §4 / the #627 lattice) —
        // params and return join component-wise, so `fn(int): int` and
        // `fn(float): int` join to `fn(float): int` exactly like the
        // Array/Map elements above. Param rows of different length are a
        // genuine structural mismatch and fall through to `Conflicted`.
        //
        // The **effect row** joins alongside them (`docs/effects-spec.md`
        // §5, issue #1680): "a cell or collection's element type accumulates
        // the join of every fn value assigned into it, through copies,
        // parameters, returns, and nesting — because typing already follows
        // values". `FnRow::join` is set union with the unknown top element
        // absorbing, so a slot that has seen one untraceable source stays
        // conservative no matter what else flows in (§3).
        (Ty::Fn(p1, r1, e1), Ty::Fn(p2, r2, e2)) if p1.len() == p2.len() => Ty::Fn(
            p1.iter().zip(p2).map(|(x, y)| unify(x, y)).collect(),
            Box::new(unify(r1, r2)),
            e1.join(e2),
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

/// Rewrite every [`Ty::Fn`] anywhere inside `ty` to carry the top
/// [`FnRow`] — the canonical form used to compare two types *modulo* their
/// effect rows. Recurses through every structural variant so a row nested
/// inside `array<fn(): int>` or `fn(fn(): int): int` is erased too.
///
/// **No wildcard arm** (issue #1758). `Option`, `Weighted`, and `Tower` were
/// each added to [`Ty`] over time, and any of them (or a future variant)
/// could nest a `Ty` the way `Array`/`Map`/`Fn` do — a `_ => ty.clone()`
/// catch-all would let such an addition silently fall through as a no-op
/// erasure instead of failing to compile, reintroducing the exact
/// spurious-`E063` class #1754 fixed (a nested [`FnRow`] surviving erasure
/// and then getting compared structurally by [`assignable`]'s callers).
/// Every nominal leaf is therefore listed explicitly: adding a new variant
/// that nests a `Ty` is a compile error here until someone decides whether
/// it needs an erasing arm.
#[must_use]
pub fn erase_fn_rows(ty: &Ty) -> Ty {
    match ty {
        Ty::Fn(params, ret, _) => Ty::Fn(
            params.iter().map(erase_fn_rows).collect(),
            Box::new(erase_fn_rows(ret)),
            FnRow::unknown(),
        ),
        Ty::Array(elem) => Ty::Array(Box::new(erase_fn_rows(elem))),
        Ty::Option(elem) => Ty::Option(Box::new(erase_fn_rows(elem))),
        Ty::Weighted(elem) => Ty::Weighted(Box::new(erase_fn_rows(elem))),
        Ty::Map(k, v) => Ty::Map(Box::new(erase_fn_rows(k)), Box::new(erase_fn_rows(v))),
        Ty::Int
        | Ty::Float
        | Ty::Bool
        | Ty::String
        | Ty::Divert
        | Ty::List(_)
        | Ty::Struct(_)
        | Ty::Handle(_)
        | Ty::Range { .. }
        | Ty::Tower(_)
        | Ty::Unknown
        | Ty::Conflicted => ty.clone(),
    }
}

/// Is a value of type `source` legal in a slot declared `target`?
///
/// The one assignability predicate every "does this argument fit this
/// parameter" check shares — `annotations::report_if_mismatched` (`E063`),
/// the two `ValueCallKind::ArgMismatch` sites in [`super::body`], and the
/// `E071` struct-field check. It is [`unify`] plus the directional reading
/// *"the join must not widen the target"*, which is what makes the one legal
/// numeric coercion (`int` into a `float` slot) pass while `float` into an
/// `int` slot fails.
///
/// **Row-insensitive** (issue #1680 step 2). Two `fn(): int` values created
/// at different targets have different [`FnRow`]s, so their join is a third
/// row that equals neither operand — a *structural* `unify(target, source)
/// == target` test would therefore report a mismatch on perfectly correct
/// code, and `strict.rs` promotes that mismatch to an `E063` **error** under
/// `types = strict`. Effect rows are inferred provenance, never part of the
/// written type language (see [`Ty::display`]), so they must not decide
/// assignability: both sides are erased before the comparison.
#[must_use]
pub fn assignable(target: &Ty, source: &Ty) -> bool {
    erase_fn_rows(&unify(target, source)) == erase_fn_rows(target)
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
/// **Surface spelling landed in B1** (issue #1460): `InfixOp::Coalesce`,
/// produced only by native lowering (`hir::lower_native::expr::infix_op`,
/// the `KW_OR` token). The brink/ink dialect still has no coalescing
/// operator to hang this on — `InfixOp::Or` there stays ink's boolean
/// `||`, oracle-frozen and untouched — so this rule is consumed
/// exclusively by [`super::body::InferPass::infer_infix`]'s
/// `InfixOp::Coalesce` arm.
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
        Ty::Fn(params.to_vec(), Box::new(ret), FnRow::unknown())
    }

    /// A stand-in creation-target id.
    fn def(n: u64) -> DefinitionId {
        DefinitionId::new(brink_format::DefinitionTag::Address, n)
    }

    /// A `Ty::Fn` carrying a concrete creation-target row (issue #1680).
    fn fn_ty_from(params: &[Ty], ret: Ty, targets: &[u64]) -> Ty {
        let row = targets.iter().fold(FnRow::empty(), |acc, &t| {
            acc.join(&FnRow::of_target(def(t)))
        });
        Ty::Fn(params.to_vec(), Box::new(ret), row)
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

    // ─── §5 effect rows on `Ty::Fn` (issue #1680 steps 2/3) ────────────

    #[test]
    fn fn_row_join_is_union_with_unknown_absorbing() {
        let a = FnRow::of_target(def(1));
        let b = FnRow::of_target(def(2));
        let ab = a.join(&b);
        assert_eq!(
            ab.targets().map(BTreeSet::len),
            Some(2),
            "two creation sites union"
        );
        assert!(ab.targets().is_some_and(|t| t.contains(&def(1))));
        assert!(ab.targets().is_some_and(|t| t.contains(&def(2))));
        // The top element absorbs in either position — one untraceable
        // source poisons the slot for good (§3, conservative-total).
        assert!(a.join(&FnRow::unknown()).is_unknown());
        assert!(FnRow::unknown().join(&a).is_unknown());
        assert!(FnRow::unknown().join(&FnRow::unknown()).is_unknown());
        // `empty` is the join identity, so a fold can start there.
        assert_eq!(FnRow::empty().join(&a), a);
    }

    #[test]
    fn fn_row_join_is_commutative_and_idempotent() {
        let a = FnRow::of_target(def(1));
        let b = FnRow::of_target(def(2));
        assert_eq!(a.join(&b), b.join(&a));
        assert_eq!(a.join(&a), a);
        assert_eq!(a.join(&b).join(&b), a.join(&b));
    }

    #[test]
    fn unify_joins_the_effect_row_alongside_params_and_return() {
        // §5: "a cell accumulates the join of every fn value assigned into
        // it" — two `#fn` literals reaching one slot leave both targets on
        // the slot's type.
        let joined = unify(
            &fn_ty_from(&[Ty::Int], Ty::Int, &[1]),
            &fn_ty_from(&[Ty::Int], Ty::Int, &[2]),
        );
        assert_eq!(joined, fn_ty_from(&[Ty::Int], Ty::Int, &[1, 2]));
        // A traced row joined with an unknown one stays unknown.
        assert_eq!(
            unify(
                &fn_ty_from(&[Ty::Int], Ty::Int, &[1]),
                &fn_ty(&[Ty::Int], Ty::Int)
            ),
            fn_ty(&[Ty::Int], Ty::Int)
        );
        // `Ty::Unknown` is still the identity — it carries no row at all,
        // so an unobserved slot must not poison a traced one.
        let traced = fn_ty_from(&[Ty::Int], Ty::Int, &[1]);
        assert_eq!(unify(&Ty::Unknown, &traced), traced);
        assert_eq!(unify(&traced, &Ty::Unknown), traced);
    }

    #[test]
    fn effect_rows_do_not_change_the_displayed_type() {
        // Rows are inferred provenance, not written syntax — every `E063`/
        // `E066` message that quotes a type back must be unaffected.
        assert_eq!(
            fn_ty_from(&[Ty::Int], Ty::Bool, &[1, 2]).display(),
            fn_ty(&[Ty::Int], Ty::Bool).display()
        );
    }

    #[test]
    fn erase_fn_rows_reaches_nested_positions() {
        let nested = Ty::Array(Box::new(Ty::Map(
            Box::new(Ty::String),
            Box::new(fn_ty_from(
                &[fn_ty_from(&[], Ty::Int, &[3])],
                Ty::Int,
                &[1, 2],
            )),
        )));
        let erased = Ty::Array(Box::new(Ty::Map(
            Box::new(Ty::String),
            Box::new(fn_ty(&[fn_ty(&[], Ty::Int)], Ty::Int)),
        )));
        assert_eq!(erase_fn_rows(&nested), erased);
        assert_eq!(erase_fn_rows(&erased), erased, "erasure is idempotent");
    }

    /// Whether a `Ty` variant nests another `Ty` — and so needs an erasing
    /// arm in [`erase_fn_rows`] — or is a nominal leaf that erasure must
    /// leave untouched.
    enum ErasureShape {
        NestsTy,
        Leaf,
    }

    /// Structural exhaustiveness guard (issue #1758), the same idiom as
    /// `brink-format`'s `assert_value_variants_exhaustive` (issue #883) and
    /// `brink-syntax-native`'s `coverage.rs` `classify`: an *exhaustive*
    /// match over every current [`Ty`] variant with **no wildcard arm**, so
    /// this — and therefore `cargo test` for this crate — fails to compile
    /// the moment a new variant lands on `Ty`, until it is explicitly
    /// classified here (and in `erase_fn_rows` itself, which mirrors this
    /// classification one-for-one). `Option`, `Weighted`, and `Tower` were
    /// each added to `Ty` over time carrying exactly this risk; a `_ =>`
    /// wildcard here would let the next one slip past unnoticed the way
    /// `erase_fn_rows`'s old wildcard would have.
    fn classify_erasure_shape(ty: &Ty) -> ErasureShape {
        match ty {
            Ty::Fn(..) | Ty::Array(_) | Ty::Map(_, _) | Ty::Option(_) | Ty::Weighted(_) => {
                ErasureShape::NestsTy
            }
            Ty::Int
            | Ty::Float
            | Ty::Bool
            | Ty::String
            | Ty::Divert
            | Ty::List(_)
            | Ty::Struct(_)
            | Ty::Handle(_)
            | Ty::Range { .. }
            | Ty::Tower(_)
            | Ty::Unknown
            | Ty::Conflicted => ErasureShape::Leaf,
        }
    }

    #[test]
    fn erase_fn_rows_leaves_every_nominal_leaf_untouched() {
        // Every variant `classify_erasure_shape` calls `Leaf`, exercised
        // through `erase_fn_rows` directly: proves the leaf arms really are
        // no-ops, not just that the guard match above compiles. If a
        // future `Ty` variant is added and mis-classified as `Leaf` when it
        // actually nests a `Ty`, this test's job is done by the guard
        // failing to compile before this even runs — this loop just pins
        // today's leaves so a leaf accidentally gaining mutation logic
        // later is also caught.
        let leaves = [
            Ty::Int,
            Ty::Float,
            Ty::Bool,
            Ty::String,
            Ty::Divert,
            Ty::List("Weathers".into()),
            Ty::Struct("Vec2".into()),
            Ty::Handle("AudioInstance".into()),
            Ty::Range { non_empty: true },
            Ty::Range { non_empty: false },
            Ty::Tower(TowerTy::Vec2),
            Ty::Unknown,
            Ty::Conflicted,
        ];
        for leaf in leaves {
            assert!(
                matches!(classify_erasure_shape(&leaf), ErasureShape::Leaf),
                "expected {leaf:?} to classify as a nominal leaf"
            );
            assert_eq!(erase_fn_rows(&leaf), leaf, "leaf erasure must be a no-op");
        }
    }

    #[test]
    fn assignable_ignores_effect_rows_but_not_the_rest_of_the_type() {
        // The step-2 guarantee: a fn value created at *any* target fits a
        // slot declared `fn(int): int`, whatever row the annotation carries
        // (an annotation's row is always the top element). A structural
        // `unify(param, arg) == param` test fails all three of these.
        let declared = fn_ty(&[Ty::Int], Ty::Int);
        assert!(assignable(
            &declared,
            &fn_ty_from(&[Ty::Int], Ty::Int, &[1])
        ));
        assert!(assignable(
            &fn_ty_from(&[Ty::Int], Ty::Int, &[1]),
            &fn_ty_from(&[Ty::Int], Ty::Int, &[2])
        ));
        assert!(assignable(
            &Ty::Array(Box::new(declared.clone())),
            &Ty::Array(Box::new(fn_ty_from(&[Ty::Int], Ty::Int, &[7])))
        ));
        // Everything the structural test used to reject is still rejected.
        assert!(!assignable(
            &declared,
            &fn_ty_from(&[Ty::String], Ty::Int, &[1])
        ));
        assert!(!assignable(&declared, &fn_ty_from(&[], Ty::Int, &[1])));
        assert!(!assignable(&declared, &Ty::Int));
        assert!(!assignable(&Ty::Int, &Ty::Float));
        // …and the one legal directional numeric coercion still passes.
        assert!(assignable(&Ty::Float, &Ty::Int));
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

    // ─── NS-A5 `Ty::Range` + the NonEmptyRange refinement (F7/F8) ──────

    fn range(non_empty: bool) -> Ty {
        Ty::Range { non_empty }
    }

    #[test]
    fn range_display_names_the_refinement() {
        assert_eq!(range(false).display(), "range");
        assert_eq!(range(true).display(), "NonEmptyRange");
        assert_eq!(opt(range(true)).display(), "Option[NonEmptyRange]");
    }

    #[test]
    fn range_evidence_joins_with_and() {
        // Evidence survives only if EVERY observed source carries it — the
        // sound direction (a join can lose evidence, never fabricate it).
        assert_eq!(unify(&range(true), &range(true)), range(true));
        assert_eq!(unify(&range(true), &range(false)), range(false));
        assert_eq!(unify(&range(false), &range(true)), range(false));
        assert_eq!(unify(&range(false), &range(false)), range(false));
        // Unknown is the identity, refinement bit included.
        assert_eq!(unify(&Ty::Unknown, &range(true)), range(true));
    }

    #[test]
    fn range_vs_other_concrete_is_conflicted() {
        // A range never coerces — not to int, not to array<int>, and the
        // refinement is a view over Range, never a separate kind.
        assert_eq!(unify(&range(false), &Ty::Int), Ty::Conflicted);
        assert_eq!(
            unify(&range(true), &Ty::Array(Box::new(Ty::Int))),
            Ty::Conflicted
        );
        assert_eq!(unify(&opt(range(true)), &range(true)), Ty::Conflicted);
    }

    #[test]
    fn range_evidence_join_is_order_independent() {
        let orderings: [&[Ty]; 3] = [
            &[
                Ty::Range { non_empty: true },
                Ty::Range { non_empty: false },
            ],
            &[
                Ty::Range { non_empty: false },
                Ty::Range { non_empty: true },
            ],
            &[
                Ty::Unknown,
                Ty::Range { non_empty: true },
                Ty::Range { non_empty: false },
            ],
        ];
        for ordering in orderings {
            assert_eq!(
                unify_all(ordering.iter().cloned()),
                Ty::Range { non_empty: false },
                "order {ordering:?} must lose the evidence at the join"
            );
        }
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
