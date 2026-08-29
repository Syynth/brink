use alloc::string::String;
use alloc::vec::Vec;

use crate::definition::{
    AddressDef, AddressPath, AliasEntry, ContainerDef, DebugInfoSection, EffectRowEntry,
    ExternalFnDef, FrameShapeDef, GlobalVarDef, LineVariantGroup, ListDef, ListItemDef,
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
    /// The M-3 `AliasTable` (`docs/modules-spec.md` §5, format section tag
    /// `0x0F`): old→new `DefinitionId` rename records emitted from
    /// `#@was(old_name)` directives on modules and definitions. Sorted by
    /// `old` for the runtime's binary-search miss-path lookup. Empty for
    /// every story that uses no `#@was` — including the entire pre-M-3
    /// corpus and converter output.
    pub alias_table: Vec<AliasEntry>,
    /// The T2-3 `EffectRows` table (`docs/effects-spec.md` §11, format section
    /// tag `0x0D`): one factored effect row per knot/stitch — the host's
    /// resume-scheduling estimate (§12.1). **Additive metadata**: the runtime
    /// does not consume rows yet (`sleep`/narrowing are the future clients), so
    /// a story that carries rows runs byte-identically to one that does not.
    /// Empty for converter output and any story compiled before this slice.
    /// Sorted by `def` (ascending raw id) for determinism.
    pub effect_rows: Vec<EffectRowEntry>,
    /// The FS-3 `FrameShapes` table (`docs/flow-suspension-spec.md` §4/§11,
    /// `.inkb` section tag `0x10`): one [`FrameShapeDef`] per `await` site —
    /// the name-keyed static description of which locals cross the park, so
    /// the runtime knows what to spill/restore around a suspension. Sorted by
    /// `site` (ascending raw id) for determinism.
    ///
    /// **Reserved-through-fence**: additive metadata the runtime does not
    /// consume yet, and — because the E052 `await` lowering fence stands
    /// (FS-3c) — never populated by compilation today. Empty for every story
    /// compiled today and all converter output, so a story carrying frame
    /// shapes runs byte-identically to one without. First emission rides the
    /// continuation-splitting codegen when the fence drops (FS-3r). The
    /// section is **omitted entirely** from `.inkb` when empty (self-framed in
    /// the offset table, like `Visibility`), so existing stories stay
    /// byte-identical.
    pub frame_shapes: Vec<FrameShapeDef>,
    /// D6 `DebugInfo` (`docs/debugger-spec.md` §2, `.inkb` tag `0x11`):
    /// bytecode-offset → source-range map. `None` when debug info was not
    /// requested at compile time — the ship-policy default (§1.2): a
    /// release-exported story never carries this, so the section is
    /// omitted entirely from `.inkb` and every existing byte stays
    /// identical to before this field existed. `Some` only for a dev/studio
    /// compile or an explicit CLI debug flag.
    pub debug_info: Option<DebugInfoSection>,
    /// Line-variant groups (stage 1 of the shared-alternatives track,
    /// issue #3273): records tying runs of consecutive line-table entries
    /// back to one authored line whose inline alternatives were enumerated
    /// at recognition time. Empty until the stage-2 flip routes lines here;
    /// the `.inkb` section (tag `0x12`) is **omitted entirely when empty**,
    /// so every story without variant groups stays byte-identical.
    pub line_variant_groups: Vec<LineVariantGroup>,
    /// CRC-32 checksum from the `.inkb` header, used for locale validation.
    /// Zero for stories not loaded from `.inkb`.
    pub source_checksum: u32,
}
