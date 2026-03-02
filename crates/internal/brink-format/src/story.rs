use crate::definition::{ContainerDef, ExternalFnDef, GlobalVarDef, ListDef, ListItemDef};

/// The top-level compiled story: everything the runtime needs to execute.
#[derive(Debug, Clone, PartialEq)]
pub struct StoryData {
    pub containers: Vec<ContainerDef>,
    pub variables: Vec<GlobalVarDef>,
    pub list_defs: Vec<ListDef>,
    pub list_items: Vec<ListItemDef>,
    pub externals: Vec<ExternalFnDef>,
    /// Interned name strings, indexed by [`NameId`](crate::id::NameId).
    pub name_table: Vec<String>,
}
