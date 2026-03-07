//! JSON backend: LIR → `brink_json::InkJson`.
//!
//! Emits the same ink.json format that inklecate produces, enabling
//! diff-based validation against the reference compiler.

mod emit;

use std::collections::HashMap;

use brink_format::DefinitionId;
use brink_ir::lir;
use brink_json::{
    Container, ContainerFlags, ControlCommand, Element, InkJson, InkValue, VariableAssignment,
};

/// Emit an `InkJson` from a resolved LIR `Program`.
pub fn emit(program: &lir::Program) -> InkJson {
    let lookups = Lookups::build(program);

    // Build the root container with inklecate's special wrapping:
    //   root = [ inner_container, "done", { knots + global_decl } ]
    let root = build_root(&program.root, program, &lookups);

    // Build list definitions
    let list_defs = build_list_defs(program, &lookups);

    InkJson {
        ink_version: 21,
        root,
        list_defs,
    }
}

// ─── Container tree emission ────────────────────────────────────────

/// Build the root container with inklecate's special wrapping.
///
/// In inklecate's format, the root serializes as:
/// ```json
/// [ inner_container, "done", { knots_and_metadata } ]
/// ```
/// - `inner_container`: anonymous container with root body + non-knot children
///   (gathers, choice targets) as inline indexed elements
/// - `"done"`: root-level done
/// - metadata object: knots as named content, plus `"global decl"` if present
fn build_root(root: &lir::Container, program: &lir::Program, lookups: &Lookups) -> Container {
    let cctx = emit::ContainerCtx::build_from_tree(root, lookups, "");

    // Emit body elements for the inner container
    let (mut inner_contents, inner_named) = emit::emit_body(&root.body, lookups, &cctx);

    // Partition children: knots go to root named_content, everything else
    // (gathers, choice targets) goes to the inner container's named_content
    let mut inner_named_content = inner_named;
    let mut root_named_content: HashMap<String, Element> = HashMap::new();

    for child in &root.children {
        let child_name = child.name.as_deref().unwrap_or("_anon");
        let child_container = build_container(child, child_name, lookups);

        match child.kind {
            lir::ContainerKind::Knot => {
                root_named_content
                    .insert(child_name.to_string(), Element::Container(child_container));
            }
            _ => {
                inner_named_content
                    .insert(child_name.to_string(), Element::Container(child_container));
            }
        }
    }

    // Inklecate wraps the trailing "done" in the inner container inside a
    // named "g-0" gather container. Remove the trailing done from the body
    // (if present) and always append a g-0 wrapper.
    let trailing_done = matches!(
        inner_contents.last(),
        Some(Element::ControlCommand(ControlCommand::Done))
    );
    if trailing_done {
        inner_contents.pop();
    }
    inner_contents.push(Element::Container(Container {
        flags: None,
        name: Some("g-0".to_string()),
        named_content: HashMap::new(),
        contents: vec![Element::ControlCommand(ControlCommand::Done)],
    }));

    let inner = Container {
        flags: None,
        name: None,
        named_content: inner_named_content,
        contents: inner_contents,
    };

    // Build root contents: [inner_container, "done"]
    let root_contents = vec![
        Element::Container(inner),
        Element::ControlCommand(ControlCommand::Done),
    ];

    // Add global declarations container to root named_content
    let global_decl = build_global_decl_container(program, lookups);
    if !global_decl.contents.is_empty() {
        root_named_content.insert("global decl".to_string(), Element::Container(global_decl));
    }

    Container {
        flags: None,
        name: None,
        named_content: root_named_content,
        contents: root_contents,
    }
}

fn build_container(container: &lir::Container, path: &str, lookups: &Lookups) -> Container {
    let cctx = emit::ContainerCtx::build_from_tree(container, lookups, path);

    // Emit body elements
    let (contents, mut named_content) = emit::emit_body(&container.body, lookups, &cctx);

    // Recursively build child containers and add to named_content
    for child in &container.children {
        let child_name = child.name.as_deref().unwrap_or("_anon");
        let child_path = if path.is_empty() {
            child_name.to_string()
        } else {
            format!("{path}.{child_name}")
        };
        let child_container = build_container(child, &child_path, lookups);
        named_content.insert(child_name.to_string(), Element::Container(child_container));
    }

    // Convert counting flags
    let flags = convert_counting_flags(container.counting_flags);

    Container {
        flags: if flags.is_empty() { None } else { Some(flags) },
        name: None,
        named_content,
        contents,
    }
}

fn convert_counting_flags(flags: brink_format::CountingFlags) -> ContainerFlags {
    let mut out = ContainerFlags::empty();
    if flags.contains(brink_format::CountingFlags::VISITS) {
        out |= ContainerFlags::VISITS;
    }
    if flags.contains(brink_format::CountingFlags::TURNS) {
        out |= ContainerFlags::TURNS;
    }
    if flags.contains(brink_format::CountingFlags::COUNT_START_ONLY) {
        out |= ContainerFlags::COUNT_START_ONLY;
    }
    out
}

// ─── Lookup tables ──────────────────────────────────────────────────

/// Precomputed lookup tables for the emission pass.
pub struct Lookups {
    /// `DefinitionId` → container path string.
    container_paths: HashMap<DefinitionId, String>,
    /// `DefinitionId` → global/const/list variable name.
    global_names: HashMap<DefinitionId, String>,
    /// `DefinitionId` → `("ListName.ItemName", ordinal)`.
    list_item_info: HashMap<DefinitionId, (String, i32)>,
    /// `DefinitionId` → list name.
    list_names: HashMap<DefinitionId, String>,
    /// Name table from the program.
    name_table: Vec<String>,
}

impl Lookups {
    fn build(program: &lir::Program) -> Self {
        // Walk the container tree to build id→path map
        let mut container_paths = HashMap::new();
        collect_container_paths(&program.root, "", &mut container_paths);

        let mut global_names = HashMap::new();
        for g in &program.globals {
            global_names.insert(g.id, program.name_table[g.name.0 as usize].clone());
        }

        let mut list_names = HashMap::new();
        let mut list_item_info = HashMap::new();
        for list in &program.lists {
            let list_name = &program.name_table[list.name.0 as usize];
            list_names.insert(list.id, list_name.clone());
        }
        for item in &program.list_items {
            let item_name = &program.name_table[item.name.0 as usize];
            if let Some(origin_name) = list_names.get(&item.origin) {
                let qualified = format!("{origin_name}.{item_name}");
                list_item_info.insert(item.id, (qualified, item.ordinal));
            }
        }

        for ext in &program.externals {
            let name = program.name_table[ext.name.0 as usize].clone();
            global_names.insert(ext.id, name);
        }

        Lookups {
            container_paths,
            global_names,
            list_item_info,
            list_names,
            name_table: program.name_table.clone(),
        }
    }

    pub fn container_path(&self, id: DefinitionId) -> String {
        self.container_paths.get(&id).cloned().unwrap_or_default()
    }

    pub fn global_name(&self, id: DefinitionId) -> String {
        self.global_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "_unknown".to_string())
    }

    pub fn name(&self, id: brink_format::NameId) -> &str {
        &self.name_table[id.0 as usize]
    }

    pub fn list_item_info(&self, id: DefinitionId) -> Option<(String, i32)> {
        self.list_item_info.get(&id).cloned()
    }

    pub fn list_name(&self, id: DefinitionId) -> Option<String> {
        self.list_names.get(&id).cloned()
    }
}

/// Recursively walk the container tree to build `DefinitionId → path` map.
fn collect_container_paths(
    container: &lir::Container,
    path: &str,
    out: &mut HashMap<DefinitionId, String>,
) {
    out.insert(container.id, path.to_string());
    for child in &container.children {
        let child_name = child.name.as_deref().unwrap_or("_anon");
        let child_path = if path.is_empty() {
            child_name.to_string()
        } else {
            format!("{path}.{child_name}")
        };
        collect_container_paths(child, &child_path, out);
    }
}

// ─── Global declarations ────────────────────────────────────────────

fn build_global_decl_container(program: &lir::Program, lookups: &Lookups) -> Container {
    let mut contents = Vec::new();

    for global in &program.globals {
        let name = lookups.global_name(global.id);

        contents.push(Element::ControlCommand(ControlCommand::BeginLogicalEval));
        emit_const_value(&global.default, lookups, &mut contents);
        contents.push(Element::ControlCommand(ControlCommand::EndLogicalEval));
        contents.push(Element::VariableAssignment(
            VariableAssignment::GlobalAssignment { variable: name },
        ));
    }

    if !contents.is_empty() {
        contents.push(Element::ControlCommand(ControlCommand::End));
    }

    Container {
        flags: None,
        name: None,
        named_content: HashMap::new(),
        contents,
    }
}

#[expect(clippy::cast_lossless)]
fn emit_const_value(value: &lir::ConstValue, lookups: &Lookups, out: &mut Vec<Element>) {
    match value {
        lir::ConstValue::Int(n) => out.push(Element::Value(InkValue::Integer(*n as i64))),
        lir::ConstValue::Float(f) => out.push(Element::Value(InkValue::Float(*f as f64))),
        lir::ConstValue::Bool(b) => out.push(Element::Value(InkValue::Bool(*b))),
        lir::ConstValue::String(s) => out.push(Element::Value(InkValue::String(s.clone()))),
        lir::ConstValue::Null => out.push(Element::Void),
        lir::ConstValue::DivertTarget(id) => {
            let path = lookups.container_path(*id);
            out.push(Element::Value(InkValue::DivertTarget(path)));
        }
        lir::ConstValue::List { items, origins } => {
            let mut map = std::collections::HashMap::new();
            for &item_id in items {
                if let Some((qualified, ordinal)) = lookups.list_item_info(item_id) {
                    map.insert(qualified, ordinal as i64);
                }
            }
            let origin_names: Vec<String> = origins
                .iter()
                .filter_map(|&id| lookups.list_name(id))
                .collect();
            out.push(Element::Value(InkValue::List(brink_json::InkList {
                items: map,
                origins: origin_names,
            })));
        }
    }
}

// ─── List definitions ───────────────────────────────────────────────

fn build_list_defs(
    program: &lir::Program,
    lookups: &Lookups,
) -> HashMap<String, HashMap<String, i64>> {
    let mut defs = HashMap::new();

    for list in &program.lists {
        let list_name = lookups.name(list.name).to_string();
        let mut items = HashMap::new();
        for &(item_name_id, ordinal) in &list.items {
            let item_name = lookups.name(item_name_id).to_string();
            items.insert(item_name, i64::from(ordinal));
        }
        defs.insert(list_name, items);
    }

    defs
}
