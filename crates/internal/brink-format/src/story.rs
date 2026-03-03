use crate::definition::{
    ContainerDef, ContainerLineTable, ExternalFnDef, GlobalVarDef, LabelDef, ListDef, ListItemDef,
};
use crate::value::ListValue;

/// The top-level compiled story: everything the runtime needs to execute.
#[derive(Debug, Clone, PartialEq)]
pub struct StoryData {
    pub containers: Vec<ContainerDef>,
    /// Per-container line tables, parallel to `containers`.
    pub line_tables: Vec<ContainerLineTable>,
    pub variables: Vec<GlobalVarDef>,
    pub list_defs: Vec<ListDef>,
    pub list_items: Vec<ListItemDef>,
    pub externals: Vec<ExternalFnDef>,
    /// Labels pointing to byte offsets within containers.
    pub labels: Vec<LabelDef>,
    /// Interned name strings, indexed by [`NameId`](crate::id::NameId).
    pub name_table: Vec<String>,
    /// List literal values referenced by `PushList(idx)` opcodes.
    pub list_literals: Vec<ListValue>,
}
