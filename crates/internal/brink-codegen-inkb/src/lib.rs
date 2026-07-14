//! Bytecode backend: LIR → `StoryData`.

mod container;
mod content;
mod expr;

use std::collections::HashMap;

use brink_format::{
    AddressDef, AddressPath, ContainerDef, DefinitionId, ExternalFnDef, GlobalVarDef, LineContent,
    LineEntry, ListDef, ListItemDef, ListValue, MapKey, NameId, Opcode, OrderedMap, ScopeLineTable,
    ShapeId, StoryData, StructShapeDef, Value,
};
use brink_ir::lir;

/// A defect in the LIR fed to codegen — an invariant that a well-formed
/// `Program` is guaranteed to satisfy by earlier, non-suppressible compiler
/// stages, which codegen has no independent way to verify structurally
/// beyond this checkpoint. See #586: with #577's `Nop` degradation removed,
/// `container.rs`'s `LogicBreak`/`LogicContinue` handling had zero
/// codegen-level guard against a `loop_stack` that's empty — a future or
/// refactored LIR producer that ever emitted one outside a loop would
/// silently corrupt bytecode via an unpatched `Jump(0)` that looks
/// well-formed, rather than fail. This is the hard error that replaces
/// that silent corruption; today it can only fire on hand-assembled LIR
/// that bypasses `brink-ir::lir::lower` (which rejects this case at E057,
/// non-suppressibly, before a `Program` is ever produced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    message: String,
}

impl CodegenError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Collapse runs of consecutive spaces/tabs within `s` to a single space.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c == ' ' || c == '\t' {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            prev_ws = false;
            out.push(c);
        }
    }
    out
}

/// Compile a resolved LIR `Program` into `StoryData` for the runtime.
///
/// Returns `Err(CodegenError)` only for a defect in the LIR itself — see
/// [`CodegenError`]. A well-formed `Program` (the only kind
/// `brink-ir::lir::lower` ever hands back) always succeeds.
pub fn emit(program: &lir::Program) -> Result<StoryData, CodegenError> {
    let mut state = EmitState {
        containers: Vec::new(),
        addresses: Vec::new(),
        address_paths: Vec::new(),
        scope_line_tables: HashMap::new(),
        list_literals: Vec::new(),
        literal_pool: Vec::new(),
        name_table: program.name_table.clone(),
        name_index: HashMap::new(),
        errors: Vec::new(),
    };

    // Build the name index from the existing name table for dedup.
    for (i, name) in state.name_table.iter().enumerate() {
        #[expect(clippy::cast_possible_truncation)]
        state.name_index.insert(name.clone(), NameId(i as u16));
    }

    // Walk the container tree depth-first.
    // Root is always a scope — its scope_id is its own id; its author scope
    // path is empty.
    walk_container(&program.root, "", "", program.root.id, &mut state);

    if let Some(first) = core::mem::take(&mut state.errors).into_iter().next() {
        return Err(first);
    }

    // Build globals, lists, externals.
    let variables = build_globals(&program.globals, &mut state);
    let list_defs = build_list_defs(&program.lists);
    let list_items = build_list_items(&program.list_items);
    let externals = build_externals(&program.externals);
    let struct_shapes = build_struct_shapes(&program.struct_shapes);

    // Convert scope line tables to a sorted Vec<ScopeLineTable>.
    let mut line_tables: Vec<ScopeLineTable> = state
        .scope_line_tables
        .into_iter()
        .map(|(scope_id, lines)| ScopeLineTable { scope_id, lines })
        .collect();
    line_tables.sort_by_key(|lt| lt.scope_id.to_raw());

    Ok(StoryData {
        containers: state.containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        addresses: state.addresses,
        address_paths: state.address_paths,
        name_table: state.name_table,
        list_literals: state.list_literals,
        literal_pool: state.literal_pool,
        struct_shapes,
        source_checksum: 0,
    })
}

/// TM-4c: `lir::StructShapeDef` → `brink_format::StructShapeDef`, id order
/// preserved (`lir::lower::structs::struct_shape_defs` already hands back a
/// `Vec` ordered by `ShapeId`, so this is a 1:1 field mapping, not a sort).
fn build_struct_shapes(shapes: &[lir::StructShapeDef]) -> Vec<StructShapeDef> {
    shapes
        .iter()
        .map(|s| StructShapeDef {
            id: ShapeId(s.id),
            name: s.name,
            fields: s.fields.clone(),
        })
        .collect()
}

// ─── Emission state ─────────────────────────────────────────────────

struct EmitState {
    containers: Vec<ContainerDef>,
    addresses: Vec<AddressDef>,
    /// Qualified-path → target table (scope containers + author labels).
    address_paths: Vec<AddressPath>,
    /// Scope-shared line tables: `scope_id` → accumulated line entries.
    scope_line_tables: HashMap<DefinitionId, Vec<LineEntry>>,
    list_literals: Vec<ListValue>,
    /// The T1b `LiteralPool` (`docs/format-v4-rfc.md` §2), built up as
    /// `PushLiteral` sites are emitted. Content-hash-dedup isn't needed for
    /// correctness (structural equality dedup below is exact); a linear
    /// scan is fine at game-corpus literal-pool sizes.
    literal_pool: Vec<Value>,
    name_table: Vec<String>,
    name_index: HashMap<String, NameId>,
    /// Codegen-level defects found during the tree walk (see
    /// [`CodegenError`]) — accumulated the same way `brink-ir`'s LIR
    /// lowering accumulates diagnostics (`ctx.diagnostics.push`), checked
    /// once after the whole walk finishes rather than threading a
    /// `Result` through every recursive emitter call. Bounded by the size
    /// of the `Program` being walked, same as every other `Vec` here.
    errors: Vec<CodegenError>,
}

/// Intern a string into a story name table, deduping against entries already
/// present. Shared by both codegen phases: the container walk
/// ([`ContainerEmitter::intern_string`]) and the post-walk build phase
/// ([`const_to_value`]), which hold the name table/index by different paths
/// but need identical dedup semantics.
fn intern_into(
    name_table: &mut Vec<String>,
    name_index: &mut HashMap<String, NameId>,
    s: &str,
) -> NameId {
    if let Some(&id) = name_index.get(s) {
        return id;
    }
    #[expect(clippy::cast_possible_truncation)]
    let id = NameId(name_table.len() as u16);
    name_table.push(s.to_string());
    name_index.insert(s.to_string(), id);
    id
}

// ─── Container emitter ──────────────────────────────────────────────

struct ContainerEmitter<'a> {
    bytecode: Vec<u8>,
    scope_line_table: &'a mut Vec<LineEntry>,
    list_literals: &'a mut Vec<ListValue>,
    literal_pool: &'a mut Vec<Value>,
    state_name_table: &'a mut Vec<String>,
    state_name_index: &'a mut HashMap<String, NameId>,
    in_conditional_branch: bool,
    /// Stack of open T1b `LogicWhile` loops (innermost last) — targets for
    /// `break`/`continue` jump patching. Empty outside any loop.
    loop_stack: Vec<LoopCtx>,
    /// Shared with every other `ContainerEmitter` created during the same
    /// `emit()` call (see `EmitState::errors`).
    errors: &'a mut Vec<CodegenError>,
}

/// Jump-patch bookkeeping for one open `LogicWhile` (innermost = top of
/// `ContainerEmitter::loop_stack`).
struct LoopCtx {
    /// `break` sites — patched to land just after the whole loop.
    break_patches: Vec<usize>,
    /// `continue` sites — patched to land at the start of `post` (the
    /// backward jump to `condition` for a plain `while`, since `post` is
    /// empty then).
    continue_patches: Vec<usize>,
}

impl<'a> ContainerEmitter<'a> {
    fn new(state: &'a mut EmitState, scope_id: DefinitionId) -> Self {
        let scope_line_table = state.scope_line_tables.entry(scope_id).or_default();
        Self {
            bytecode: Vec::new(),
            scope_line_table,
            list_literals: &mut state.list_literals,
            literal_pool: &mut state.literal_pool,
            state_name_table: &mut state.name_table,
            state_name_index: &mut state.name_index,
            in_conditional_branch: false,
            loop_stack: Vec::new(),
            errors: &mut state.errors,
        }
    }

    #[expect(clippy::needless_pass_by_value)]
    fn emit(&mut self, op: Opcode) {
        op.encode(&mut self.bytecode);
    }

    fn add_line(&mut self, text: &str) -> u16 {
        self.add_line_with_hash(text, brink_format::content_hash(text), Vec::new(), None)
    }

    #[expect(clippy::cast_possible_truncation)]
    fn add_line_with_hash(
        &mut self,
        text: &str,
        source_hash: u64,
        slot_info: Vec<brink_format::SlotInfo>,
        source_location: Option<brink_format::SourceLocation>,
    ) -> u16 {
        let idx = self.scope_line_table.len() as u16;
        let content = LineContent::Plain(collapse_whitespace(text));
        let flags = brink_format::LineFlags::from_content(&content);
        self.scope_line_table.push(LineEntry {
            content,
            flags,
            source_hash,
            audio_ref: None,
            slot_info,
            source_location,
        });
        idx
    }

    #[expect(clippy::cast_possible_truncation)]
    fn add_template_line(
        &mut self,
        parts: brink_format::LineTemplate,
        source_hash: u64,
        slot_info: Vec<brink_format::SlotInfo>,
        source_location: Option<brink_format::SourceLocation>,
    ) -> u16 {
        let idx = self.scope_line_table.len() as u16;
        let parts = parts
            .into_iter()
            .map(|part| match part {
                brink_format::LinePart::Literal(s) => {
                    brink_format::LinePart::Literal(collapse_whitespace(&s))
                }
                other => other,
            })
            .collect();
        let content = LineContent::Template(parts);
        let flags = brink_format::LineFlags::from_content(&content);
        self.scope_line_table.push(LineEntry {
            content,
            flags,
            source_hash,
            audio_ref: None,
            slot_info,
            source_location,
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

/// Returns `true` if the container kind is a lexical scope (root, knot, stitch).
fn is_scope_kind(kind: lir::ContainerKind) -> bool {
    matches!(
        kind,
        lir::ContainerKind::Root | lir::ContainerKind::Knot | lir::ContainerKind::Stitch
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "single linear emit sequence; splitting would obscure the order"
)]
fn walk_container(
    container: &lir::Container,
    path: &str,
    scope_author_path: &str,
    scope_id: DefinitionId,
    state: &mut EmitState,
) {
    // The author-facing path of the nearest enclosing scope. For a scope
    // container this is its own `path` (scope paths never get inklecate's
    // implicit `0.` stitch prefix); non-scope containers inherit it. Used to
    // qualify author labels (matching the analyzer's `qualify_label`),
    // independent of the inklecate `path` used for `path_hash`.
    let this_scope_path = if is_scope_kind(container.kind) {
        path
    } else {
        scope_author_path
    };

    // Emit this container's bytecode.
    let mut emitter = ContainerEmitter::new(state, scope_id);

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

    // Scope-owning containers get a human-readable name for the intl pipeline.
    let name = if is_scope_kind(container.kind) {
        Some(emitter.intern_string(path))
    } else {
        None
    };

    // Qualified author path → this container, for find_address. Scope
    // containers are addressable by their scope path; author-labeled
    // gathers/choices by `{enclosing_scope}.{label}`. Interned now while the
    // emitter is alive; the entry is pushed after the emitter is consumed.
    let address_path_id: Option<NameId> = if is_scope_kind(container.kind) {
        name
    } else if container.labeled {
        let label = container.name.as_deref().unwrap_or("_anon");
        let qualified = if this_scope_path.is_empty() {
            label.to_string()
        } else {
            format!("{this_scope_path}.{label}")
        };
        Some(emitter.intern_string(&qualified))
    } else {
        None
    };

    let def = ContainerDef {
        id: container.id,
        scope_id,
        name,
        bytecode: emitter.bytecode,
        counting_flags: container.counting_flags,
        path_hash,
        // Declared-parameter count for arity-checking a host-directed entry
        // (`choose_path_string_with_args`) / `call_function`. Saturates — a
        // knot with >255 params is absurd and never legitimately occurs.
        param_count: u8::try_from(container.params.len()).unwrap_or(u8::MAX),
        // Per-param name/mode metadata (T1c, #700): carried so the runtime can
        // validate a rehydrated function value against the current signature.
        // The LIR param `NameId`s are valid in the story name table (it is
        // cloned from `program.name_table` — see `emit`), so they are used
        // as-is.
        params: container
            .params
            .iter()
            .map(|p| brink_format::ParamMeta {
                name: p.name,
                is_ref: p.is_ref,
            })
            .collect(),
        local: container.local,
    };
    state.containers.push(def);

    // Primary address: every container is addressable by its own id.
    state.addresses.push(AddressDef {
        id: container.id,
        container_id: container.id,
        byte_offset: 0,
    });

    // Record the qualified-path → container mapping for find_address.
    if let Some(path_id) = address_path_id {
        state.address_paths.push(AddressPath {
            path: path_id,
            target: container.id,
        });
    }

    // Recurse into children.
    for child in &container.children {
        let child_name = child.name.as_deref().unwrap_or("_anon");

        // Compute the path segment for this child, applying inklecate-compatible
        // naming rules so that path_hash values match for shuffle RNG seeding.
        // Inklecate-compatible path naming rules so path_hash values match
        // for shuffle RNG seeding. Only non-function knots get the implicit
        // stitch ".0" prefix (functions store children directly).
        let needs_stitch_prefix = container.kind == lir::ContainerKind::Knot
            && !container.is_function
            && child.kind != lir::ContainerKind::Stitch;

        let segment = if needs_stitch_prefix && child.kind == lir::ContainerKind::Sequence {
            // Rule 1+2: stitch prefix + rename "s-N" → "N"
            let n = child_name.strip_prefix("s-").unwrap_or(child_name);
            format!("0.{n}")
        } else if needs_stitch_prefix {
            // Rule 1: just add stitch prefix
            format!("0.{child_name}")
        } else if child.kind == lir::ContainerKind::Sequence {
            // Rule 2: Sequence wrappers elsewhere: rename "s-N" → "N"
            child_name
                .strip_prefix("s-")
                .unwrap_or(child_name)
                .to_string()
        } else if container.kind == lir::ContainerKind::Sequence
            && child.kind == lir::ContainerKind::SequenceBranch
        {
            // Rule 3: Sequence branches: rename "N" → "sN"
            format!("s{child_name}")
        } else {
            child_name.to_string()
        };

        let child_path = if path.is_empty() {
            segment
        } else {
            format!("{path}.{segment}")
        };
        // If this child is a scope (knot, stitch, root), it starts a new scope.
        // Otherwise it inherits the parent's scope.
        let child_scope_id = if is_scope_kind(child.kind) {
            child.id
        } else {
            scope_id
        };
        // A scope child's author path is its own (author-form) path; other
        // children inherit the nearest enclosing scope's author path.
        let child_scope_author_path: &str = if is_scope_kind(child.kind) {
            &child_path
        } else {
            this_scope_path
        };
        walk_container(
            child,
            &child_path,
            child_scope_author_path,
            child_scope_id,
            state,
        );
    }
}

// ─── Top-level definition builders ─────────────────────────────────

fn build_globals(globals: &[lir::GlobalDef], state: &mut EmitState) -> Vec<GlobalVarDef> {
    globals
        .iter()
        .map(|g| GlobalVarDef {
            id: g.id,
            name: g.name,
            value_type: const_value_type(&g.default),
            default_value: const_to_value(&g.default, &mut state.name_table, &mut state.name_index),
            mutable: g.mutable,
            local: g.local,
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
        lir::ConstValue::Array(_) => brink_format::ValueType::Array,
        lir::ConstValue::Map(_) => brink_format::ValueType::Map,
        lir::ConstValue::FnRef(_) => brink_format::ValueType::FnRef,
        lir::ConstValue::Closure { .. } => brink_format::ValueType::Closure,
    }
}

fn const_to_value(
    v: &lir::ConstValue,
    name_table: &mut Vec<String>,
    name_index: &mut HashMap<String, NameId>,
) -> Value {
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
        lir::ConstValue::Array(items) => Value::array(
            items
                .iter()
                .map(|i| const_to_value(i, name_table, name_index))
                .collect(),
        ),
        lir::ConstValue::Map(entries) => {
            let mut map = OrderedMap::with_capacity(entries.len());
            for (k, v) in entries {
                let val = const_to_value(v, name_table, name_index);
                map.insert(const_map_key_to_value(k), val);
            }
            Value::map(map)
        }
        // Function values baked into a declaration default (T1c, #700). The
        // param name is interned (deduped) into the story name table so it
        // resolves to the same string the target container's `params` table
        // carries — the runtime rehydration check compares the two by name.
        lir::ConstValue::FnRef(target) => Value::FnRef(*target),
        lir::ConstValue::Closure { target, env } => {
            let env = env
                .iter()
                .map(|e| match e {
                    lir::ConstClosureEntry::Val { name, value } => {
                        let payload = const_to_value(value, name_table, name_index);
                        brink_format::ClosureEnvEntry {
                            name: intern_into(name_table, name_index, name),
                            is_ref: false,
                            payload,
                        }
                    }
                    lir::ConstClosureEntry::Ref { name, cell } => brink_format::ClosureEnvEntry {
                        name: intern_into(name_table, name_index, name),
                        is_ref: true,
                        payload: Value::VariablePointer(*cell),
                    },
                })
                .collect();
            Value::closure(*target, env)
        }
    }
}

fn const_map_key_to_value(k: &lir::ConstMapKey) -> MapKey {
    match k {
        lir::ConstMapKey::Int(n) => MapKey::Int(*n),
        lir::ConstMapKey::Str(s) => MapKey::Str(s.clone().into()),
        lir::ConstMapKey::Bool(b) => MapKey::Bool(*b),
    }
}
