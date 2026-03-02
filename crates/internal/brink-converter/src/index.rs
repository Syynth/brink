use std::collections::HashMap;

use brink_format::DefinitionId;
use brink_json::{Container, Divert, Element, InkJson, VariableAssignment};

use crate::error::ConvertError;
use crate::path;

/// The result of Pass 1: a registry of all named definitions in the story.
pub struct StoryIndex {
    /// ink.json container path → `DefinitionId`
    pub containers: HashMap<String, DefinitionId>,
    /// Global variable name → `DefinitionId`
    pub globals: HashMap<String, DefinitionId>,
    /// External function name → `(DefinitionId, arg_count)`
    pub externals: HashMap<String, (DefinitionId, u8)>,
    /// List definition name → `DefinitionId`
    pub list_defs: HashMap<String, DefinitionId>,
    /// `"ListName.ItemName"` → `(DefinitionId, ordinal)`
    pub list_items: HashMap<String, (DefinitionId, i32)>,
}

impl StoryIndex {
    /// Look up a container by path. If the exact path isn't registered (e.g. the
    /// path points to a non-container index within a container), fall back to the
    /// nearest ancestor container.
    pub fn resolve_container(&self, path: &str) -> Option<DefinitionId> {
        if let Some(&id) = self.containers.get(path) {
            return Some(id);
        }
        // Progressively strip trailing components to find the nearest container
        let mut p = path;
        while let Some(dot) = p.rfind('.') {
            p = &p[..dot];
            if let Some(&id) = self.containers.get(p) {
                return Some(id);
            }
        }
        // Try root
        self.containers.get("").copied()
    }
}

/// Build a `StoryIndex` from a parsed `InkJson`.
pub fn build_index(story: &InkJson) -> Result<StoryIndex, ConvertError> {
    let mut index = StoryIndex {
        containers: HashMap::new(),
        globals: HashMap::new(),
        externals: HashMap::new(),
        list_defs: HashMap::new(),
        list_items: HashMap::new(),
    };

    // 1. Walk containers recursively
    register_container(&mut index, &story.root, "")?;

    // 2. Process list definitions
    for (list_name, items) in &story.list_defs {
        let list_id = path::list_def_id(list_name);
        index.list_defs.insert(list_name.clone(), list_id);

        for (item_name, &ordinal) in items {
            let qualified = format!("{list_name}.{item_name}");
            let item_id = path::list_item_id(&qualified);
            #[expect(clippy::cast_possible_truncation)]
            let ordinal_i32 = ordinal as i32;
            index.list_items.insert(qualified, (item_id, ordinal_i32));
        }
    }

    // 3. Scan root's named content for global variable declarations
    if let Some(Element::Container(global_decl)) = story.root.named_content.get("global decl") {
        scan_global_decls(&mut index, global_decl)?;
    }

    // 4. Scan all containers for external function diverts
    scan_externals(&mut index, &story.root);

    Ok(index)
}

/// Recursively register a container and all its children.
fn register_container(
    index: &mut StoryIndex,
    container: &Container,
    current_path: &str,
) -> Result<(), ConvertError> {
    let id = path::container_id(current_path);
    index.containers.insert(current_path.to_string(), id);

    // Walk contents — sub-containers at index i get path "parent.i".
    // If a container also has a `#n` name, register it under "parent.name" too.
    for (i, element) in container.contents.iter().enumerate() {
        if let Element::Container(child) = element {
            let child_path = if current_path.is_empty() {
                i.to_string()
            } else {
                format!("{current_path}.{i}")
            };
            register_container(index, child, &child_path)?;

            // Also register under the container's own name if it has one.
            // Reuse the indexed container's ID so that both paths resolve to
            // the same container def at link time.
            if let Some(name) = &child.name {
                let named_path = if current_path.is_empty() {
                    name.clone()
                } else {
                    format!("{current_path}.{name}")
                };
                let child_id = path::container_id(&child_path);
                index.containers.insert(named_path, child_id);
            }
        }
    }

    // Walk named content — named entries get path "parent.name"
    for (name, element) in &container.named_content {
        if let Element::Container(child) = element {
            let child_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{current_path}.{name}")
            };
            register_container(index, child, &child_path)?;
        }
    }

    Ok(())
}

/// Walk the "global decl" container to find variable assignments and register
/// global variable ids.
fn scan_global_decls(index: &mut StoryIndex, container: &Container) -> Result<(), ConvertError> {
    for element in &container.contents {
        if let Element::VariableAssignment(
            VariableAssignment::GlobalAssignment { variable }
            | VariableAssignment::TemporaryAssignment {
                variable,
                reassign: false,
            },
        ) = element
        {
            // In global decl, temp= with reassign=false is an initial global declaration
            let id = path::global_var_id(variable);
            index.globals.insert(variable.clone(), id);
        }
    }

    // Also scan named content for nested global decl containers
    for element in container.named_content.values() {
        if let Element::Container(child) = element {
            scan_global_decls(index, child)?;
        }
    }

    Ok(())
}

/// Recursively scan for `ExternalFunction` diverts and register them.
fn scan_externals(index: &mut StoryIndex, container: &Container) {
    for element in &container.contents {
        match element {
            Element::Divert(Divert::ExternalFunction {
                name, arg_count, ..
            }) => {
                #[expect(clippy::cast_possible_truncation)]
                let argc = *arg_count as u8;
                let id = path::external_fn_id(name);
                index.externals.insert(name.clone(), (id, argc));
            }
            Element::Container(child) => scan_externals(index, child),
            _ => {}
        }
    }

    for element in container.named_content.values() {
        if let Element::Container(child) = element {
            scan_externals(index, child);
        }
    }
}
