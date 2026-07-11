use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::id::DefinitionId;

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
}

/// A runtime value in the ink VM.
///
/// Heap-allocating variants (`String`, `List`, `Array`, `Map`) are wrapped in
/// `Arc` so that cloning a `Value` is always O(1) — a refcount bump, not a
/// deep copy. This matches C#'s reference-type semantics and makes call-frame
/// cloning (during `fork_thread`) essentially free. Atomic refcounts are used
/// so `Value` can flow through Bevy's parallel scheduler.
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
/// `remove` shifts later entries down. Lookups are linear; that is the
/// intended trade for small maps and stable ordering.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OrderedMap {
    entries: Vec<(MapKey, Value)>,
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
    fn map_equality_is_order_sensitive() {
        // Insertion order is observable, so two maps with the same entries in
        // different order are distinct values.
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
        assert_ne!(Value::map(m1.clone()), Value::map(m2));
        assert_eq!(Value::map(m1.clone()), Value::map(m1));
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
}
