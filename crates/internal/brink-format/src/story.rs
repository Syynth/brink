use crate::definition::{
    AddressDef, ContainerDef, ExternalFnDef, GlobalVarDef, ListDef, ListItemDef, ScopeLineTable,
};
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
    /// Interned name strings, indexed by [`NameId`](crate::id::NameId).
    pub name_table: Vec<String>,
    /// List literal values referenced by `PushList(idx)` opcodes.
    pub list_literals: Vec<ListValue>,
}
