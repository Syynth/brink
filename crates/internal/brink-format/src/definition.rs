use alloc::string::String;
use alloc::vec::Vec;

use crate::counting::CountingFlags;
use crate::id::{DefinitionId, NameId};
use crate::line::LineContent;
use crate::value::{ShapeId, Value, ValueType};

/// A compiled container (knot, stitch, gather, or anonymous flow block).
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerDef {
    pub id: DefinitionId,
    /// The lexical scope this container belongs to.
    /// For scope containers (root, knot, stitch): `scope_id == id`.
    /// For child containers (gather, choice target, sequence, etc.): `scope_id` is
    /// the enclosing scope's `DefinitionId`.
    pub scope_id: DefinitionId,
    /// Human-readable name for scope-owning containers (root, knot, stitch).
    /// `None` for child containers.
    pub name: Option<NameId>,
    pub bytecode: Vec<u8>,
    pub counting_flags: CountingFlags,
    /// Sum of char values from the container's ink path string.
    /// Used to seed the RNG for shuffle sequences.
    pub path_hash: i32,
    /// Number of parameters this container declares (a parameterized knot,
    /// stitch, or function — e.g. `=== call(action, present) ===` has 2). The
    /// container's prologue binds them with that many leading `DeclareTemp`s.
    /// `0` for the vast majority of containers. Lets the runtime arity-check a
    /// host-directed entry (`choose_path_string_with_args`) or `call_function`.
    /// The converter reference pipeline leaves this `0` (inklecate's JSON does
    /// not expose it); only the brink compiler populates the true count.
    pub param_count: u8,
    /// Per-parameter name and mode metadata, in declared order (T1c,
    /// `docs/t1c-spec.md` §6). Empty for the vast majority of containers.
    ///
    /// Carried so the runtime can validate a **rehydrated function value**
    /// against the *current* signature: a `#fn`/closure saved before a
    /// recompile stores its bound params' names and modes, and on load/invoke
    /// they are checked against this table — a renamed or re-moded param is a
    /// defined fault, never a silent misbinding (spec §6). `len()` always
    /// equals [`param_count`](Self::param_count); both are kept (the count is
    /// the pre-T1c arity-check field, the metadata is additive). The converter
    /// reference pipeline leaves this empty.
    pub params: Vec<ParamMeta>,
    /// Compiled scope default: `true` for a flow-private (`#@local`) knot or
    /// stitch. Only ever set on scope-owning containers; subtree coverage of
    /// interior containers is resolved by the runtime at policy resolution
    /// (`docs/directive-annotations-spec.md`). The converter always emits
    /// `false` (inklecate has no flow-private concept).
    pub local: bool,
}

/// Name + mode of one declared parameter of a container (T1c,
/// `docs/t1c-spec.md` §6). See [`ContainerDef::params`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamMeta {
    /// The parameter's interned name.
    pub name: NameId,
    /// `true` if declared `ref`, `false` for a by-value param.
    pub is_ref: bool,
}

/// Metadata for a single interpolation slot in a template line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    pub index: u8,
    pub name: String,
}

/// Source location of a line in the original `.ink` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub range_start: u32,
    pub range_end: u32,
}

/// One entry in a container's line table.
#[derive(Debug, Clone, PartialEq)]
pub struct LineEntry {
    pub content: LineContent,
    pub flags: crate::LineFlags,
    pub source_hash: u64,
    pub audio_ref: Option<String>,
    pub slot_info: Vec<SlotInfo>,
    pub source_location: Option<SourceLocation>,
}

/// A locale line entry — content + optional audio, no source metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct LocaleLineEntry {
    pub content: LineContent,
    pub audio_ref: Option<String>,
}

/// A per-scope locale line table.
#[derive(Debug, Clone, PartialEq)]
pub struct LocaleScopeTable {
    pub scope_id: DefinitionId,
    pub lines: Vec<LocaleLineEntry>,
}

/// Complete locale overlay data from a `.inkl` file.
#[derive(Debug, Clone, PartialEq)]
pub struct LocaleData {
    pub locale_tag: String,
    pub base_checksum: u32,
    pub line_tables: Vec<LocaleScopeTable>,
}

/// Per-scope line table, stored separately from [`ContainerDef`] for
/// locale overlay swapping (`.inkl`).
///
/// All containers within a lexical scope (knot, stitch, or root) share one
/// `ScopeLineTable`. `EmitLine(idx)` indices are scope-relative.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeLineTable {
    pub scope_id: DefinitionId,
    pub lines: Vec<LineEntry>,
}

/// A global variable definition.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalVarDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub value_type: ValueType,
    pub default_value: Value,
    pub mutable: bool,
    /// Compiled scope default: `true` for a flow-private (`#@local`)
    /// variable, `false` for ordinary shared state. Consumed by the runtime
    /// as the base layer of `WorldPolicy` resolution
    /// (`docs/directive-annotations-spec.md`).
    pub local: bool,
}

/// A list (enum-like set) definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// `(item_name, ordinal)` pairs in declaration order.
    pub items: Vec<(NameId, i32)>,
}

/// A `STRUCT` shape definition (TM-4, `docs/typed-mode-spec.md` §6;
/// `StructShapes` section, `docs/format-spec.md` tag `0x0C`).
///
/// Closed shape: `fields` is the ordered set of declared field names — the
/// same order [`crate::value::Value::Record`]'s flat field vector follows,
/// and the order `RecordNew`/static `RecordGet`/`RecordSet` offsets index
/// into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructShapeDef {
    pub id: ShapeId,
    pub name: NameId,
    pub fields: Vec<NameId>,
}

/// One old→new `DefinitionId` rename record (M-3, `docs/modules-spec.md`
/// §5): the compiler emits one of these per `#@was(old_name)` directive —
/// on a module, one entry per definition the renamed module currently
/// owns; on a single definition, one entry for it. Rehydration
/// (`brink-runtime`'s `load_state`) consults the table **only on the miss
/// path**: a saved fn token, divert value, or visit-count key that the
/// current program doesn't recognize is looked up here before being
/// treated as genuinely gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AliasEntry {
    /// The identity a save from before the rename may still carry.
    pub old: DefinitionId,
    /// The definition's current identity.
    pub new: DefinitionId,
}

/// A single list item definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListItemDef {
    pub id: DefinitionId,
    pub origin: DefinitionId,
    pub ordinal: i32,
    pub name: NameId,
}

/// An address pointing to a specific byte offset within a container.
///
/// Addresses are used for divert targets, visit tracking, and any definition
/// that maps to a position within a container. A "primary" address has
/// `byte_offset == 0` and the same `id` as its `container_id`, functioning
/// like the old `Container` tag. Intra-container addresses have non-zero
/// offsets and distinct IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressDef {
    pub id: DefinitionId,
    pub container_id: DefinitionId,
    pub byte_offset: u32,
}

/// Maps a qualified author path (e.g. `knot`, `knot.stitch`, `knot.label`,
/// `knot.stitch.label`) to the [`DefinitionId`] it addresses.
///
/// This is the source of truth for path → address lookup
/// ([`Program::find_address`](../../brink_runtime/struct.Program.html#method.find_address)):
/// the linker resolves each `target` through its address map. The compiler
/// emits one entry per scope container (knot/stitch) and per author-labeled
/// gather/choice. `path` indexes the name table; `target` is the addressed
/// container/label id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressPath {
    pub path: NameId,
    pub target: DefinitionId,
}

/// Compute a deterministic hash of line content text.
///
/// Used by both the compiler codegen and the converter to populate
/// [`LineEntry::source_hash`]. The hash detects when source text has
/// changed across builds, enabling the regeneration workflow in the
/// internationalization pipeline.
pub fn content_hash(text: &str) -> u64 {
    #[cfg(feature = "std")]
    {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
    #[cfg(not(feature = "std"))]
    {
        // `std::collections::hash_map::DefaultHasher` isn't available
        // without `std`. This is a plain FNV-1a fallback: still
        // deterministic, but NOT bit-identical to the `std` path above —
        // nothing compares hashes produced by the two builds against each
        // other, so that's fine.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
}

/// An externally-bound function definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalFnDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub arg_count: u8,
    pub fallback: Option<DefinitionId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let a = content_hash("Hello, world!");
        let b = content_hash("Hello, world!");
        assert_eq!(a, b);
    }

    #[test]
    fn content_hash_non_zero_for_non_empty() {
        assert_ne!(content_hash("some text"), 0);
        assert_ne!(content_hash("x"), 0);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        assert_ne!(content_hash("hello"), content_hash("world"));
    }
}
