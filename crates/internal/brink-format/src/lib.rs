//! Binary interface between the brink compiler and runtime.
//!
//! This crate defines the types shared across the compiler/runtime boundary:
//! `DefinitionId`, opcodes, value types, line templates, and the top-level
//! `StoryData` container.
//!
//! `brink-runtime` depends ONLY on this crate — nothing else from brink.
//!
//! `no_std` + `alloc`: this crate builds without the standard library when
//! the default `std` feature is disabled (see `docs/no-std-portability.md`).
//! `inkt`/`inkt-write` (the `.inkt` text format, used by the intl pipeline)
//! are not part of the `no_std` surface — `pest` is std-oriented.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod codec;
mod conventions;
mod counting;
mod definition;
mod id;
mod inkb;
mod inkl;
mod line;
pub mod manifest_field_names;
mod opcode;
mod save;
mod story;
mod value;

#[cfg(any(feature = "inkt", feature = "inkt-write"))]
mod inkt;

pub use conventions::{
    CONVENTIONS_PROJECTION_WIRE_VERSION, ConventionAttachDef, ConventionAttachFieldDef,
    ConventionEntryDef, ConventionModeDef, ConventionsProjectionDef, SchemaTypeDef,
    read_conventions_projection, write_conventions_projection,
};
pub use counting::CountingFlags;
pub use definition::{
    AddressDef, AddressPath, AliasEntry, CallAtom, CapabilityParam, ContainerDef,
    DEBUG_FLAG_IS_STMT, DEBUG_FLAG_PROLOGUE_END, DEBUG_FLAG_RESERVED_MASK, DebugContainerTable,
    DebugEntry, DebugFileEntry, DebugInfoSection, DebugLocalEntry, DirectEffects, DispatchEntry,
    EffectRowEntry, ExternalFnDef, FileSurface, FrameShapeDef, GlobalVarDef, LineEntry,
    LineVariantGroup, ListDef, ListItemDef, LocaleData, LocaleLineEntry, LocaleScopeTable,
    ParamMeta, ScopeLineTable, SlotInfo, SourceLocation, StructShapeDef, content_hash,
};
pub use id::{DefinitionId, DefinitionTag, LineId, NameId};
pub use inkb::{
    InkbIndex, SectionEntry, SectionKind, assemble_inkb, read_inkb, read_inkb_index,
    read_section_address_paths, read_section_addresses, read_section_alias_table,
    read_section_containers, read_section_debug_info, read_section_effect_rows,
    read_section_externals, read_section_frame_shapes, read_section_line_tables,
    read_section_list_defs, read_section_list_items, read_section_list_literals,
    read_section_literal_pool, read_section_name_table, read_section_struct_shapes,
    read_section_variables, read_section_visibility, write_inkb, write_section_address_paths,
    write_section_addresses, write_section_alias_table, write_section_containers,
    write_section_debug_info, write_section_effect_rows, write_section_externals,
    write_section_frame_shapes, write_section_line_tables, write_section_list_defs,
    write_section_list_items, write_section_list_literals, write_section_literal_pool,
    write_section_name_table, write_section_struct_shapes, write_section_variables,
    write_section_visibility,
};
pub use inkl::{read_inkl, write_inkl};
pub use line::{
    LineContent, LineFlags, LinePart, LineTemplate, PluralCategory, PluralResolver, SelectKey,
};
pub use opcode::{ChoiceFlags, CollectOp, DecodeError, Opcode, SeqVerbOp, SequenceKind, TowerOp};
pub use save::{
    LoadReport, SAVE_FORMAT_VERSION, SUSPENDED_FLOW_SECTION_VERSION, SaveState, SuspendedFlow,
    VisitEntry, WakePolicy, WakeSource,
};
pub use story::StoryData;
pub use value::{
    ClosureEnvEntry, ClosureValue, ListValue, MAX_DECODE_DEPTH, MapKey, OrderedMap, ProjSegment,
    ProjectionValue, ShapeId, Value, ValueType, WeightedValue,
};

#[cfg(any(feature = "inkt", feature = "inkt-write"))]
pub use inkt::write_inkt;
#[cfg(feature = "inkt")]
pub use inkt::{InktParseError, read_inkt};
