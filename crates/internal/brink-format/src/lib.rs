//! Binary interface between the brink compiler and runtime.
//!
//! This crate defines the types shared across the compiler/runtime boundary:
//! `DefinitionId`, opcodes, value types, line templates, and the top-level
//! `StoryData` container.
//!
//! `brink-runtime` depends ONLY on this crate — nothing else from brink.

mod counting;
mod definition;
mod id;
mod line;
mod opcode;
mod story;
mod value;

pub use counting::CountingFlags;
pub use definition::{ContainerDef, ExternalFnDef, GlobalVarDef, LineEntry, ListDef, ListItemDef};
pub use id::{DefinitionId, DefinitionTag, LineId, NameId};
pub use line::{LineContent, LinePart, LineTemplate, PluralCategory, PluralResolver, SelectKey};
pub use opcode::{ChoiceFlags, DecodeError, Opcode, SequenceKind};
pub use story::StoryData;
pub use value::{ListValue, Value, ValueType};
