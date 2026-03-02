//! Binary interface between the brink compiler and runtime.
//!
//! This crate defines the types shared across the compiler/runtime boundary:
//! `DefinitionId`, opcodes, value types, line templates, and the top-level
//! `StoryData` container.
//!
//! `brink-runtime` depends ONLY on this crate — nothing else from brink.

mod codec;
mod counting;
mod definition;
mod id;
mod inkb;
mod line;
mod opcode;
mod story;
mod value;

#[cfg(feature = "inkt")]
mod inkt;

pub use counting::CountingFlags;
pub use definition::{
    ContainerDef, ContainerLineTable, ExternalFnDef, GlobalVarDef, LineEntry, ListDef, ListItemDef,
};
pub use id::{DefinitionId, DefinitionTag, LineId, NameId};
pub use inkb::{
    InkbIndex, SectionEntry, SectionKind, assemble_inkb, read_inkb, read_inkb_index,
    read_section_containers, read_section_externals, read_section_line_tables,
    read_section_list_defs, read_section_list_items, read_section_name_table,
    read_section_variables, write_inkb, write_section_containers, write_section_externals,
    write_section_line_tables, write_section_list_defs, write_section_list_items,
    write_section_name_table, write_section_variables,
};
pub use line::{LineContent, LinePart, LineTemplate, PluralCategory, PluralResolver, SelectKey};
pub use opcode::{ChoiceFlags, DecodeError, Opcode, SequenceKind};
pub use story::StoryData;
pub use value::{ListValue, Value, ValueType};

#[cfg(feature = "inkt")]
pub use inkt::{InktParseError, read_inkt, write_inkt};
