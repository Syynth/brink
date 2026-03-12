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
    /// Resolved path → intra-container address `DefinitionId` (for index-based divert targets)
    pub intra_addresses: HashMap<String, DefinitionId>,
}

impl StoryIndex {
    /// Look up a container by path. If the exact path isn't registered (e.g. the
    /// path points to a non-container index within a container), fall back to the
    /// nearest ancestor container.
    pub fn resolve_container(&self, path: &str) -> Option<DefinitionId> {
        self.resolve_container_with_index(path).map(|(id, _)| id)
    }

    /// Resolve a path, returning the container ID and an optional element index.
    ///
    /// When the path points to a specific element index within a container
    /// (e.g. `"foo.bar.15"` where `15` is not itself a container), returns
    /// `(container_id_for_foo_bar, Some(15))`.
    ///
    /// When the path directly names a container, returns `(container_id, None)`.
    pub fn resolve_container_with_index(
        &self,
        path: &str,
    ) -> Option<(DefinitionId, Option<usize>)> {
        if let Some(&id) = self.containers.get(path) {
            return Some((id, None));
        }
        // Progressively strip trailing components to find the nearest container.
        // If the first stripped component is numeric, it's an element index.
        let mut p = path;
        let mut stripped_suffix: Option<&str> = None;
        while let Some(dot) = p.rfind('.') {
            if stripped_suffix.is_none() {
                stripped_suffix = Some(&p[dot + 1..]);
            }
            p = &p[..dot];
            if let Some(&id) = self.containers.get(p) {
                // Check if the first stripped component was a numeric index.
                let element_index = stripped_suffix.and_then(|s| s.parse::<usize>().ok());
                return Some((id, element_index));
            }
        }
        // Try root
        self.containers.get("").map(|&id| (id, None))
    }

    /// Resolve a divert target path, returning a `DefinitionId` for either
    /// a container or a label.
    ///
    /// If the path points directly to a container, returns the container's ID.
    /// If the path points to an element index within a container, returns a
    /// label ID (registering the label if needed).
    pub fn resolve_target(&mut self, path: &str) -> Option<DefinitionId> {
        match self.resolve_container_with_index(path) {
            Some((container_id, None)) => Some(container_id),
            Some((_container_id, Some(_index))) => {
                // This is an index target — return or register an intra-container address.
                if let Some(&addr_id) = self.intra_addresses.get(path) {
                    Some(addr_id)
                } else {
                    let id = crate::path::intra_address_id(path);
                    self.intra_addresses.insert(path.to_string(), id);
                    Some(id)
                }
            }
            None => None,
        }
    }
}

/// Build a `StoryIndex` from a parsed `InkJson`.
///
/// Uses `canonical_root` (the preprocessed root with canonicalized paths
/// and $r elements removed) for container registration, label scanning,
/// and external function scanning. Uses `story` for list definitions.
pub fn build_index(
    story: &InkJson,
    canonical_root: &Container,
) -> Result<StoryIndex, ConvertError> {
    let mut index = StoryIndex {
        containers: HashMap::new(),
        globals: HashMap::new(),
        externals: HashMap::new(),
        list_defs: HashMap::new(),
        list_items: HashMap::new(),
        intra_addresses: HashMap::new(),
    };

    // 1. Walk containers recursively (using canonical root)
    register_container(&mut index, canonical_root, "")?;

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
    if let Some(Element::Container(global_decl)) = canonical_root.named_content.get("global decl") {
        scan_global_decls(&mut index, global_decl)?;
    }

    // 4. Scan all containers for external function diverts
    scan_externals(&mut index, canonical_root);

    // 5. Scan all divert targets and register addresses for index-based targets
    register_addresses(&mut index, canonical_root, "");

    Ok(index)
}

/// Recursively register a container and all its children.
fn register_container(
    index: &mut StoryIndex,
    container: &Container,
    current_path: &str,
) -> Result<(), ConvertError> {
    let id = path::address_id(current_path);
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
                let child_id = path::address_id(&child_path);
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
            VariableAssignment::GlobalAssignment { variable, .. }
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

/// Recursively scan all divert targets and register labels for paths that
/// point to a specific element index within a container.
fn register_addresses(index: &mut StoryIndex, container: &Container, current_path: &str) {
    for (i, element) in container.contents.iter().enumerate() {
        match element {
            Element::Divert(divert) => {
                let paths: Vec<&str> = match divert {
                    Divert::Target { path, .. }
                    | Divert::Function { path, .. }
                    | Divert::Tunnel { path, .. } => vec![path.as_str()],
                    Divert::Variable { .. }
                    | Divert::FunctionVariable { .. }
                    | Divert::TunnelVariable { .. }
                    | Divert::ExternalFunction { .. } => vec![],
                };
                for ink_path in paths {
                    let resolved = path::resolve_path(current_path, ink_path);
                    index.resolve_target(&resolved);
                }
            }
            Element::ChoicePoint(cp) => {
                let resolved = path::resolve_path(current_path, &cp.target);
                index.resolve_target(&resolved);
            }
            Element::Value(brink_json::InkValue::DivertTarget(p)) => {
                let resolved = path::resolve_path(current_path, p);
                index.resolve_target(&resolved);
            }
            Element::Container(child) => {
                let child_path = if current_path.is_empty() {
                    i.to_string()
                } else {
                    format!("{current_path}.{i}")
                };
                register_addresses(index, child, &child_path);
            }
            _ => {}
        }
    }

    // Walk named content
    for (name, element) in &container.named_content {
        if let Element::Container(child) = element {
            let child_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{current_path}.{name}")
            };
            register_addresses(index, child, &child_path);
        }
    }
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
