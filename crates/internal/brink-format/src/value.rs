use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::id::{DefinitionId, NameId};

/// Maximum nesting depth permitted when decoding `VAL_ARRAY`/`VAL_MAP`
/// values. Generous for legitimate data but bounds worst-case recursion so a
/// crafted file of nested single-element arrays (~5 bytes/level) cannot
/// stack-overflow the reader (CLAUDE.md "guard against unbounded growth";
/// issue #553).
///
/// This is the single canonical definition, shared by every `decode_value`
/// implementation that recurses on collection values — the `.inkb` reader
/// (`brink_format::inkb::read`) and the runtime transcript reader
/// (`brink_runtime::transcript`) both reference this constant rather than
/// each defining their own copy (issue #561).
pub const MAX_DECODE_DEPTH: usize = 128;

/// Identifies a struct shape (TM-4, `docs/typed-mode-spec.md` §6) within the
/// compiled story's `StructShapes` section (`docs/format-spec.md`, section
/// tag `0x0C`).
///
/// Distinct from [`crate::id::DefinitionId`]: a `STRUCT` declaration is a
/// compile-time nominal type, not a runtime storage location, so its wire
/// footprint is this flat `u32` index into `StructShapes`, not a tagged
/// definition id. Assigned deterministically at codegen time (sorted by
/// declared struct name, never `HashMap` iteration order — CLAUDE.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ShapeId(pub u32);

/// The runtime type of a [`Value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    String,
    List,
    DivertTarget,
    VariablePointer,
    TempPointer,
    Null,
    FragmentRef,
    /// A copy-on-write ordered collection ([`Value::Array`]).
    Array,
    /// A copy-on-write insertion-ordered map ([`Value::Map`]).
    Map,
    /// A closed-shape, copy-on-write record ([`Value::Record`]) — TM-4.
    Record,
    /// A zero-bound function value ([`Value::FnRef`]) — T1c.
    FnRef,
    /// A function value with a bound-arg prefix ([`Value::Closure`]) — T1c.
    Closure,
    /// An opaque host-resource token ([`Value::Handle`]) — T1d.
    Handle,
    /// A symbolic path projection ([`Value::Projection`]) — T1e.
    Projection,
    /// A typed-absence value ([`Value::OptionVal`]) — NS-A1, `Option[T]`.
    Option,
    /// An integer range value ([`Value::Range`]) — NS-A5, F7.
    Range,
    /// A 2-lane f32 vector ([`Value::Vec2`]) — NS-A8, the numeric tower.
    Vec2,
    /// A 3-lane f32 vector ([`Value::Vec3`]) — NS-A8.
    Vec3,
    /// A 4-lane f32 vector ([`Value::Vec4`]) — NS-A8.
    Vec4,
    /// A rotation quaternion, `(x, y, z, w)` ([`Value::Quat`]) — NS-A8.
    Quat,
    /// A column-major 2×2 f32 matrix ([`Value::Mat2`]) — NS-A8.
    Mat2,
    /// A column-major 3×3 f32 matrix ([`Value::Mat3`]) — NS-A8.
    Mat3,
    /// A column-major 4×4 f32 matrix ([`Value::Mat4`]) — NS-A8.
    Mat4,
    /// A weighted table ([`Value::Weighted`]) — NS-A7, `Weighted[T]`.
    Weighted,
}

/// A runtime value in the ink VM.
///
/// Heap-allocating variants (`String`, `List`, `Array`, `Map`, `Record`,
/// `Closure`, `Projection`, `OptionVal`, `Weighted`) are wrapped in `Arc` so
/// that cloning a `Value` is always O(1) — a refcount bump, not a deep copy —
/// which makes call-frame cloning (during `fork_thread`) essentially free.
/// Atomic refcounts are used so `Value` can flow through Bevy's parallel
/// scheduler.
///
/// This `Arc` wrapping is a *performance* mechanism under **value
/// semantics**, not reference semantics: sharing the underlying allocation
/// is unobservable (`docs/value-model-spec.md` §3), because every mutating
/// entry point forks the shared allocation on write (take → `make_mut` →
/// write-back — see `docs/runtime-spec.md`'s "Value model" section). A
/// binding that never observed a mutation never sees it, regardless of
/// whether it shares the `Arc` underneath.
///
/// The `Array`/`Map` collections follow the ratified value model
/// (`docs/value-model-spec.md` §4/§5): value semantics with copy-on-write
/// sharing. Sharing is unobservable (§3) — [equality](Value#impl-PartialEq-for-Value)
/// is structural with an `Arc::ptr_eq` fast path, and mutation goes through
/// the take → [`make_mut`](Value::array_make_mut) → write-back RMW discipline
/// so an unshared collection mutates in place and a shared one performs a
/// single copy.
///
/// `PartialEq` is implemented by hand rather than derived so the collection
/// arms can take the `ptr_eq` shortcut; every scalar arm matches what the
/// derive would have produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(Arc<str>),
    List(Arc<ListValue>),
    DivertTarget(DefinitionId),
    /// A reference to a global variable, used for `ref` parameters.
    VariablePointer(DefinitionId),
    /// A runtime-only pointer to a temp in a specific call frame.
    /// Used for `ref` parameters that target temp variables.
    TempPointer {
        slot: u16,
        frame_depth: u16,
    },
    Null,
    /// A reference to a fragment in the output buffer's fragment store.
    /// Fragments preserve structural output parts for locale re-rendering.
    FragmentRef(u32),
    /// An ordered, copy-on-write collection of values (value-model-spec §4).
    ///
    /// The backing `Vec` is shared behind an `Arc`; clone is a refcount bump.
    /// Mutation goes through [`array_make_mut`](Self::array_make_mut), which
    /// copies the backing vector only when the `Arc` is shared.
    Array(Arc<Vec<Value>>),
    /// An insertion-ordered, copy-on-write map with scalar keys
    /// (value-model-spec §4). Keys are `int`/`string`/`bool` ([`MapKey`]);
    /// iteration order is insertion order, so the value is deterministic
    /// without sorting or hashing.
    Map(Arc<OrderedMap>),
    /// A closed-shape record (typed-dialect era, TM-4;
    /// `docs/value-model-spec.md` §11c / `docs/typed-mode-spec.md` §6).
    ///
    /// `fields` is a flat, shape-ordered vector shared behind an `Arc` — the
    /// exact same COW discipline as [`Array`](Self::Array)/[`Map`](Self::Map):
    /// clone is a refcount bump, mutation goes through
    /// [`record_make_mut`](Self::record_make_mut). The shape is closed (no
    /// dynamic add/remove of fields) and identified by [`ShapeId`], an
    /// interned index into the compiled story's `StructShapes` table — two
    /// records are never equal unless their shapes match, even if their
    /// field vectors happen to coincide.
    Record {
        shape: ShapeId,
        fields: Arc<Vec<Value>>,
    },
    /// A **zero-bound function value** (`docs/t1c-spec.md` §1/§6, wire tag
    /// `VAL_FN_REF`). Partial application over a statically-named function
    /// with no bound-arg prefix — `#fn(name)` where the target has no `ref`
    /// params. The `DefinitionId` is the fn token (hashes from the target's
    /// name, so a saved token survives recompiles that only edit the body).
    ///
    /// A no-payload scalar (`Copy`-cheap `DefinitionId`); no `Arc` needed.
    /// Structural equality is "same fn token" (§5, sharing-unobservable).
    FnRef(DefinitionId),
    /// A **function value with a bound-arg prefix** (`docs/t1c-spec.md`
    /// §1/§6, wire tag `VAL_CLOSURE`). Despite the name there is no lexical
    /// environment — ink has no free variables; the "env" is the bound
    /// prefix of the target's declared params (a `val` entry snapshots the
    /// value at creation, a `ref` entry captures a durable cell as a
    /// [`VariablePointer`](Self::VariablePointer)).
    ///
    /// Wrapped in `Arc` for the same O(1)-clone discipline as the collection
    /// variants. Structural equality (§5): same fn token **and** equal env
    /// rows — `ref` entries compare by bound cell, `val` entries by value,
    /// both of which fall out of the derived per-entry comparison because a
    /// `ref` payload is a `VariablePointer` (compared by its `DefinitionId`).
    Closure(Arc<ClosureValue>),
    /// An opaque host-resource token (`docs/t1d-spec.md` §1/§2, wire tag
    /// `VAL_HANDLE`). Host resources (entities, audio instances, assets,
    /// timers) enter the script world as `Handle` tokens: `{kind, id}`
    /// scalars with value semantics — copied like ints, serializable,
    /// compared by token. No live pointer ever lives in a `Value`;
    /// dereferencing happens only host-side, against the host's registry,
    /// inside bindings (the sharing-unobservable dogma applies verbatim — a
    /// handle is a *name*, re-bound at a defined seam).
    ///
    /// `kind` is a [`NameId`] into the compiled story's manifest-declared
    /// kind vocabulary (analyzer-side, not the format — the format ships
    /// only the token shape, `docs/format-v4-rfc.md` §2). `id` is an opaque
    /// `u64` the host allocates; the script never interprets it.
    ///
    /// A no-payload scalar (`Copy`-cheap: `NameId` + `u64`); no `Arc`
    /// needed. Structural equality is token equality (`kind == kind && id
    /// == id`, §6) — no ordering exists (any `<`/`>`/`<=`/`>=` is a runtime
    /// fault in gradual mode, a compile error under the typed dialect), and
    /// a handle is never a legal map key (the domain stays
    /// int/string/bool — same restriction as function values).
    ///
    /// **No literal syntax and no dedicated opcode construct this value —
    /// handles enter the script world only via bindings** (`docs/t1d-spec.md`
    /// §2 RULED). This variant, its wire encoding, and its `.inkt` atom
    /// exist so a handle received from a binding can flow through globals,
    /// collections, saves, and the transcript like any other value.
    Handle {
        /// The manifest-declared kind name (analyzer-side vocabulary).
        kind: NameId,
        /// The host-allocated token id. Opaque to the script.
        id: u64,
    },
    /// A symbolic path projection (`docs/t1e-spec.md` §1/§3, wire tag
    /// `VAL_PROJECTION`): `(root cell, path segments)` — never an interior
    /// pointer. Created only in `ref`-argument position (`ref
    /// npc.inventory[3]`); reads walk the path against the root's *current*
    /// value, writes desugar to root-cell RMW (take → walk → `make_mut`
    /// spine → write → store back, spec §3). The segment list is fixed at
    /// creation ("index expressions snapshot at `ref` creation", spec §1) —
    /// there is no lazy re-evaluation.
    ///
    /// Wrapped in `Arc` for the same O(1)-clone discipline as
    /// [`Closure`](Self::Closure). Structural equality (spec §4 PROPOSED,
    /// implemented here): same root cell and equal segments.
    Projection(Arc<ProjectionValue>),
    /// A typed-absence value — the compiler-owned `Option[T]` enum (NS-A1,
    /// `docs/stdlib-spec.md` §1.1/§1.4, ruled 2026-07-18: "a fault says
    /// 'your program is wrong'; Option says 'the world didn't have one'").
    ///
    /// `Option[T]` is the third compiler-known parameterized builtin
    /// (joining `[T]`/`[K: V]`) — a compiler-owned enum shape, NOT user
    /// generics. Runtime representation: `None` is payload-free (no
    /// allocation); `Some` wraps its inner value behind an `Arc` so clone
    /// stays O(1) like every other heap-bearing variant. Nesting is legal
    /// and meaningful (`some(none) != none` — the wire and equality both
    /// preserve it).
    ///
    /// Named `OptionVal` (not `Option`) purely to avoid the eternal
    /// `core::option::Option` shadowing hazard inside `match` arms; the
    /// [`ValueType`] discriminant and every author-facing surface still
    /// say "option".
    ///
    /// Structural equality: `none == none`; `some(x) == some(y)` iff
    /// `x == y`; an Option is never equal to a bare `T` (the ruled
    /// `Option[T] ≠ T` strictness holds at the value layer too). Display
    /// (`stringify`/`string(x)`): `none` / `some(<inner>)` — the boring,
    /// stable form, total forever (F28). The §1.6b display-boundary
    /// forgiveness (Track B4) is a `brink-runtime`-only concern layered on
    /// top at read time (`value_ops::stringify_display`) — deliberately
    /// not implemented at this value-definition layer, since `string()`
    /// and every non-display consumer must keep seeing the total form.
    OptionVal(Option<Arc<Value>>),
    /// An integer range value (NS-A5, `docs/stdlib-spec.md` §7 — F7, ruled
    /// 2026-07-19: "ranges are a REAL Value kind"). `start..end` (exclusive)
    /// or `start..=end` (inclusive) over `int` bounds — v1 is int-only.
    ///
    /// Ranges join the closed iterable set (`for i in 0..n`), index like a
    /// virtual array of their elements, and are the substrate of the
    /// language's first value refinement (the inhabited range consumed by
    /// `rand::int`). A durable wire form exists (`VAL_RANGE`) because
    /// `FlowFrame` spills for-loop iterators across `await` — a range held
    /// in a loop snapshot must survive save/load.
    ///
    /// A no-payload scalar (two `i32`s + a flag); no `Arc` needed. The
    /// **written form is preserved** — `1..7` and `1..=6` keep their
    /// `inclusive` flag through saves, the transcript, and display — but
    /// **equality is content equality** (F7's ruling word): two ranges are
    /// equal iff they denote the same integer sequence, so `1..=6 == 1..7`
    /// and every empty range equals every other empty range. This is the
    /// same content-over-form posture as the #909 map-equality ruling
    /// (insertion order iterates, content compares).
    Range {
        /// The first element of the range (always inclusive).
        start: i32,
        /// The written end bound; whether it is an element depends on
        /// `inclusive`.
        end: i32,
        /// `true` for the `..=` form (`end` is the last element), `false`
        /// for the `..` form (`end` is one past the last element).
        inclusive: bool,
    },
    /// A 2-lane f32 vector (NS-A8, `docs/tower-mini-spec.md` T1: the tower
    /// value kinds are **glam-backed** — glam is the in-memory compute type,
    /// so vector/quaternion/matrix ops arrive correct-by-construction).
    ///
    /// Serde discipline (T5): the derive on `Value` routes every tower
    /// variant through the hand-written lane modules in [`tower_serde`] —
    /// explicit `x, y(, z, w)` lane order for vectors and the quat,
    /// column-major column-by-column for matrices — NEVER glam's memory
    /// representation (which varies with SIMD features and versions) and
    /// never glam's own `serde` feature (kept off in `Cargo.toml`).
    ///
    /// Equality (T4): componentwise IEEE via glam's derived `PartialEq` — a
    /// NaN-bearing vector never equals itself, `-0.0 == +0.0` per lane,
    /// exactly like bare `Float`. Tower values are NOT orderable (§4b: a
    /// vector in an ordering context is a `NotOrderable` fault) and are
    /// never legal map keys (`MapKey::from_value` has no tower arms).
    Vec2(#[serde(with = "tower_serde::vec2")] glam::Vec2),
    /// A 3-lane f32 vector (NS-A8). The **unaligned** `glam::Vec3` (not
    /// `Vec3A`) per the mini-spec — aligned variants would bloat every
    /// `Value`. See [`Vec2`](Self::Vec2) for the shared tower discipline.
    Vec3(#[serde(with = "tower_serde::vec3")] glam::Vec3),
    /// A 4-lane f32 vector (NS-A8). See [`Vec2`](Self::Vec2).
    Vec4(#[serde(with = "tower_serde::vec4")] glam::Vec4),
    /// A rotation quaternion (NS-A8), lane order `(x, y, z, w)` per glam
    /// (T3: conventions per glam, wholesale — right-handed, `quat * quat`
    /// composes, `quat * vec` rotates). See [`Vec2`](Self::Vec2).
    Quat(#[serde(with = "tower_serde::quat")] glam::Quat),
    /// A column-major 2×2 f32 matrix (NS-A8, T2: all matrix sizes ship).
    /// See [`Vec2`](Self::Vec2).
    Mat2(#[serde(with = "tower_serde::mat2")] glam::Mat2),
    /// A column-major 3×3 f32 matrix (NS-A8). The **unaligned** `glam::Mat3`
    /// (not `Mat3A`). See [`Vec2`](Self::Vec2).
    Mat3(#[serde(with = "tower_serde::mat3")] glam::Mat3),
    /// A column-major 4×4 f32 matrix (NS-A8). See [`Vec2`](Self::Vec2).
    Mat4(#[serde(with = "tower_serde::mat4")] glam::Mat4),
    /// A weighted table (NS-A7, `docs/stdlib-spec.md` §8): `Weighted[T]` —
    /// positive-int weights over values, in construction order.
    /// **Evidence-by-construction**: the only producer (`weighted_new`)
    /// refuses empty tables and non-positive/non-int weights, so a
    /// `Weighted` that exists is always a valid `roll` target (total). The
    /// entry row is a **multiset** (F17: duplicate weights legal and
    /// meaningful — deliberately divergent from `Map`'s key-set). v1 is
    /// construct-and-roll: no `len`, no iteration, no mutation.
    Weighted(Arc<WeightedValue>),
}

/// Hand-written serde lane codecs for the tower variants (NS-A8,
/// `docs/tower-mini-spec.md` T5): each type serializes as its flat lane
/// array — vectors and the quat as `[x, y(, z, w)]`, matrices as their
/// column-major `to_cols_array()` — and deserializes back through glam's
/// explicit `from_array`/`from_cols_array` constructors. Glam computes; the
/// serialized form is ours: no glam memory layout, no serde-through-glam.
pub mod tower_serde {
    /// Expand one lane codec module: `to`/`from` are the explicit
    /// lane-array conversions (never a memory-layout cast). Matrix `from`
    /// constructors (`from_cols_array`) take the array by reference, hence
    /// the closure rather than a bare path.
    macro_rules! lane_codec {
        ($name:ident, $ty:ty, $lanes:literal, $to:ident, |$a:ident| $from:expr) => {
            pub mod $name {
                use serde::{Deserialize, Deserializer, Serialize, Serializer};

                pub fn serialize<S: Serializer>(v: &$ty, s: S) -> Result<S::Ok, S::Error> {
                    v.$to().serialize(s)
                }

                pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<$ty, D::Error> {
                    <[f32; $lanes]>::deserialize(d).map(|$a| $from)
                }
            }
        };
    }

    lane_codec!(vec2, glam::Vec2, 2, to_array, |a| glam::Vec2::from_array(a));
    lane_codec!(vec3, glam::Vec3, 3, to_array, |a| glam::Vec3::from_array(a));
    lane_codec!(vec4, glam::Vec4, 4, to_array, |a| glam::Vec4::from_array(a));
    lane_codec!(quat, glam::Quat, 4, to_array, |a| glam::Quat::from_array(a));
    lane_codec!(mat2, glam::Mat2, 4, to_cols_array, |a| {
        glam::Mat2::from_cols_array(&a)
    });
    lane_codec!(mat3, glam::Mat3, 9, to_cols_array, |a| {
        glam::Mat3::from_cols_array(&a)
    });
    lane_codec!(mat4, glam::Mat4, 16, to_cols_array, |a| {
        glam::Mat4::from_cols_array(&a)
    });
}

/// The payload of a [`Value::Projection`] — the root cell plus its ordered
/// segment chain (`docs/t1e-spec.md` §1/§3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionValue {
    /// The durable root cell this projection reads/writes through (a global
    /// `VAR`/`#@local` — never a temp, enforced at compile time by T1e-1's
    /// E080 durable-root check). Mirrors the `VAL_VAR_POINTER` payload shape
    /// (`docs/format-v4-rfc.md` §1: "cell reference … reused not
    /// reinvented").
    pub cell: DefinitionId,
    /// The ordered path segments, fixed at creation.
    pub segments: Vec<ProjSegment>,
}

/// One path-projection segment (`docs/format-v4-rfc.md` §1: `segments: 0 =
/// index i32, 1 = key value`). Segment kind `2 = range` is RESERVED and never
/// constructed in T1e (icebox #829 — sequence slices/ranges).
///
/// The kind recorded here is a **wire-compactness choice**, not a semantic
/// tag the walker trusts blindly: an evaluated segment value that happens to
/// be an `Int` is captured as [`Index`](Self::Index) (the compact i32 form);
/// everything else — a map key of another scalar type, or a struct field
/// name (always a `Value::String` literal) — is captured as
/// [`Key`](Self::Key). Walking dispatches on the *root's current container
/// type* at each step (spec §4: reads walk against the root's current
/// value), so an `Index(n)` segment applied to a `Map` is reinterpreted as
/// `MapKey::Int(n)` — the distinction never forecloses either domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProjSegment {
    /// `[i]` — captured because the evaluated segment value was an `Int`.
    Index(i32),
    /// `[k]` (a non-`Int` map key) or `.field` (a struct field name,
    /// captured as `Value::String`).
    Key(Value),
}

impl ProjSegment {
    /// Build a segment from an evaluated `Value` — the classification rule
    /// this whole module documents: `Int` → [`Index`](Self::Index), anything
    /// else → [`Key`](Self::Key).
    #[must_use]
    pub fn from_value(v: Value) -> Self {
        match v {
            Value::Int(n) => Self::Index(n),
            other => Self::Key(other),
        }
    }
}

/// The payload of a [`Value::Closure`] — the fn token plus its bound-arg
/// prefix (`docs/t1c-spec.md` §1/§6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosureValue {
    /// The target function's [`DefinitionId`] (the fn token).
    pub target: DefinitionId,
    /// The bound-arg prefix, in the target's declared param order. The
    /// entries carry the param **name and mode** deliberately (spec §6): on
    /// load/invoke after a recompile they are validated against the current
    /// signature so a renamed/re-moded param faults cleanly instead of
    /// silently misbinding.
    pub env: Vec<ClosureEnvEntry>,
}

/// One bound-arg entry in a [`ClosureValue`]'s env.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosureEnvEntry {
    /// The bound param's interned name (redundancy for rehydration checking).
    pub name: NameId,
    /// `true` if the target declared this param `ref` (the payload is a
    /// captured cell), `false` for a `val` snapshot.
    pub is_ref: bool,
    /// The bound value: a snapshot (`val`) or a captured cell reference
    /// (`ref` — a [`VariablePointer`](Value::VariablePointer)).
    pub payload: Value,
}

impl Value {
    /// Return the type discriminant for this value.
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Int(_) => ValueType::Int,
            Self::Float(_) => ValueType::Float,
            Self::Bool(_) => ValueType::Bool,
            Self::String(_) => ValueType::String,
            Self::List(_) => ValueType::List,
            Self::DivertTarget(_) => ValueType::DivertTarget,
            Self::VariablePointer(_) => ValueType::VariablePointer,
            Self::TempPointer { .. } => ValueType::TempPointer,
            Self::Null => ValueType::Null,
            Self::FragmentRef(_) => ValueType::FragmentRef,
            Self::Array(_) => ValueType::Array,
            Self::Map(_) => ValueType::Map,
            Self::Record { .. } => ValueType::Record,
            Self::FnRef(_) => ValueType::FnRef,
            Self::Closure(_) => ValueType::Closure,
            Self::Handle { .. } => ValueType::Handle,
            Self::Projection(_) => ValueType::Projection,
            Self::OptionVal(_) => ValueType::Option,
            Self::Range { .. } => ValueType::Range,
            Self::Vec2(_) => ValueType::Vec2,
            Self::Vec3(_) => ValueType::Vec3,
            Self::Vec4(_) => ValueType::Vec4,
            Self::Quat(_) => ValueType::Quat,
            Self::Mat2(_) => ValueType::Mat2,
            Self::Mat3(_) => ValueType::Mat3,
            Self::Mat4(_) => ValueType::Mat4,
            Self::Weighted(_) => ValueType::Weighted,
        }
    }

    /// Build a [`Range`](Self::Range) from its written bounds.
    pub fn range(start: i32, end: i32, inclusive: bool) -> Self {
        Self::Range {
            start,
            end,
            inclusive,
        }
    }

    /// Borrow the `(start, end, inclusive)` triple if this value is a
    /// [`Range`](Self::Range).
    pub fn as_range(&self) -> Option<(i32, i32, bool)> {
        match self {
            Self::Range {
                start,
                end,
                inclusive,
            } => Some((*start, *end, *inclusive)),
            _ => None,
        }
    }

    /// The one-past-the-last element bound of a [`Range`](Self::Range),
    /// normalized over the written form (`i64` so `1..=i32::MAX` cannot
    /// overflow). `None` for any non-range value.
    pub fn range_end_exclusive(&self) -> Option<i64> {
        match self {
            Self::Range { end, inclusive, .. } => {
                Some(i64::from(*end) + i64::from(u8::from(*inclusive)))
            }
            _ => None,
        }
    }

    /// Number of elements a [`Range`](Self::Range) denotes (`0` for an empty
    /// range — a range never has negative length). `None` for any non-range
    /// value. `i64` because `i32::MIN..=i32::MAX` has 2³² elements.
    pub fn range_len(&self) -> Option<i64> {
        match self {
            Self::Range { start, .. } => {
                let end_ex = self.range_end_exclusive()?;
                Some((end_ex - i64::from(*start)).max(0))
            }
            _ => None,
        }
    }

    /// Build a [`Weighted`](Self::Weighted) table from `(weight, value)`
    /// entries. The caller owns the §8 evidence-by-construction invariant
    /// (non-empty, positive weights) — the VM's `weighted_new` op and the
    /// wire reader both validate before calling this.
    #[must_use]
    pub fn weighted(entries: Vec<(i32, Value)>) -> Self {
        Self::Weighted(Arc::new(WeightedValue { entries }))
    }

    /// Build a `some(inner)` [`OptionVal`](Self::OptionVal).
    pub fn some(inner: Value) -> Self {
        Self::OptionVal(Some(Arc::new(inner)))
    }

    /// Build a `none` [`OptionVal`](Self::OptionVal).
    pub fn none() -> Self {
        Self::OptionVal(None)
    }

    /// Borrow the Option payload if this value is an
    /// [`OptionVal`](Self::OptionVal): `Some(Some(&inner))` for `some(x)`,
    /// `Some(None)` for `none`, `None` for any non-Option value.
    pub fn as_option(&self) -> Option<Option<&Value>> {
        match self {
            Self::OptionVal(inner) => Some(inner.as_deref()),
            _ => None,
        }
    }

    /// Extract an `i32` if this value is an [`Int`](Self::Int).
    ///
    /// Strict: does not coerce floats or booleans. Returns `None` for any
    /// other variant. For binding authors that want to read an integer
    /// argument from ink.
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Extract an `f32` if this value is numeric.
    ///
    /// Lenient on the int → float direction only: an [`Int`](Self::Int) is
    /// widened to `f32` (matching ink's implicit int→float promotion), but a
    /// float is never truncated to an int by [`as_int`](Self::as_int).
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(f) => Some(*f),
            #[expect(
                clippy::cast_precision_loss,
                reason = "int->float promotion matches ink coercion semantics"
            )]
            Self::Int(i) => Some(*i as f32),
            _ => None,
        }
    }

    /// Extract a `bool` if this value is a [`Bool`](Self::Bool).
    ///
    /// Strict: does not treat nonzero numbers as truthy. Use the VM's own
    /// truthiness rules if you need ink-style coercion.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Borrow the string contents if this value is a [`String`](Self::String).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Build an [`Array`](Self::Array) from a vector of values.
    pub fn array(items: Vec<Value>) -> Self {
        Self::Array(Arc::new(items))
    }

    /// Build a [`Map`](Self::Map) from an [`OrderedMap`].
    pub fn map(map: OrderedMap) -> Self {
        Self::Map(Arc::new(map))
    }

    /// Build a [`Record`](Self::Record) from a shape id and its field values
    /// (already in the shape's declared field order — the caller, not this
    /// constructor, is responsible for ordering).
    pub fn record(shape: ShapeId, fields: Vec<Value>) -> Self {
        Self::Record {
            shape,
            fields: Arc::new(fields),
        }
    }

    /// Borrow the record payload if this value is a [`Record`](Self::Record).
    pub fn as_record(&self) -> Option<(ShapeId, &Arc<Vec<Value>>)> {
        match self {
            Self::Record { shape, fields } => Some((*shape, fields)),
            _ => None,
        }
    }

    /// Build a [`Closure`](Self::Closure) from a target and its bound-arg
    /// prefix (T1c, `docs/t1c-spec.md` §6).
    pub fn closure(target: DefinitionId, env: Vec<ClosureEnvEntry>) -> Self {
        Self::Closure(Arc::new(ClosureValue { target, env }))
    }

    /// The fn token (target [`DefinitionId`]) if this value is a function
    /// value — [`FnRef`](Self::FnRef) or [`Closure`](Self::Closure).
    pub fn fn_target(&self) -> Option<DefinitionId> {
        match self {
            Self::FnRef(target) => Some(*target),
            Self::Closure(c) => Some(c.target),
            _ => None,
        }
    }

    /// Borrow the closure payload if this value is a [`Closure`](Self::Closure).
    pub fn as_closure(&self) -> Option<&Arc<ClosureValue>> {
        match self {
            Self::Closure(c) => Some(c),
            _ => None,
        }
    }

    /// Build a [`Handle`](Self::Handle) token from a kind and host-allocated
    /// id (T1d, `docs/t1d-spec.md` §2). Note: this constructor is a plain
    /// value builder, not a capability mint — the invariant that handles
    /// only ever originate from a binding is enforced by the compiler (no
    /// literal syntax, no opcode constructs this value), not by this type.
    pub fn handle(kind: NameId, id: u64) -> Self {
        Self::Handle { kind, id }
    }

    /// Borrow the `(kind, id)` pair if this value is a [`Handle`](Self::Handle).
    pub fn as_handle(&self) -> Option<(NameId, u64)> {
        match self {
            Self::Handle { kind, id } => Some((*kind, *id)),
            _ => None,
        }
    }

    /// Build a [`Projection`](Self::Projection) from a root cell and its
    /// ordered segment chain (`docs/t1e-spec.md` §1/§3).
    pub fn projection(cell: DefinitionId, segments: Vec<ProjSegment>) -> Self {
        Self::Projection(Arc::new(ProjectionValue { cell, segments }))
    }

    /// Borrow the projection payload if this value is a
    /// [`Projection`](Self::Projection).
    pub fn as_projection(&self) -> Option<&Arc<ProjectionValue>> {
        match self {
            Self::Projection(p) => Some(p),
            _ => None,
        }
    }

    /// Extract the glam payload if this value is a [`Vec2`](Self::Vec2) —
    /// the NS-A8 identity-marshal read for binding authors (T1: glam is the
    /// compute type on both sides of the boundary). Strict like
    /// [`as_int`](Self::as_int): no cross-kind coercion.
    pub fn as_vec2(&self) -> Option<glam::Vec2> {
        match self {
            Self::Vec2(v) => Some(*v),
            _ => None,
        }
    }

    /// Extract the glam payload if this value is a [`Vec3`](Self::Vec3).
    pub fn as_vec3(&self) -> Option<glam::Vec3> {
        match self {
            Self::Vec3(v) => Some(*v),
            _ => None,
        }
    }

    /// Extract the glam payload if this value is a [`Vec4`](Self::Vec4).
    pub fn as_vec4(&self) -> Option<glam::Vec4> {
        match self {
            Self::Vec4(v) => Some(*v),
            _ => None,
        }
    }

    /// Extract the glam payload if this value is a [`Quat`](Self::Quat).
    pub fn as_quat(&self) -> Option<glam::Quat> {
        match self {
            Self::Quat(q) => Some(*q),
            _ => None,
        }
    }

    /// Extract the glam payload if this value is a [`Mat2`](Self::Mat2).
    pub fn as_mat2(&self) -> Option<glam::Mat2> {
        match self {
            Self::Mat2(m) => Some(*m),
            _ => None,
        }
    }

    /// Extract the glam payload if this value is a [`Mat3`](Self::Mat3).
    pub fn as_mat3(&self) -> Option<glam::Mat3> {
        match self {
            Self::Mat3(m) => Some(*m),
            _ => None,
        }
    }

    /// Extract the glam payload if this value is a [`Mat4`](Self::Mat4).
    pub fn as_mat4(&self) -> Option<glam::Mat4> {
        match self {
            Self::Mat4(m) => Some(*m),
            _ => None,
        }
    }

    /// Borrow the array payload if this value is an [`Array`](Self::Array).
    ///
    /// Read-only: the returned slice never triggers a copy. Mutation uses
    /// [`array_make_mut`](Self::array_make_mut).
    pub fn as_array(&self) -> Option<&Arc<Vec<Value>>> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Borrow the map payload if this value is a [`Map`](Self::Map).
    ///
    /// Read-only: mutation uses [`map_make_mut`](Self::map_make_mut).
    pub fn as_map(&self) -> Option<&Arc<OrderedMap>> {
        match self {
            Self::Map(map) => Some(map),
            _ => None,
        }
    }

    /// Copy-on-write mutable access to an [`Array`](Self::Array)'s backing
    /// vector, or `None` for any other value.
    ///
    /// This is the `make_mut` step of the take → `make_mut` → write-back RMW
    /// discipline (value-model-spec §5). When the backing `Arc` is unique the
    /// mutation is in place (O(1) amortized append); when it is shared with a
    /// snapshot or another slot, exactly one O(n) copy is made and the value
    /// becomes unique again. Because sharing is unobservable (§3), callers
    /// cannot tell which path was taken.
    pub fn array_make_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::Array(items) => Some(Arc::make_mut(items)),
            _ => None,
        }
    }

    /// Copy-on-write mutable access to a [`Map`](Self::Map)'s contents, or
    /// `None` for any other value. See [`array_make_mut`](Self::array_make_mut)
    /// for the RMW discipline this implements.
    pub fn map_make_mut(&mut self) -> Option<&mut OrderedMap> {
        match self {
            Self::Map(map) => Some(Arc::make_mut(map)),
            _ => None,
        }
    }

    /// Copy-on-write mutable access to a [`Record`](Self::Record)'s flat
    /// field vector, or `None` for any other value. See
    /// [`array_make_mut`](Self::array_make_mut) for the RMW discipline this
    /// implements — the shape itself never changes (closed shape), only
    /// field values.
    pub fn record_make_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::Record { fields, .. } => Some(Arc::make_mut(fields)),
            _ => None,
        }
    }
}

/// Structural equality with an `Arc::ptr_eq` fast path for collections
/// (value-model-spec §4/§5).
///
/// Every scalar arm reproduces exactly what `#[derive(PartialEq)]` would emit.
/// The `Array`/`Map` arms add the `ptr_eq` shortcut: two values that share the
/// same `Arc` (the same snapshot) compare equal immediately, otherwise the
/// comparison is element-wise structural. NaN-bearing collections that are
/// *not* the same snapshot never compare equal, because `f32` equality
/// composes structurally through the elements; a collection compared against
/// *itself* (same `Arc`) is equal even if it contains a NaN — the spec calls
/// this out as harmless and stated (§4).
impl PartialEq for Value {
    #[expect(
        clippy::match_same_arms,
        reason = "each scalar variant is spelled out so the mapping to the \
                  derive it replaces is auditable; merging identical `a == b` \
                  bodies would obscure which variants are covered"
    )]
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::DivertTarget(a), Self::DivertTarget(b)) => a == b,
            (Self::VariablePointer(a), Self::VariablePointer(b)) => a == b,
            (
                Self::TempPointer {
                    slot: a_slot,
                    frame_depth: a_depth,
                },
                Self::TempPointer {
                    slot: b_slot,
                    frame_depth: b_depth,
                },
            ) => a_slot == b_slot && a_depth == b_depth,
            (Self::Null, Self::Null) => true,
            (Self::FragmentRef(a), Self::FragmentRef(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => Arc::ptr_eq(a, b) || a == b,
            (Self::Map(a), Self::Map(b)) => Arc::ptr_eq(a, b) || a == b,
            (
                Self::Record {
                    shape: sa,
                    fields: a,
                },
                Self::Record {
                    shape: sb,
                    fields: b,
                },
            ) => sa == sb && (Arc::ptr_eq(a, b) || a == b),
            // Function values (T1c, docs/t1c-spec.md §5): structural equality
            // is "same fn token and equal bound-arg rows". `FnRef` is the
            // zero-bound case (same token). `Closure` adds the env comparison,
            // with the `Arc::ptr_eq` fast path mirroring the collection arms;
            // a `ref` env entry's `VariablePointer` payload compares by cell,
            // a `val` entry by value — both fall out of `ClosureValue`'s
            // derived `PartialEq`.
            (Self::FnRef(a), Self::FnRef(b)) => a == b,
            (Self::Closure(a), Self::Closure(b)) => Arc::ptr_eq(a, b) || a == b,
            // Handle equality (T1d, docs/t1d-spec.md §6): token equality —
            // same kind and same id, nothing else. No ordering exists (there
            // is no `PartialOrd`/`Ord` impl for `Value`), and a handle is
            // never a legal map key (`MapKey::from_value` has no `Handle`
            // arm, so it falls through to `None` for this variant).
            (Self::Handle { kind: ka, id: ida }, Self::Handle { kind: kb, id: idb }) => {
                ka == kb && ida == idb
            }
            // Projection equality (T1e, docs/t1e-spec.md §4 PROPOSED): same
            // root cell + equal segments, with the `Arc::ptr_eq` fast path
            // mirroring every other heap-allocated variant.
            (Self::Projection(a), Self::Projection(b)) => Arc::ptr_eq(a, b) || a == b,
            // Option equality (NS-A1): structural — `none == none`,
            // `some(x) == some(y)` iff `x == y`, with the `Arc::ptr_eq`
            // fast path on the `some` payload mirroring every other
            // heap-allocated variant. Cross-variant (`some(1) == 1`) falls
            // through to `false` below — the ruled `Option[T] ≠ T`
            // strictness at the value layer.
            // Weighted equality (NS-A7): multiset content with the
            // `Arc::ptr_eq` fast path — see `WeightedValue`'s `PartialEq`.
            (Self::Weighted(a), Self::Weighted(b)) => Arc::ptr_eq(a, b) || a == b,
            (Self::OptionVal(a), Self::OptionVal(b)) => match (a, b) {
                (None, None) => true,
                (Some(x), Some(y)) => Arc::ptr_eq(x, y) || x == y,
                _ => false,
            },
            // Range equality (NS-A5, F7 "content equality"): two ranges are
            // equal iff they denote the same integer sequence — the written
            // form (`..` vs `..=`) is display fidelity, not content, so
            // `1..=6 == 1..7`, and every empty range equals every other
            // empty range (both denote the zero-length sequence, exactly as
            // two empty arrays are equal). The #909 map ruling is the
            // precedent: content compares, form displays.
            (a @ Self::Range { start: sa, .. }, b @ Self::Range { start: sb, .. }) => {
                let (la, lb) = (a.range_len(), b.range_len());
                match (la, lb) {
                    (Some(0), Some(0)) => true,
                    (Some(x), Some(y)) => x == y && sa == sb,
                    // Unreachable: both sides are `Range`.
                    _ => false,
                }
            }
            // Tower equality (NS-A8, `docs/tower-mini-spec.md` T4):
            // componentwise IEEE via glam's own `PartialEq` — a NaN lane
            // makes a value unequal to *itself*, `-0.0 == +0.0` per lane,
            // exactly like the bare `Float` arm above. Cross-kind pairs
            // (`Vec2` vs `Vec3`) fall through to `false` below, like every
            // other cross-variant pair.
            (Self::Vec2(a), Self::Vec2(b)) => a == b,
            (Self::Vec3(a), Self::Vec3(b)) => a == b,
            (Self::Vec4(a), Self::Vec4(b)) => a == b,
            (Self::Quat(a), Self::Quat(b)) => a == b,
            (Self::Mat2(a), Self::Mat2(b)) => a == b,
            (Self::Mat3(a), Self::Mat3(b)) => a == b,
            (Self::Mat4(a), Self::Mat4(b)) => a == b,
            _ => false,
        }
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::Int(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Self::Float(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(Arc::from(v))
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(Arc::from(v))
    }
}

impl From<Arc<str>> for Value {
    fn from(v: Arc<str>) -> Self {
        Self::String(v)
    }
}

impl From<()> for Value {
    /// The unit type maps to [`Null`](Self::Null) — the natural return for a
    /// fire-and-forget external that produces no value.
    fn from((): ()) -> Self {
        Self::Null
    }
}

// NS-A8: identity conversions from the glam compute types (T1 — "one
// workspace-pinned glam version shared with bevy-brink → the bevy marshal is
// identity on the same types"). A host binding returning `impl Into<Value>`
// can hand back a `glam::Vec3` directly.

impl From<glam::Vec2> for Value {
    fn from(v: glam::Vec2) -> Self {
        Self::Vec2(v)
    }
}

impl From<glam::Vec3> for Value {
    fn from(v: glam::Vec3) -> Self {
        Self::Vec3(v)
    }
}

impl From<glam::Vec4> for Value {
    fn from(v: glam::Vec4) -> Self {
        Self::Vec4(v)
    }
}

impl From<glam::Quat> for Value {
    fn from(v: glam::Quat) -> Self {
        Self::Quat(v)
    }
}

impl From<glam::Mat2> for Value {
    fn from(v: glam::Mat2) -> Self {
        Self::Mat2(v)
    }
}

impl From<glam::Mat3> for Value {
    fn from(v: glam::Mat3) -> Self {
        Self::Mat3(v)
    }
}

impl From<glam::Mat4> for Value {
    fn from(v: glam::Mat4) -> Self {
        Self::Mat4(v)
    }
}

/// An ink list value: a set of list items plus their origin list definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListValue {
    /// The active items in this list (each a `ListItem` `DefinitionId`).
    pub items: Vec<DefinitionId>,
    /// The origin list definitions this value was derived from.
    pub origins: Vec<DefinitionId>,
}

/// A scalar key for a [`Value::Map`].
///
/// v1 permits `int`, `string`, and `bool` keys (value-model-spec §4). Keys are
/// compared for equality only — never hashed or sorted — because a
/// [`Value::Map`] iterates in insertion order. Two keys of different variants
/// are never equal (an `Int(1)` key and a `Bool(true)` key are distinct).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MapKey {
    /// An integer key.
    Int(i32),
    /// A string key.
    Str(Arc<str>),
    /// A boolean key.
    Bool(bool),
}

impl MapKey {
    /// Derive a map key from a scalar [`Value`], or `None` if the value is not
    /// one of the permitted key types (`int`/`string`/`bool`).
    ///
    /// This is the seam the collection opcodes (T1b) use to turn an indexing
    /// operand into a key; keeping it here keeps the permitted key domain in
    /// one place.
    pub fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Int(n) => Some(Self::Int(*n)),
            Value::String(s) => Some(Self::Str(Arc::clone(s))),
            Value::Bool(b) => Some(Self::Bool(*b)),
            _ => None,
        }
    }
}

impl From<i32> for MapKey {
    fn from(v: i32) -> Self {
        Self::Int(v)
    }
}

impl From<bool> for MapKey {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<&str> for MapKey {
    fn from(v: &str) -> Self {
        Self::Str(Arc::from(v))
    }
}

impl From<String> for MapKey {
    fn from(v: String) -> Self {
        Self::Str(Arc::from(v))
    }
}

impl From<Arc<str>> for MapKey {
    fn from(v: Arc<str>) -> Self {
        Self::Str(v)
    }
}

/// The payload of [`Value::Map`]: an insertion-ordered map with scalar keys.
///
/// Backed by a flat `Vec` of `(key, value)` entries. Game-scale maps are
/// small, and a flat vector beats persistent/HAMT or hashed structures on both
/// constant factors and wasm size (value-model-spec §5). Iteration is
/// insertion order — the ratified ruling for v1 (§4) — so the structure is
/// deterministic without any sorting, and there is no `HashMap` to leak
/// iteration order.
///
/// `insert`/`remove` preserve insertion order: re-inserting an existing key
/// overwrites its value in place (keeping the key's original position), and
/// The payload of a [`Value::Weighted`] (NS-A7, `docs/stdlib-spec.md` §8):
/// positive-int weights over values, kept in construction order.
///
/// `PartialEq` is hand-written: equality is **multiset content** — the same
/// `(weight, value)` entries with the same multiplicities, regardless of
/// order (the #909 map content-over-form precedent applied to the F17
/// multiset: `weighted(3, "a", 1, "b") == weighted(1, "b", 3, "a")`).
/// Duplicate entries are legal and multiplicity-sensitive. Construction
/// order still governs display and the `roll` draw walk (deterministic
/// offset → entry mapping), exactly as map iteration order survives the
/// order-insensitive map equality. O(n²) matching — the accepted trade for
/// small, hand-written game-scale tables (same as `OrderedMap`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedValue {
    /// `(weight, value)` entries in construction order. Invariant (held by
    /// the only producer, `weighted_new`, and the wire reader): non-empty,
    /// every weight ≥ 1.
    pub entries: Vec<(i32, Value)>,
}

impl WeightedValue {
    /// The total weight of the table as an `i64` (a sum of `i32` weights
    /// can exceed `i32::MAX`; the draw walks in `i64`).
    #[must_use]
    pub fn total_weight(&self) -> i64 {
        self.entries.iter().map(|(w, _)| i64::from(*w)).sum()
    }
}

impl PartialEq for WeightedValue {
    fn eq(&self, other: &Self) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }
        let mut used = vec![false; other.entries.len()];
        'outer: for (w, v) in &self.entries {
            for (i, (ow, ov)) in other.entries.iter().enumerate() {
                if !used[i] && w == ow && v == ov {
                    used[i] = true;
                    continue 'outer;
                }
            }
            return false;
        }
        true
    }
}

/// `remove` shifts later entries down. Lookups are linear; that is the
/// intended trade for small maps and stable ordering.
///
/// `PartialEq` is hand-written, not derived (issue #909, ruled 2026-07-18 —
/// `docs/decision-log.md` "Map/record equality is insertion-order-insensitive"):
/// equality is **content-based**, comparing key→value pairs regardless of
/// insertion order — `#{a:1,b:2} == #{b:2,a:1}` is `true`. Only equality
/// ignores order; [`iter`](Self::iter)/[`keys`](Self::keys)/[`values`](Self::values)
/// and every codec still walk `entries` in insertion order, unchanged. The
/// derived `PartialEq` this replaces compared `entries` as a `Vec`, which is
/// order-sensitive — the bug. `Value::Map`'s `Arc::ptr_eq` fast path (same
/// snapshot → instant `true`) lives one level up in `impl PartialEq for
/// Value`; this impl is the structural fallback it calls into.
#[derive(Debug, Clone, Default, Serialize)]
pub struct OrderedMap {
    entries: Vec<(MapKey, Value)>,
}

impl<'de> Deserialize<'de> for OrderedMap {
    /// Hand-written, not derived (issue #985, follow-up to #909): the derived
    /// impl would deserialize `entries` verbatim as a `Vec<(MapKey, Value)>`,
    /// letting a crafted or corrupt payload carry a duplicate key and
    /// construct a map that violates the content-based `Eq` invariant above
    /// — `Eq` assumes each key appears at most once. This decodes into the
    /// same shape the derive would have produced, then walks the entries
    /// through the same duplicate-key check the `.inkb`/`.inkt`/transcript
    /// decoders use (rejecting rather than silently keeping the last
    /// occurrence — a legitimate encoder never emits a repeat, since
    /// `insert` de-duplicates on the write side, so a repeat is corrupt
    /// input, never a panic).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Shadow {
            entries: Vec<(MapKey, Value)>,
        }

        let shadow = Shadow::deserialize(deserializer)?;
        let mut map = Self::with_capacity(shadow.entries.len());
        for (key, value) in shadow.entries {
            if map.contains_key(&key) {
                return Err(serde::de::Error::custom("duplicate key in map value"));
            }
            map.insert(key, value);
        }
        Ok(map)
    }
}

impl PartialEq for OrderedMap {
    /// Content comparison: same number of entries, and every key in `self`
    /// maps to an equal value in `other`. Order-insensitive by construction
    /// (a lookup by key, not a positional walk) — the len check is a fast
    /// path (mismatched sizes can never be equal, and it makes the
    /// same-length-different-keys case cheap to reject), and the per-entry
    /// `get` gives each comparison the same `Value::eq` `Arc::ptr_eq`
    /// shortcuts as any other structural compare. `O(n)` entries, each a
    /// linear `get` — `O(n^2)` worst case, the accepted trade for small,
    /// game-scale maps (same trade `get`/`insert`/`remove` already make).
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().all(|(key, value)| {
                other
                    .get(key)
                    .is_some_and(|other_value| other_value == value)
            })
    }
}

impl OrderedMap {
    /// Create an empty map.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Create an empty map with capacity for `n` entries.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            entries: Vec::with_capacity(n),
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Borrow the value for `key`, or `None` if absent.
    pub fn get(&self, key: &MapKey) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Whether `key` is present.
    pub fn contains_key(&self, key: &MapKey) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// Mutably borrow the value for `key`, or `None` if absent. The
    /// intermediate-segment leg of the T1e projection RMW spine
    /// (`docs/t1e-spec.md` §3): recursing a `make_mut` chain through a map
    /// needs a mutable handle to an *existing* entry without touching
    /// insertion order, which [`insert`](Self::insert) alone (add-or-replace)
    /// can't provide.
    pub fn get_mut(&mut self, key: &MapKey) -> Option<&mut Value> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Insert `value` under `key`, returning the previous value if the key was
    /// already present.
    ///
    /// An existing key keeps its insertion position (only its value changes);
    /// a new key is appended, so first-insertion order is preserved.
    pub fn insert(&mut self, key: MapKey, value: Value) -> Option<Value> {
        if let Some((_, slot)) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(core::mem::replace(slot, value))
        } else {
            self.entries.push((key, value));
            None
        }
    }

    /// Remove `key`, returning its value if it was present. Later entries shift
    /// down, so insertion order among the survivors is preserved.
    pub fn remove(&mut self, key: &MapKey) -> Option<Value> {
        let idx = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(idx).1)
    }

    /// Iterate `(key, value)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&MapKey, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Iterate keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &MapKey> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// Iterate values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl FromIterator<(MapKey, Value)> for OrderedMap {
    /// Collect entries in order. If a key repeats, the last value wins while
    /// the key keeps its first-insertion position — matching [`insert`].
    ///
    /// [`insert`]: OrderedMap::insert
    fn from_iter<I: IntoIterator<Item = (MapKey, Value)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DefinitionTag;

    #[test]
    fn value_type_discriminant() {
        assert_eq!(Value::Int(0).value_type(), ValueType::Int);
        assert_eq!(Value::Float(0.0).value_type(), ValueType::Float);
        assert_eq!(Value::Bool(true).value_type(), ValueType::Bool);
        assert_eq!(Value::String("".into()).value_type(), ValueType::String);
        assert_eq!(Value::Null.value_type(), ValueType::Null);

        let list = ListValue {
            items: vec![],
            origins: vec![],
        };
        assert_eq!(Value::List(list.into()).value_type(), ValueType::List);

        let target = DefinitionId::new(DefinitionTag::Address, 1);
        assert_eq!(
            Value::DivertTarget(target).value_type(),
            ValueType::DivertTarget
        );
    }

    #[test]
    fn from_impls_roundtrip() {
        assert_eq!(Value::from(7_i32), Value::Int(7));
        assert_eq!(Value::from(1.5_f32), Value::Float(1.5));
        assert_eq!(Value::from(true), Value::Bool(true));
        assert_eq!(Value::from("hi"), Value::String("hi".into()));
        assert_eq!(Value::from(String::from("hi")), Value::String("hi".into()));
        assert_eq!(Value::from(()), Value::Null);
    }

    #[test]
    fn accessors_are_strict_except_int_to_float() {
        assert_eq!(Value::Int(3).as_int(), Some(3));
        assert_eq!(Value::Float(3.0).as_int(), None);
        assert_eq!(Value::Bool(true).as_int(), None);

        // int->float promotion is allowed (matches ink coercion);
        // float->int truncation is not.
        assert_eq!(Value::Int(3).as_float(), Some(3.0));
        assert_eq!(Value::Float(2.5).as_float(), Some(2.5));

        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Int(1).as_bool(), None);

        assert_eq!(Value::String("x".into()).as_str(), Some("x"));
        assert_eq!(Value::Int(1).as_str(), None);
    }

    // ── Collections: value_type + constructors ─────────────────────────────

    #[test]
    fn collection_value_types() {
        assert_eq!(
            Value::array(vec![Value::Int(1)]).value_type(),
            ValueType::Array
        );
        assert_eq!(Value::map(OrderedMap::new()).value_type(), ValueType::Map);
    }

    #[test]
    fn array_accessors() {
        let v = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let items = v.as_array().expect("is array");
        assert_eq!(items.len(), 2);
        assert!(Value::Int(0).as_array().is_none());
        assert!(v.as_map().is_none());
    }

    // ── MapKey ─────────────────────────────────────────────────────────────

    #[test]
    fn map_key_from_value_permitted_domain() {
        assert_eq!(MapKey::from_value(&Value::Int(3)), Some(MapKey::Int(3)));
        assert_eq!(
            MapKey::from_value(&Value::String("k".into())),
            Some(MapKey::Str("k".into()))
        );
        assert_eq!(
            MapKey::from_value(&Value::Bool(true)),
            Some(MapKey::Bool(true))
        );
        // Non-scalar / disallowed key types are rejected.
        assert_eq!(MapKey::from_value(&Value::Float(1.0)), None);
        assert_eq!(MapKey::from_value(&Value::Null), None);
        assert_eq!(MapKey::from_value(&Value::array(vec![])), None);
    }

    #[test]
    fn map_key_variants_are_distinct() {
        // 1, "1", and true are three different keys even though they might
        // coerce to each other elsewhere in the VM.
        assert_ne!(MapKey::Int(1), MapKey::Bool(true));
        assert_ne!(MapKey::from(1), MapKey::from("1"));
        assert_ne!(MapKey::from(true), MapKey::from(false));
        assert_eq!(MapKey::from(1), MapKey::Int(1));
        assert_eq!(MapKey::from("a"), MapKey::Str("a".into()));
    }

    // ── OrderedMap: insertion order, insert/get/remove ─────────────────────

    #[test]
    fn ordered_map_preserves_insertion_order() {
        let mut m = OrderedMap::new();
        assert!(m.is_empty());
        m.insert(MapKey::from("b"), Value::Int(2));
        m.insert(MapKey::from("a"), Value::Int(1));
        m.insert(MapKey::from("c"), Value::Int(3));
        let keys: Vec<&MapKey> = m.keys().collect();
        assert_eq!(
            keys,
            vec![&MapKey::from("b"), &MapKey::from("a"), &MapKey::from("c")]
        );
        assert_eq!(m.len(), 3);
        assert_eq!(m.get(&MapKey::from("a")), Some(&Value::Int(1)));
        assert!(m.contains_key(&MapKey::from("c")));
        assert!(!m.contains_key(&MapKey::from("z")));
    }

    #[test]
    fn ordered_map_reinsert_keeps_position_and_returns_old() {
        let mut m = OrderedMap::new();
        m.insert(MapKey::from("x"), Value::Int(1));
        m.insert(MapKey::from("y"), Value::Int(2));
        let old = m.insert(MapKey::from("x"), Value::Int(9));
        assert_eq!(old, Some(Value::Int(1)));
        // Order unchanged: x still first.
        let keys: Vec<&MapKey> = m.keys().collect();
        assert_eq!(keys, vec![&MapKey::from("x"), &MapKey::from("y")]);
        assert_eq!(m.get(&MapKey::from("x")), Some(&Value::Int(9)));
    }

    #[test]
    fn ordered_map_remove_shifts_survivors() {
        let mut m = OrderedMap::new();
        m.insert(MapKey::from("a"), Value::Int(1));
        m.insert(MapKey::from("b"), Value::Int(2));
        m.insert(MapKey::from("c"), Value::Int(3));
        assert_eq!(m.remove(&MapKey::from("b")), Some(Value::Int(2)));
        assert_eq!(m.remove(&MapKey::from("b")), None);
        let keys: Vec<&MapKey> = m.keys().collect();
        assert_eq!(keys, vec![&MapKey::from("a"), &MapKey::from("c")]);
    }

    #[test]
    fn ordered_map_from_iter_last_wins_first_position() {
        let m: OrderedMap = [
            (MapKey::from("a"), Value::Int(1)),
            (MapKey::from("b"), Value::Int(2)),
            (MapKey::from("a"), Value::Int(10)),
        ]
        .into_iter()
        .collect();
        assert_eq!(m.len(), 2);
        let keys: Vec<&MapKey> = m.keys().collect();
        assert_eq!(keys, vec![&MapKey::from("a"), &MapKey::from("b")]);
        assert_eq!(m.get(&MapKey::from("a")), Some(&Value::Int(10)));
    }

    // ── OrderedMap::deserialize: duplicate-key rejection (#985, follow-up to
    // #909) ──────────────────────────────────────────────────────────────
    //
    // `OrderedMap`'s `Eq` is content-based and assumes each key appears at
    // most once. A legitimate `Serialize` never emits a duplicate key —
    // `insert` de-duplicates on the write side — so a JSON payload with a
    // repeated key only ever arises from a hand-crafted or corrupted save/
    // journal file (the serde deserialize boundary `Story::load_state` and
    // friends go through). The hand-written `Deserialize` below must reject
    // it with a decode error, never silently keep the last occurrence and
    // hand back a map that violates the invariant its `Eq` relies on.

    #[test]
    fn ordered_map_deserialize_rejects_duplicate_key() {
        let json = r#"{"entries":[[{"Str":"a"},{"Int":1}],[{"Str":"a"},{"Int":2}]]}"#;
        let err = serde_json::from_str::<OrderedMap>(json)
            .expect_err("duplicate key must not deserialize");
        assert!(
            err.to_string().contains("duplicate key"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ordered_map_deserialize_accepts_distinct_keys() {
        let json = r#"{"entries":[[{"Str":"a"},{"Int":1}],[{"Str":"b"},{"Int":2}]]}"#;
        let m: OrderedMap = serde_json::from_str(json).expect("distinct keys must deserialize");
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&MapKey::from("a")), Some(&Value::Int(1)));
        assert_eq!(m.get(&MapKey::from("b")), Some(&Value::Int(2)));
    }

    #[test]
    fn ordered_map_serde_json_round_trip_without_duplicates() {
        let mut m = OrderedMap::new();
        m.insert(MapKey::from("hp"), Value::Int(10));
        m.insert(MapKey::from(true), Value::String("flag".into()));
        m.insert(MapKey::from(7), Value::Float(1.5));
        let json = serde_json::to_string(&m).expect("serialize");
        let back: OrderedMap = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, m);
    }

    // A crafted `Value::Map` payload (the shape a `SaveState`/journal decode
    // actually walks) must reject the same way as the bare `OrderedMap` case
    // above — the duplicate-key check has to fire through `Value`'s derived
    // `Deserialize` too, not just when `OrderedMap` is deserialized directly.
    // ── NS-A8 tower: equality + serde lane discipline ──────────────────

    #[test]
    fn tower_equality_is_componentwise_ieee() {
        let a = Value::Vec2(glam::Vec2::new(1.0, 2.0));
        assert_eq!(a, Value::Vec2(glam::Vec2::new(1.0, 2.0)));
        // -0 == +0 per lane; a NaN lane never equals itself (T4).
        assert_eq!(
            Value::Vec2(glam::Vec2::new(-0.0, 1.0)),
            Value::Vec2(glam::Vec2::new(0.0, 1.0))
        );
        let nan = Value::Vec3(glam::Vec3::new(f32::NAN, 0.0, 0.0));
        assert_ne!(nan.clone(), nan);
        // Cross-kind is plain inequality at the value layer.
        assert_ne!(a, Value::Vec3(glam::Vec3::new(1.0, 2.0, 0.0)));
    }

    /// T5: the serde form is the flat lane array — explicit lanes
    /// (column-major for matrices), never glam's memory representation.
    #[test]
    fn tower_serde_is_flat_lane_arrays() {
        let v = Value::Vec3(glam::Vec3::new(1.0, 2.5, -3.0));
        let json = serde_json::to_string(&v).expect("serialize");
        assert_eq!(json, r#"{"Vec3":[1.0,2.5,-3.0]}"#);
        let back: Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, v);

        let m = Value::Mat2(glam::Mat2::from_cols_array(&[1.0, 2.0, 3.0, 4.0]));
        let json = serde_json::to_string(&m).expect("serialize");
        assert_eq!(json, r#"{"Mat2":[1.0,2.0,3.0,4.0]}"#);
        let back: Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, m);

        let q = Value::Quat(glam::Quat::from_xyzw(0.1, 0.2, 0.3, 0.4));
        let back: Value = serde_json::from_str(&serde_json::to_string(&q).expect("serialize"))
            .expect("deserialize");
        assert_eq!(back, q);
    }

    #[test]
    fn tower_accessors_and_from_impls_are_identity() {
        let v = glam::Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(Value::from(v).as_vec3(), Some(v));
        assert_eq!(Value::from(v).as_vec2(), None);
        let m = glam::Mat4::IDENTITY;
        assert_eq!(Value::from(m).as_mat4(), Some(m));
        assert_eq!(Value::Int(1).as_quat(), None);
    }

    #[test]
    fn value_map_deserialize_rejects_duplicate_key() {
        let json = r#"{"Map":{"entries":[[{"Str":"a"},{"Int":1}],[{"Str":"a"},{"Int":2}]]}}"#;
        let err =
            serde_json::from_str::<Value>(json).expect_err("duplicate key must not deserialize");
        assert!(
            err.to_string().contains("duplicate key"),
            "unexpected error: {err}"
        );
    }

    // ── Copy-on-write mechanics (take → make_mut → write-back) ──────────────

    #[test]
    fn clone_is_arc_bump_not_deep_copy() {
        let v = Value::array(vec![Value::Int(1)]);
        let arc = Arc::clone(v.as_array().expect("array"));
        assert_eq!(Arc::strong_count(&arc), 2); // v + arc
        let v2 = v.clone();
        assert_eq!(Arc::strong_count(&arc), 3); // v + v2 + arc
        drop(v2);
        assert_eq!(Arc::strong_count(&arc), 2);
    }

    #[test]
    fn array_make_mut_in_place_when_unique() {
        let mut v = Value::array(vec![Value::Int(1)]);
        // Unique Arc: `make_mut` returns the same allocation, no COW copy.
        // (Compare the Arc allocation address, not the Vec's data buffer,
        // which may move when `push` grows capacity.)
        let arc_before = Arc::as_ptr(v.as_array().expect("array"));
        v.array_make_mut().expect("array").push(Value::Int(2));
        let arc_after = Arc::as_ptr(v.as_array().expect("array"));
        assert_eq!(arc_before, arc_after, "unique Arc mutates in place");
        assert_eq!(v.as_array().expect("array").len(), 2);
    }

    #[test]
    fn array_make_mut_copies_when_shared() {
        let original = Value::array(vec![Value::Int(1)]);
        let mut copy = original.clone(); // shares the Arc
        // Mutate the copy: COW must fork so `original` is untouched.
        copy.array_make_mut().expect("array").push(Value::Int(2));
        assert_eq!(
            original.as_array().expect("array").as_slice(),
            &[Value::Int(1)]
        );
        assert_eq!(copy.as_array().expect("array").len(), 2);
        // After the fork both are unique again.
        assert_eq!(Arc::strong_count(original.as_array().expect("array")), 1);
    }

    #[test]
    fn map_make_mut_copies_when_shared() {
        let mut base = OrderedMap::new();
        base.insert(MapKey::from("a"), Value::Int(1));
        let original = Value::map(base);
        let mut copy = original.clone();
        copy.map_make_mut()
            .expect("map")
            .insert(MapKey::from("b"), Value::Int(2));
        assert_eq!(original.as_map().expect("map").len(), 1);
        assert_eq!(copy.as_map().expect("map").len(), 2);
    }

    #[test]
    fn make_mut_returns_none_for_non_collection() {
        assert!(Value::Int(1).array_make_mut().is_none());
        assert!(Value::Int(1).map_make_mut().is_none());
    }

    /// `record_make_mut` gets the same COW proof as `array_make_mut`/
    /// `map_make_mut` above — issue #1476's audit found this variant was the
    /// one collection `make_mut` without a dedicated "copies when shared"
    /// regression, despite sharing the exact take → `make_mut` → write-back
    /// discipline (the COW discipline documented on [`Value`] —
    /// `docs/value-model-spec.md` §4/§5).
    #[test]
    fn record_make_mut_copies_when_shared() {
        let shape = ShapeId(0);
        let original = Value::record(shape, vec![Value::Int(1), Value::Int(2)]);
        let mut copy = original.clone(); // shares the Arc
        copy.record_make_mut().expect("record")[0] = Value::Int(99);
        assert_eq!(
            original.as_record().expect("record").1.as_slice(),
            &[Value::Int(1), Value::Int(2)],
            "mutating the copy must never be observable through the original"
        );
        assert_eq!(
            copy.as_record().expect("record").1.as_slice(),
            &[Value::Int(99), Value::Int(2)]
        );
        // After the fork both are unique again.
        assert_eq!(
            Arc::strong_count(original.as_record().expect("record").1),
            1
        );
    }

    /// The classic nested-collection leak site (issue #1476), one layer
    /// deeper: a `Record` field itself holds an `Array` (two independent
    /// `Arc`s stacked — the record's field vec, and the array's backing
    /// vec). `let y = x` then mutating `x`'s field-array in place must never
    /// surface through `y`, exactly like a bare `Array`-of-`Array`
    /// (`rmw-mutator-shared-nested-lvalue`, `tests/tier1-brink/`).
    ///
    /// This is pinned only at the `Value` layer, not as an end-to-end
    /// `tests/tier1-brink/` fixture, because the obvious source form —
    /// `STRUCT Bag = #{ items: Array<int> }`, `push(a.items, 3)` — does not
    /// currently reach this code path: `push`/`insert`/`remove`'s
    /// bare-lvalue fast path (`try_lower_mutator_stmt` in
    /// `brink-ir::lir::lower::blocks`) treats *any* `hir::Expr::Path`
    /// lvalue, including a multi-segment TM-4b dotted field-access path
    /// like `a.items`, as a single bare-variable target and resolves the
    /// whole path's range to its root symbol — unlike plain field
    /// assignment's `try_lower_field_assignment`, which explicitly special-
    /// cases `path.segments.len() > 1`. The mutator ends up applied to `a`
    /// itself (a `Record`) instead of `a`'s `items` field, so `push(a.items,
    /// 3)` compiles clean but faults at runtime with
    /// `RuntimeError::NotIndexable("record")` rather than mutating the
    /// nested array. That is a real compiler bug, not a deliberate
    /// limitation — tracked separately; this `Value`-layer test is what
    /// proves the runtime's own COW discipline is already correct once a
    /// future fix routes this lvalue shape through the field-projection
    /// path instead.
    #[test]
    fn nested_array_inside_record_field_is_isolated_after_copy() {
        let shape = ShapeId(0);
        let inner = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let original = Value::record(shape, vec![Value::String("bag".into()), inner]);
        let mut copy = original.clone(); // shares both Arcs (record + inner array)

        // Take → make_mut → write-back on the copy's inner array field,
        // mirroring `collection_ops`'s RMW discipline: pull the field out,
        // COW-mutate it, write it back into the (already-uniqued) record.
        let fields = copy.record_make_mut().expect("record");
        let mut inner_copy = fields[1].clone();
        inner_copy
            .array_make_mut()
            .expect("array")
            .push(Value::Int(3));
        fields[1] = inner_copy;

        let original_inner = original
            .as_record()
            .expect("record")
            .1
            .get(1)
            .expect("field 1")
            .as_array()
            .expect("array");
        assert_eq!(
            original_inner.as_slice(),
            &[Value::Int(1), Value::Int(2)],
            "mutating the copy's nested array must never be observable through the original record"
        );
        let copy_inner = copy
            .as_record()
            .expect("record")
            .1
            .get(1)
            .expect("field 1")
            .as_array()
            .expect("array");
        assert_eq!(
            copy_inner.as_slice(),
            &[Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    // ── Structural equality with the ptr_eq fast path ──────────────────────

    #[test]
    fn array_equality_is_structural_across_distinct_arcs() {
        let a = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let b = Value::array(vec![Value::Int(1), Value::Int(2)]);
        // Distinct Arcs, equal contents.
        assert!(!Arc::ptr_eq(a.as_array().unwrap(), b.as_array().unwrap()));
        assert_eq!(a, b);
        let c = Value::array(vec![Value::Int(1), Value::Int(3)]);
        assert_ne!(a, c);
    }

    #[test]
    fn nested_collection_equality() {
        let inner = Value::array(vec![Value::Int(1)]);
        let a = Value::array(vec![inner.clone(), Value::map(OrderedMap::new())]);
        let b = Value::array(vec![
            Value::array(vec![Value::Int(1)]),
            Value::map(OrderedMap::new()),
        ]);
        assert_eq!(a, b);
    }

    #[test]
    fn shared_snapshot_is_equal_via_ptr_eq() {
        let a = Value::array(vec![Value::Int(1)]);
        let snapshot = a.clone(); // same Arc
        assert!(Arc::ptr_eq(
            a.as_array().unwrap(),
            snapshot.as_array().unwrap()
        ));
        assert_eq!(a, snapshot);
    }

    #[test]
    fn distinct_nan_arrays_never_equal_but_same_snapshot_is() {
        let a = Value::array(vec![Value::Float(f32::NAN)]);
        let b = Value::array(vec![Value::Float(f32::NAN)]);
        // Different Arcs: structural compare hits NaN != NaN.
        assert_ne!(a, b);
        // Same Arc (snapshot): ptr_eq fast path wins — equal even with NaN.
        // The spec calls this out as harmless and stated (§4).
        let snapshot = a.clone();
        assert_eq!(a, snapshot);
    }

    #[test]
    fn map_equality_is_content_based_insertion_order_insensitive() {
        // Issue #909, ruled 2026-07-18: #{a:1,b:2} == #{b:2,a:1} is TRUE.
        // Two maps with the same key/value pairs inserted in different
        // orders are the same value — equality ignores insertion order even
        // though iteration/serialization order still follows it.
        let m1: OrderedMap = [
            (MapKey::from("a"), Value::Int(1)),
            (MapKey::from("b"), Value::Int(2)),
        ]
        .into_iter()
        .collect();
        let m2: OrderedMap = [
            (MapKey::from("b"), Value::Int(2)),
            (MapKey::from("a"), Value::Int(1)),
        ]
        .into_iter()
        .collect();
        // Both directions — PartialEq::eq isn't assumed symmetric by the
        // impl, so both orderings of the comparison are checked explicitly.
        assert_eq!(Value::map(m1.clone()), Value::map(m2.clone()));
        assert_eq!(Value::map(m2.clone()), Value::map(m1.clone()));
        assert_eq!(Value::map(m1.clone()), Value::map(m1.clone()));

        // Iteration order is unaffected by the equality ruling — each map
        // still yields its own entries in the order they were inserted.
        assert_eq!(
            m1.keys().cloned().collect::<Vec<_>>(),
            vec![MapKey::from("a"), MapKey::from("b")]
        );
        assert_eq!(
            m2.keys().cloned().collect::<Vec<_>>(),
            vec![MapKey::from("b"), MapKey::from("a")]
        );
    }

    #[test]
    fn map_equality_still_rejects_different_content() {
        // Content-based equality must still distinguish maps that genuinely
        // differ — same key count, different values; and different key
        // counts entirely (exercises the len fast-path).
        let a: OrderedMap = [(MapKey::from("a"), Value::Int(1))].into_iter().collect();
        let b: OrderedMap = [(MapKey::from("a"), Value::Int(2))].into_iter().collect();
        assert_ne!(Value::map(a.clone()), Value::map(b));

        let c: OrderedMap = [
            (MapKey::from("a"), Value::Int(1)),
            (MapKey::from("b"), Value::Int(2)),
        ]
        .into_iter()
        .collect();
        assert_ne!(Value::map(a), Value::map(c));
    }

    #[test]
    fn nested_map_equality_is_order_insensitive_at_every_level() {
        // A map value nested inside another map/array is compared through
        // the same content-based rule, recursively — reordering the inner
        // map's keys must not change the outer value's equality.
        let inner1: OrderedMap = [
            (MapKey::from("x"), Value::Int(1)),
            (MapKey::from("y"), Value::Int(2)),
        ]
        .into_iter()
        .collect();
        let inner2: OrderedMap = [
            (MapKey::from("y"), Value::Int(2)),
            (MapKey::from("x"), Value::Int(1)),
        ]
        .into_iter()
        .collect();

        let outer1: OrderedMap = [
            (MapKey::from("inner"), Value::map(inner1)),
            (MapKey::from("other"), Value::Int(9)),
        ]
        .into_iter()
        .collect();
        let outer2: OrderedMap = [
            (MapKey::from("other"), Value::Int(9)),
            (MapKey::from("inner"), Value::map(inner2)),
        ]
        .into_iter()
        .collect();
        assert_eq!(Value::map(outer1), Value::map(outer2));
    }

    #[test]
    fn record_equality_is_unaffected_by_map_ordering_ruling() {
        // Records are shape-ordered, not insertion-ordered (fields have a
        // fixed position from the closed shape) — the map ruling must not
        // change record equality, which stays a positional field compare
        // gated on matching `ShapeId`. Field order can't be reordered
        // through the public API, so this locks the existing behavior
        // rather than exercising a new order-insensitivity path.
        let shape = ShapeId(0);
        let r1 = Value::record(shape, vec![Value::Int(1), Value::Int(2)]);
        let r2 = Value::record(shape, vec![Value::Int(1), Value::Int(2)]);
        let r3 = Value::record(shape, vec![Value::Int(2), Value::Int(1)]);
        assert_eq!(r1, r2);
        assert_ne!(r1, r3);
    }

    #[test]
    fn cross_type_inequality_unaffected() {
        // The hand-written PartialEq must keep the derive's cross-variant
        // behavior: different variants are never equal.
        assert_ne!(Value::Int(1), Value::Bool(true));
        assert_ne!(Value::array(vec![]), Value::Null);
        assert_ne!(Value::array(vec![]), Value::map(OrderedMap::new()));
        assert_eq!(Value::Null, Value::Null);
    }

    // ── Tree serialization (T1a-3 / #525) ──────────────────────────────────
    //
    // SaveState and the session journal serialize `Value` through its derived
    // serde representation (BTreeMap<String, Value> globals; tagged event
    // payloads). These tests lock the *tree* round-trip for the collection
    // variants: an `Array`/`Map` serializes to a nested structure and comes
    // back structurally equal, with insertion order and scalar key types
    // preserved. Sharing is deliberately not preserved on the wire (spec §5) —
    // a snapshot serializes as a plain tree.

    /// Round-trip a value through `serde_json` and assert structural equality.
    fn json_round_trip(v: &Value) -> Value {
        let json = serde_json::to_string(v).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn scalar_serde_round_trip_unchanged() {
        for v in [
            Value::Int(-7),
            Value::Float(1.5),
            Value::Bool(true),
            Value::String("hi".into()),
            Value::Null,
        ] {
            assert_eq!(json_round_trip(&v), v);
        }
    }

    #[test]
    fn array_serde_round_trip_is_structural() {
        let v = Value::array(vec![
            Value::Int(1),
            Value::String("two".into()),
            Value::Bool(false),
        ]);
        let back = json_round_trip(&v);
        assert_eq!(back, v);
        assert_eq!(back.value_type(), ValueType::Array);
    }

    #[test]
    fn map_serde_round_trip_preserves_order_and_key_types() {
        // Mixed scalar key types and a deliberately non-sorted insertion order.
        let m: OrderedMap = [
            (MapKey::from("z"), Value::Int(1)),
            (MapKey::from(10), Value::Int(2)),
            (MapKey::from(true), Value::Int(3)),
            (MapKey::from("a"), Value::Int(4)),
        ]
        .into_iter()
        .collect();
        let v = Value::map(m);
        let back = json_round_trip(&v);
        // Structural equality is order-sensitive, so this also proves the wire
        // form preserved insertion order and each key's variant.
        assert_eq!(back, v);
        let back_map = back.as_map().expect("map");
        let keys: Vec<&MapKey> = back_map.keys().collect();
        assert_eq!(
            keys,
            vec![
                &MapKey::from("z"),
                &MapKey::from(10),
                &MapKey::from(true),
                &MapKey::from("a"),
            ]
        );
    }

    #[test]
    fn nested_collection_serde_round_trip() {
        // An array of maps of arrays — the recursive tree case.
        let inner_map: OrderedMap = [
            (
                MapKey::from("items"),
                Value::array(vec![Value::Int(1), Value::Int(2)]),
            ),
            (MapKey::from("name"), Value::String("goblin".into())),
        ]
        .into_iter()
        .collect();
        let v = Value::array(vec![
            Value::map(inner_map),
            Value::array(vec![Value::map(OrderedMap::new())]),
            Value::Null,
        ]);
        assert_eq!(json_round_trip(&v), v);
    }

    // ── Handle (T1d, docs/t1d-spec.md §2/§6) ────────────────────────────────

    #[test]
    fn handle_value_type_and_constructor() {
        let h = Value::handle(NameId(3), 42);
        assert_eq!(h.value_type(), ValueType::Handle);
        assert_eq!(h.as_handle(), Some((NameId(3), 42)));
        assert!(Value::Int(0).as_handle().is_none());
    }

    #[test]
    fn handle_equality_is_token_equality() {
        // Same kind, same id: equal.
        assert_eq!(Value::handle(NameId(1), 42), Value::handle(NameId(1), 42));
        // Same id, different kind: not equal — kind is part of the token.
        assert_ne!(Value::handle(NameId(1), 42), Value::handle(NameId(2), 42));
        // Same kind, different id: not equal.
        assert_ne!(Value::handle(NameId(1), 1), Value::handle(NameId(1), 2));
        // A Handle is never equal to any other variant, even with a
        // coincidentally matching id (compare against a DivertTarget encoding
        // the same raw bits).
        assert_ne!(
            Value::handle(NameId(1), 42),
            Value::DivertTarget(DefinitionId::new(DefinitionTag::Address, 42))
        );
    }

    #[test]
    fn handle_is_not_a_legal_map_key() {
        // MapKey's domain is int/string/bool (value-model-spec §4); Handle
        // has no `MapKey::from_value` arm and falls through to `None`.
        assert_eq!(MapKey::from_value(&Value::handle(NameId(1), 42)), None);
    }

    #[test]
    fn handle_serde_round_trip_is_structural() {
        let v = Value::handle(NameId(7), u64::MAX);
        let back = json_round_trip(&v);
        assert_eq!(back, v);
        assert_eq!(back.value_type(), ValueType::Handle);
        assert_eq!(back.as_handle(), Some((NameId(7), u64::MAX)));
    }

    #[test]
    fn handle_nested_in_collection_serde_round_trip() {
        let v = Value::array(vec![
            Value::handle(NameId(1), 1),
            Value::handle(NameId(2), 2),
            Value::Null,
        ]);
        assert_eq!(json_round_trip(&v), v);
    }

    // ── Option (NS-A1, docs/stdlib-spec.md §1.1/§1.4) ───────────────────

    #[test]
    fn option_value_type_and_constructors() {
        assert_eq!(Value::none().value_type(), ValueType::Option);
        assert_eq!(Value::some(Value::Int(3)).value_type(), ValueType::Option);
        assert_eq!(Value::none().as_option(), Some(None));
        assert_eq!(
            Value::some(Value::Int(3)).as_option(),
            Some(Some(&Value::Int(3)))
        );
        assert_eq!(Value::Int(3).as_option(), None);
    }

    #[test]
    fn option_equality_is_structural() {
        assert_eq!(Value::none(), Value::none());
        assert_eq!(Value::some(Value::Int(1)), Value::some(Value::Int(1)));
        assert_ne!(Value::some(Value::Int(1)), Value::some(Value::Int(2)));
        assert_ne!(Value::some(Value::Int(1)), Value::none());
        // The ruled `Option[T] ≠ T` strictness at the value layer: a
        // wrapped value is never equal to its bare form.
        assert_ne!(Value::some(Value::Int(1)), Value::Int(1));
        assert_ne!(Value::none(), Value::Null);
    }

    #[test]
    fn option_nesting_is_preserved() {
        // some(none) is a real value, distinct from none — the enum shape
        // nests like any parameterized builtin.
        let some_none = Value::some(Value::none());
        assert_ne!(some_none, Value::none());
        assert_eq!(some_none, Value::some(Value::none()));
    }

    #[test]
    fn option_clone_is_arc_bump() {
        let v = Value::some(Value::array(vec![Value::Int(1)]));
        let v2 = v.clone();
        let (Value::OptionVal(Some(a)), Value::OptionVal(Some(b))) = (&v, &v2) else {
            unreachable!("both are freshly built some values");
        };
        assert!(Arc::ptr_eq(a, b), "clone shares the payload Arc");
    }

    #[test]
    fn option_serde_round_trip_is_structural() {
        for v in [
            Value::none(),
            Value::some(Value::Int(7)),
            Value::some(Value::none()),
            Value::array(vec![Value::none(), Value::some(Value::from("x"))]),
        ] {
            assert_eq!(json_round_trip(&v), v);
        }
    }

    // ── NS-A5 `Value::Range` (F7, docs/stdlib-spec.md §7) ──────────────────

    #[test]
    fn range_value_type_and_accessors() {
        let r = Value::range(1, 6, true);
        assert_eq!(r.value_type(), ValueType::Range);
        assert_eq!(r.as_range(), Some((1, 6, true)));
        assert_eq!(Value::Int(1).as_range(), None);
        assert_eq!(r.range_end_exclusive(), Some(7));
        assert_eq!(Value::range(1, 7, false).range_end_exclusive(), Some(7));
        assert_eq!(r.range_len(), Some(6));
        assert_eq!(Value::range(0, 0, false).range_len(), Some(0));
        // Backwards ranges are empty, never negative-length.
        assert_eq!(Value::range(5, 2, false).range_len(), Some(0));
        // i64 normalization: 1..=i32::MAX does not overflow.
        assert_eq!(
            Value::range(1, i32::MAX, true).range_end_exclusive(),
            Some(i64::from(i32::MAX) + 1)
        );
        assert_eq!(
            Value::range(i32::MIN, i32::MAX, true).range_len(),
            Some(1i64 << 32)
        );
    }

    #[test]
    fn range_equality_is_content_equality() {
        // Same sequence, different written form: equal (F7 "content
        // equality" — the form is display fidelity, not content).
        assert_eq!(Value::range(1, 6, true), Value::range(1, 7, false));
        assert_eq!(Value::range(1, 7, false), Value::range(1, 6, true));
        // Same form, same bounds: equal.
        assert_eq!(Value::range(0, 3, false), Value::range(0, 3, false));
        // Different sequences: unequal.
        assert_ne!(Value::range(0, 3, false), Value::range(1, 3, false));
        assert_ne!(Value::range(0, 3, false), Value::range(0, 4, false));
        // Every empty range equals every other empty range (both denote
        // the zero-length sequence, like two empty arrays).
        assert_eq!(Value::range(0, 0, false), Value::range(5, 5, false));
        assert_eq!(Value::range(9, 2, false), Value::range(0, 0, false));
        // An empty range is never equal to a non-empty one.
        assert_ne!(Value::range(0, 0, false), Value::range(0, 1, false));
        // Cross-variant: a range is not an array, int, or anything else.
        assert_ne!(Value::range(0, 2, false), Value::array(vec![]));
        assert_ne!(Value::range(0, 2, false), Value::Int(0));
    }

    #[test]
    fn range_serde_round_trip_preserves_the_written_form() {
        for v in [
            Value::range(1, 6, true),
            Value::range(0, 10, false),
            Value::range(-3, 3, false),
            Value::range(0, 0, false),
            Value::array(vec![Value::range(1, 2, true), Value::Int(9)]),
        ] {
            let back = json_round_trip(&v);
            assert_eq!(back, v);
            // The written form survives (not just content equality): the
            // triple round-trips bit-for-bit.
            if let Value::Range { .. } = &v {
                assert_eq!(back.as_range(), v.as_range());
            }
        }
    }
}
