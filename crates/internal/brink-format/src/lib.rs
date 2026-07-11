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
mod counting;
mod definition;
mod id;
mod inkb;
mod inkl;
mod line;
mod opcode;
mod save;
mod story;
mod value;

#[cfg(any(feature = "inkt", feature = "inkt-write"))]
mod inkt;

pub use counting::CountingFlags;
pub use definition::{
    AddressDef, AddressPath, ContainerDef, ExternalFnDef, GlobalVarDef, LineEntry, ListDef,
    ListItemDef, LocaleData, LocaleLineEntry, LocaleScopeTable, ScopeLineTable, SlotInfo,
    SourceLocation, content_hash,
};
pub use id::{DefinitionId, DefinitionTag, LineId, NameId};
pub use inkb::{
    InkbIndex, SectionEntry, SectionKind, assemble_inkb, read_inkb, read_inkb_index,
    read_section_address_paths, read_section_addresses, read_section_containers,
    read_section_externals, read_section_line_tables, read_section_list_defs,
    read_section_list_items, read_section_list_literals, read_section_name_table,
    read_section_variables, write_inkb, write_section_address_paths, write_section_addresses,
    write_section_containers, write_section_externals, write_section_line_tables,
    write_section_list_defs, write_section_list_items, write_section_list_literals,
    write_section_name_table, write_section_variables,
};
pub use inkl::{read_inkl, write_inkl};
pub use line::{
    LineContent, LineFlags, LinePart, LineTemplate, PluralCategory, PluralResolver, SelectKey,
};
pub use opcode::{ChoiceFlags, DecodeError, Opcode, SequenceKind};
pub use save::{LoadReport, SAVE_FORMAT_VERSION, SaveState, VisitEntry};
pub use story::StoryData;
pub use value::{ListValue, MapKey, OrderedMap, Value, ValueType};

#[cfg(any(feature = "inkt", feature = "inkt-write"))]
pub use inkt::write_inkt;
#[cfg(feature = "inkt")]
pub use inkt::{InktParseError, read_inkt};
