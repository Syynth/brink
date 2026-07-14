use alloc::string::String;
use alloc::vec::Vec;

use crate::definition::{
    AddressDef, AddressPath, ContainerDef, ExternalFnDef, GlobalVarDef, ListDef, ListItemDef,
    ScopeLineTable, StructShapeDef,
};
use crate::id::DefinitionId;
use crate::value::ListValue;

/// The top-level compiled story: everything the runtime needs to execute.
#[derive(Debug, Clone, PartialEq)]
pub struct StoryData {
    pub containers: Vec<ContainerDef>,
    /// Per-scope line tables. Each scope (root, knot, stitch) gets one table
    /// shared by all containers within that scope.
    pub line_tables: Vec<ScopeLineTable>,
    pub variables: Vec<GlobalVarDef>,
    pub list_defs: Vec<ListDef>,
    pub list_items: Vec<ListItemDef>,
    pub externals: Vec<ExternalFnDef>,
    /// Address definitions mapping IDs to byte offsets within containers.
    pub addresses: Vec<AddressDef>,
    /// Qualified-path → address-target table. The single source of truth for
    /// [`Program::find_address`](../../brink_runtime/struct.Program.html#method.find_address);
    /// empty for legacy/converter output (the linker then falls back to
    /// deriving scope paths from container names).
    pub address_paths: Vec<AddressPath>,
    /// Interned name strings, indexed by [`NameId`](crate::id::NameId).
    pub name_table: Vec<String>,
    /// List literal values referenced by `PushList(idx)` opcodes.
    pub list_literals: Vec<ListValue>,
    /// The T1b `LiteralPool` (`docs/format-v4-rfc.md` §2): content-hash
    /// deduplicated constant values referenced by `PushLiteral(idx)` opcodes.
    /// Distinct from `list_literals`/`PushList` — this is additive new
    /// surface for T1b collection literals, not a replacement (the RFC's
    /// `ListLiterals` absorption is a separate, larger migration; see the
    /// T1b-2 PR description).
    pub literal_pool: Vec<crate::value::Value>,
    /// The TM-4 `StructShapes` table (`docs/format-spec.md` section tag
    /// `0x0C`): one entry per declared `STRUCT`, indexed by
    /// [`crate::value::ShapeId`]. Referenced by `RecordNew`/static
    /// `RecordGet`/`RecordSet` opcodes and by `Value::Record` values in the
    /// literal pool, globals, and the transcript.
    pub struct_shapes: Vec<StructShapeDef>,
    /// `DefinitionId`s of every `#@private` definition (M-2b,
    /// `docs/modules-spec.md` §4 boundary rule 2). Sorted ascending by raw id
    /// for determinism. Empty for the entire pre-modules / all-public world,
    /// in which case the `.inkb` `Visibility` section (tag `0x0E`) is omitted
    /// entirely — so public-only stories stay byte-identical.
    ///
    /// This is the complement encoding: public is the default, private names
    /// are enumerated (mirroring how `#@local` scope defaults are carried).
    /// The runtime builds a lookup set from it and refuses host **semantic**
    /// access (variable get/set, entry lookup, function eval) to these defs;
    /// host **persistence** (save/load/journal/replay) ignores it and sees
    /// everything (§4 boundary rule 2).
    pub private_defs: Vec<DefinitionId>,
    /// CRC-32 checksum from the `.inkb` header, used for locale validation.
    /// Zero for stories not loaded from `.inkb`.
    pub source_checksum: u32,
}
