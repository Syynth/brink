//! Bytecode backend: LIR → `StoryData`.

mod container;
mod content;
mod expr;

use std::collections::HashMap;

use brink_format::{
    ContainerDef, ContainerLineTable, ExternalFnDef, GlobalVarDef, LineContent, LineEntry, ListDef,
    ListItemDef, ListValue, NameId, Opcode, StoryData, Value,
};
use brink_ir::lir;

/// Compile a resolved LIR `Program` into `StoryData` for the runtime.
pub fn emit(program: &lir::Program) -> StoryData {
    let mut state = EmitState {
        containers: Vec::new(),
        line_tables: Vec::new(),
        list_literals: Vec::new(),
        name_table: program.name_table.clone(),
        name_index: HashMap::new(),
    };

    // Build the name index from the existing name table for dedup.
    for (i, name) in state.name_table.iter().enumerate() {
        #[expect(clippy::cast_possible_truncation)]
        state.name_index.insert(name.clone(), NameId(i as u16));
    }

    // Walk the container tree depth-first.
    walk_container(&program.root, "", &mut state);

    // Build globals, lists, externals.
    let variables = build_globals(&program.globals);
    let list_defs = build_list_defs(&program.lists);
    let list_items = build_list_items(&program.list_items);
    let externals = build_externals(&program.externals);

    StoryData {
        containers: state.containers,
        line_tables: state.line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        labels: Vec::new(),
        name_table: state.name_table,
        list_literals: state.list_literals,
    }
}

// ─── Emission state ─────────────────────────────────────────────────

struct EmitState {
    containers: Vec<ContainerDef>,
    line_tables: Vec<ContainerLineTable>,
    list_literals: Vec<ListValue>,
    name_table: Vec<String>,
    name_index: HashMap<String, NameId>,
}

// ─── Container emitter ──────────────────────────────────────────────

struct ContainerEmitter<'a> {
    bytecode: Vec<u8>,
    line_table: Vec<LineEntry>,
    list_literals: &'a mut Vec<ListValue>,
    state_name_table: &'a mut Vec<String>,
    state_name_index: &'a mut HashMap<String, NameId>,
    in_conditional_branch: bool,
}

impl<'a> ContainerEmitter<'a> {
    fn new(state: &'a mut EmitState) -> Self {
        Self {
            bytecode: Vec::new(),
            line_table: Vec::new(),
            list_literals: &mut state.list_literals,
            state_name_table: &mut state.name_table,
            state_name_index: &mut state.name_index,
            in_conditional_branch: false,
        }
    }

    #[expect(clippy::needless_pass_by_value)]
    fn emit(&mut self, op: Opcode) {
        op.encode(&mut self.bytecode);
    }

    #[expect(clippy::cast_possible_truncation)]
    fn add_line(&mut self, text: &str) -> u16 {
        let idx = self.line_table.len() as u16;
        self.line_table.push(LineEntry {
            content: LineContent::Plain(text.to_string()),
            source_hash: 0,
        });
        idx
    }

    fn intern_string(&mut self, s: &str) -> NameId {
        if let Some(&id) = self.state_name_index.get(s) {
            return id;
        }
        #[expect(clippy::cast_possible_truncation)]
        let id = NameId(self.state_name_table.len() as u16);
        self.state_name_table.push(s.to_string());
        self.state_name_index.insert(s.to_string(), id);
        id
    }

    /// Emit a jump-like instruction with a placeholder offset.
    /// Returns the byte position of the i32 offset field for later patching.
    #[expect(clippy::needless_pass_by_value)]
    fn emit_jump_placeholder(&mut self, op: Opcode) -> usize {
        op.encode(&mut self.bytecode);
        // The i32 offset occupies the last 4 bytes of the encoded instruction.
        self.bytecode.len() - 4
    }

    /// Patch a previously emitted jump offset to point to the current position.
    /// The offset is relative: bytes from end of the jump instruction to current pos.
    fn patch_jump(&mut self, offset_pos: usize) {
        let target = self.bytecode.len();
        // The jump instruction ends right after the i32 field (offset_pos + 4).
        let instruction_end = offset_pos + 4;
        #[expect(clippy::cast_possible_wrap)]
        #[expect(clippy::cast_possible_truncation)]
        let relative = (target - instruction_end) as i32;
        let bytes = relative.to_le_bytes();
        self.bytecode[offset_pos..offset_pos + 4].copy_from_slice(&bytes);
    }
}

// ─── Container tree walk ────────────────────────────────────────────

fn walk_container(container: &lir::Container, path: &str, state: &mut EmitState) {
    // Emit this container's bytecode.
    let mut emitter = ContainerEmitter::new(state);

    // Branch containers (conditional or sequence) suppress `Done` after
    // ChoiceSets. Choices inside branches form part of a larger logical
    // ChoiceSet in the parent — the runtime auto-presents pending choices
    // on frame/container exhaustion (no explicit Done needed).
    if container.kind == lir::ContainerKind::ConditionalBranch
        || container.kind == lir::ContainerKind::SequenceBranch
    {
        emitter.in_conditional_branch = true;
    }

    // Emit DeclareTemp for each parameter (pops args from eval stack into
    // temp slots). Reverse order: caller pushes first arg first, so last
    // arg is on top of the stack and gets popped first.
    for param in container.params.iter().rev() {
        emitter.emit(Opcode::DeclareTemp(param.slot));
    }

    emitter.emit_body(&container.body);

    let path_hash: i32 = path.chars().map(|c| c as i32).sum();

    let def = ContainerDef {
        id: container.id,
        bytecode: emitter.bytecode,
        content_hash: 0,
        counting_flags: container.counting_flags,
        path_hash,
    };
    let lt = ContainerLineTable {
        container_id: container.id,
        lines: emitter.line_table,
    };

    state.containers.push(def);
    state.line_tables.push(lt);

    // Recurse into children.
    for child in &container.children {
        let child_name = child.name.as_deref().unwrap_or("_anon");
        let child_path = if path.is_empty() {
            child_name.to_string()
        } else {
            format!("{path}.{child_name}")
        };
        walk_container(child, &child_path, state);
    }
}

// ─── Top-level definition builders ─────────────────────────────────

fn build_globals(globals: &[lir::GlobalDef]) -> Vec<GlobalVarDef> {
    globals
        .iter()
        .map(|g| GlobalVarDef {
            id: g.id,
            name: g.name,
            value_type: const_value_type(&g.default),
            default_value: const_to_value(&g.default),
            mutable: g.mutable,
        })
        .collect()
}

fn build_list_defs(lists: &[lir::ListDef]) -> Vec<ListDef> {
    lists
        .iter()
        .map(|l| ListDef {
            id: l.id,
            name: l.name,
            items: l.items.clone(),
        })
        .collect()
}

fn build_list_items(items: &[lir::ListItemDef]) -> Vec<ListItemDef> {
    items
        .iter()
        .map(|i| ListItemDef {
            id: i.id,
            origin: i.origin,
            ordinal: i.ordinal,
            name: i.name,
        })
        .collect()
}

fn build_externals(externals: &[lir::ExternalDef]) -> Vec<ExternalFnDef> {
    externals
        .iter()
        .map(|e| ExternalFnDef {
            id: e.id,
            name: e.name,
            arg_count: e.arg_count,
            fallback: e.fallback,
        })
        .collect()
}

fn const_value_type(v: &lir::ConstValue) -> brink_format::ValueType {
    match v {
        lir::ConstValue::Int(_) => brink_format::ValueType::Int,
        lir::ConstValue::Float(_) => brink_format::ValueType::Float,
        lir::ConstValue::Bool(_) => brink_format::ValueType::Bool,
        lir::ConstValue::String(_) => brink_format::ValueType::String,
        lir::ConstValue::List { .. } => brink_format::ValueType::List,
        lir::ConstValue::DivertTarget(_) => brink_format::ValueType::DivertTarget,
        lir::ConstValue::Null => brink_format::ValueType::Null,
    }
}

fn const_to_value(v: &lir::ConstValue) -> Value {
    match v {
        lir::ConstValue::Int(n) => Value::Int(*n),
        lir::ConstValue::Float(f) => Value::Float(*f),
        lir::ConstValue::Bool(b) => Value::Bool(*b),
        lir::ConstValue::String(s) => Value::String(s.clone().into()),
        lir::ConstValue::Null => Value::Null,
        lir::ConstValue::DivertTarget(id) => Value::DivertTarget(*id),
        lir::ConstValue::List { items, origins } => Value::List(
            ListValue {
                items: items.clone(),
                origins: origins.clone(),
            }
            .into(),
        ),
    }
}
