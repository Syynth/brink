use std::collections::HashMap;

use brink_format::{
    ChoiceFlags, ContainerDef, CountingFlags, DefinitionId, ExternalFnDef, GlobalVarDef,
    LineContent, LineEntry, ListDef, ListItemDef, ListValue, NameId, Opcode, SequenceKind, Value,
};
use brink_json::{
    ChoicePoint, ChoicePointFlags, Container, ContainerFlags, ControlCommand, Divert, Element,
    InkValue, NativeFunction, VariableAssignment, VariableReference,
};

use crate::error::ConvertError;
use crate::index::StoryIndex;
use crate::path;

/// Temp variable scope, shared across a knot/function.
pub(crate) struct TempScope {
    vars: HashMap<String, u16>,
    next_slot: u16,
}

impl TempScope {
    pub(crate) fn new() -> Self {
        Self {
            vars: HashMap::new(),
            next_slot: 0,
        }
    }

    fn declare(&mut self, name: &str) -> Result<u16, ConvertError> {
        // If already declared in this scope, reuse the existing slot.
        // Ink temp variables are flat within a callframe — redeclaring
        // just overwrites the same slot.
        if let Some(&existing) = self.vars.get(name) {
            return Ok(existing);
        }
        let slot = self.next_slot;
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .ok_or(ConvertError::TempOverflow)?;
        self.vars.insert(name.to_string(), slot);
        Ok(slot)
    }

    fn get(&self, name: &str) -> Option<u16> {
        self.vars.get(name).copied()
    }
}

/// Emitter state for a single container.
struct ContainerEmitter<'a> {
    index: &'a StoryIndex,
    current_path: String,
    bytecode: Vec<u8>,
    line_table: Vec<LineEntry>,
    /// Offset into the scope's line table when this container started emitting.
    /// Updated when local lines are flushed to the scope table.
    scope_line_offset: u16,
    in_eval_mode: bool,
    in_string_eval: bool,
}

impl<'a> ContainerEmitter<'a> {
    fn new(index: &'a StoryIndex, current_path: String, scope_line_offset: u16) -> Self {
        Self {
            index,
            current_path,
            bytecode: Vec::new(),
            line_table: Vec::new(),
            scope_line_offset,
            in_eval_mode: false,
            in_string_eval: false,
        }
    }

    fn resolve_divert_target(
        &self,
        ink_path: &str,
    ) -> Result<brink_format::DefinitionId, ConvertError> {
        let resolved = path::resolve_path(&self.current_path, ink_path);
        // Check intra-container addresses first (for index-based targets), then containers.
        if let Some(addr_id) = self.index.intra_addresses.get(&resolved) {
            return Ok(*addr_id);
        }
        self.index
            .resolve_container(&resolved)
            .ok_or(ConvertError::UnresolvedPath(resolved))
    }

    /// Check whether an ink.json divert path resolves to a direct named child
    /// container of the current container. Used to detect sequence branch
    /// diverts (`.^.s0`) that should be emitted as `EnterContainer` instead
    /// of `Goto`.
    fn is_child_container_divert(&self, ink_path: &str) -> bool {
        let resolved = path::resolve_path(&self.current_path, ink_path);
        let prefix = format!("{}.", self.current_path);
        if !resolved.starts_with(&prefix) {
            return false;
        }
        let suffix = &resolved[prefix.len()..];
        // Direct child: no more dots in the suffix, and it's a registered container.
        !suffix.contains('.') && self.index.containers.contains_key(&resolved)
    }

    fn emit(&mut self, op: &Opcode) {
        op.encode(&mut self.bytecode);
    }

    fn add_line(&mut self, text: &str) -> Result<u16, ConvertError> {
        let local_idx =
            u16::try_from(self.line_table.len()).map_err(|_| ConvertError::LineTableOverflow)?;
        self.line_table.push(LineEntry {
            content: LineContent::Plain(text.to_string()),
            source_hash: brink_format::content_hash(text),
        });
        // Return scope-relative index.
        Ok(self.scope_line_offset + local_idx)
    }

    /// Flush accumulated local line entries to the scope's line table.
    /// Must be called before recursing into child containers that share the same scope.
    fn flush_lines(&mut self, scope_line_table: &mut Vec<LineEntry>) {
        scope_line_table.append(&mut self.line_table);
        #[expect(clippy::cast_possible_truncation)]
        {
            self.scope_line_offset = scope_line_table.len() as u16;
        }
    }

    fn emit_element(
        &mut self,
        element: &Element,
        name_table: &mut NameTableWriter,
        temps: &mut TempScope,
        list_literals: &mut Vec<ListValue>,
    ) -> Result<(), ConvertError> {
        match element {
            Element::Value(InkValue::String(s)) if s == "\n" => {
                if !self.in_eval_mode || self.in_string_eval {
                    self.emit(&Opcode::EmitNewline);
                }
            }

            Element::Value(InkValue::String(s)) => {
                if self.in_eval_mode && !self.in_string_eval {
                    let name_id = name_table.intern(s)?;
                    self.emit(&Opcode::PushString(name_id.0));
                } else {
                    let idx = self.add_line(s)?;
                    self.emit(&Opcode::EmitLine(idx));
                }
            }

            Element::Value(InkValue::Integer(i)) => {
                #[expect(clippy::cast_possible_truncation)]
                let val = *i as i32;
                self.emit(&Opcode::PushInt(val));
            }

            Element::Value(InkValue::Float(f)) => {
                #[expect(clippy::cast_possible_truncation)]
                let val = *f as f32;
                self.emit(&Opcode::PushFloat(val));
            }

            Element::Value(InkValue::Bool(b)) => {
                self.emit(&Opcode::PushBool(*b));
            }

            Element::Value(InkValue::DivertTarget(p)) => {
                let id = self.resolve_divert_target(p)?;
                self.emit(&Opcode::PushDivertTarget(id));
            }

            Element::Value(InkValue::VariablePointer(var)) => {
                if let Some(slot) = temps.get(var) {
                    // Temp ref — push a pointer to the temp slot.
                    self.emit(&Opcode::PushTempPointer(slot));
                } else {
                    // Global variable — push a pointer to it.
                    let id = self
                        .index
                        .globals
                        .get(var.as_str())
                        .copied()
                        .unwrap_or_else(|| path::global_var_id(var));
                    self.emit(&Opcode::PushVarPointer(id));
                }
            }

            Element::Value(InkValue::List(ink_list)) => {
                let lv = ink_list_to_list_value(ink_list, self.index);
                let idx = list_literals.len();
                list_literals.push(lv);
                #[expect(clippy::cast_possible_truncation)]
                self.emit(&Opcode::PushList(idx as u16));
            }

            Element::Void => {
                self.emit(&Opcode::PushNull);
            }

            Element::ControlCommand(cmd) => {
                self.emit_control_command(cmd);
            }

            Element::NativeFunction(func) => {
                self.emit_native_function(*func);
            }

            Element::Divert(divert) => {
                self.emit_divert(divert, temps)?;
            }

            Element::VariableAssignment(assign) => {
                self.emit_variable_assignment(assign, temps)?;
            }

            Element::VariableReference(VariableReference { variable }) => {
                if let Some(slot) = temps.get(variable) {
                    self.emit(&Opcode::GetTemp(slot));
                } else {
                    let id = self
                        .index
                        .globals
                        .get(variable.as_str())
                        .copied()
                        .unwrap_or_else(|| path::global_var_id(variable));
                    self.emit(&Opcode::GetGlobal(id));
                }
            }

            Element::ReadCount(rc) => {
                let id = self.resolve_divert_target(&rc.variable)?;
                self.emit(&Opcode::PushDivertTarget(id));
                self.emit(&Opcode::VisitCount);
            }

            Element::ChoicePoint(cp) => {
                self.emit_choice_point(cp)?;
            }

            Element::Container(child) => {
                self.emit_child_container(child);
            }

            Element::Nop => {}
        }

        Ok(())
    }

    fn emit_control_command(&mut self, cmd: &ControlCommand) {
        match cmd {
            ControlCommand::BeginLogicalEval => self.in_eval_mode = true,
            ControlCommand::EndLogicalEval => self.in_eval_mode = false,
            ControlCommand::Output => self.emit(&Opcode::EmitValue),
            ControlCommand::Pop => self.emit(&Opcode::Pop),
            ControlCommand::TunnelReturn => self.emit(&Opcode::TunnelReturn),
            ControlCommand::FunctionReturn => self.emit(&Opcode::Return),
            ControlCommand::Duplicate => self.emit(&Opcode::Duplicate),
            ControlCommand::BeginStringEval => {
                self.in_string_eval = true;
                self.emit(&Opcode::BeginStringEval);
            }
            ControlCommand::EndStringEval => {
                self.in_string_eval = false;
                self.emit(&Opcode::EndStringEval);
            }
            ControlCommand::NoOperation => self.emit(&Opcode::Nop),
            ControlCommand::ChoiceCount => self.emit(&Opcode::ChoiceCount),
            ControlCommand::Turn => self.emit(&Opcode::TurnIndex),
            ControlCommand::Turns => self.emit(&Opcode::TurnsSince),
            ControlCommand::Visit => self.emit(&Opcode::CurrentVisitCount),
            ControlCommand::Sequence => self.emit(&Opcode::Sequence(SequenceKind::Shuffle, 0)),
            ControlCommand::Thread => self.emit(&Opcode::ThreadStart),
            ControlCommand::Done => self.emit(&Opcode::Done),
            ControlCommand::End => self.emit(&Opcode::End),
            ControlCommand::Tag => self.emit(&Opcode::BeginTag),
            ControlCommand::Glue => self.emit(&Opcode::Glue),
            ControlCommand::EndTag => self.emit(&Opcode::EndTag),
        }
    }

    fn emit_native_function(&mut self, func: NativeFunction) {
        let op = match func {
            NativeFunction::Add => Opcode::Add,
            NativeFunction::Subtract => Opcode::Subtract,
            NativeFunction::Multiply => Opcode::Multiply,
            NativeFunction::Divide => Opcode::Divide,
            NativeFunction::Modulo => Opcode::Modulo,
            NativeFunction::Negate => Opcode::Negate,
            NativeFunction::Equal => Opcode::Equal,
            NativeFunction::NotEqual => Opcode::NotEqual,
            NativeFunction::GreaterThan => Opcode::Greater,
            NativeFunction::LessThan => Opcode::Less,
            NativeFunction::GreaterThanEqual => Opcode::GreaterOrEqual,
            NativeFunction::LessThanEqual => Opcode::LessOrEqual,
            NativeFunction::And => Opcode::And,
            NativeFunction::Or => Opcode::Or,
            NativeFunction::Not => Opcode::Not,
            NativeFunction::Min => Opcode::Min,
            NativeFunction::Max => Opcode::Max,
            NativeFunction::Has => Opcode::ListContains,
            NativeFunction::HasNot => Opcode::ListNotContains,
            NativeFunction::Intersect => Opcode::ListIntersect,
            NativeFunction::Random => Opcode::Random,
            NativeFunction::SeedRandom => Opcode::SeedRandom,
            NativeFunction::ReadCount => Opcode::VisitCount,
            NativeFunction::Floor => Opcode::Floor,
            NativeFunction::Ceiling => Opcode::Ceiling,
            NativeFunction::IntCast => Opcode::CastToInt,
            NativeFunction::FloatCast => Opcode::CastToFloat,
            NativeFunction::Pow => Opcode::Pow,
            NativeFunction::ListCount => Opcode::ListCount,
            NativeFunction::ListAll => Opcode::ListAll,
            NativeFunction::ListMin => Opcode::ListMin,
            NativeFunction::ListMax => Opcode::ListMax,
            NativeFunction::ListValue => Opcode::ListValue,
            NativeFunction::ListRandom | NativeFunction::ListRandom2 => Opcode::ListRandom,
            NativeFunction::ListRange | NativeFunction::Range => Opcode::ListRange,
            NativeFunction::ListInvert => Opcode::ListInvert,
            NativeFunction::ListInt => Opcode::ListFromInt,
        };
        self.emit(&op);
    }

    fn emit_divert(&mut self, divert: &Divert, temps: &mut TempScope) -> Result<(), ConvertError> {
        match divert {
            Divert::Target { conditional, path } => {
                if *conditional && self.is_child_container_divert(path) {
                    // Conditional divert to a named child of the current
                    // container (e.g. sequence branches `.^.s0`). Emit as
                    // JumpIfFalse + EnterContainer so the child is pushed
                    // on the container stack rather than replacing it.
                    let resolved = path::resolve_path(&self.current_path, path);
                    let child_id = self.index.containers[&resolved];
                    // EnterContainer encodes as 1 (tag) + 8 (DefinitionId) = 9 bytes.
                    self.emit(&Opcode::JumpIfFalse(9));
                    self.emit(&Opcode::EnterContainer(child_id));
                } else {
                    let id = self.resolve_divert_target(path)?;
                    if *conditional {
                        self.emit(&Opcode::GotoIf(id));
                    } else {
                        self.emit(&Opcode::Goto(id));
                    }
                }
            }

            Divert::Variable { path, .. } => {
                // Push the variable's value onto the stack before diverting.
                if let Some(slot) = temps.get(path) {
                    self.emit(&Opcode::GetTemp(slot));
                } else {
                    let id = path::global_var_id(path);
                    self.emit(&Opcode::GetGlobal(id));
                }
                self.emit(&Opcode::GotoVariable);
            }

            Divert::Function { path, .. } => {
                let id = self.resolve_divert_target(path)?;
                self.emit(&Opcode::Call(id));
            }

            Divert::FunctionVariable { path, .. } => {
                if let Some(slot) = temps.get(path) {
                    self.emit(&Opcode::GetTemp(slot));
                } else {
                    let id = path::global_var_id(path);
                    self.emit(&Opcode::GetGlobal(id));
                }
                self.emit(&Opcode::CallVariable);
            }

            Divert::Tunnel { path, .. } => {
                let id = self.resolve_divert_target(path)?;
                self.emit(&Opcode::TunnelCall(id));
            }

            Divert::TunnelVariable { path, .. } => {
                if let Some(slot) = temps.get(path) {
                    self.emit(&Opcode::GetTemp(slot));
                } else {
                    let id = path::global_var_id(path);
                    self.emit(&Opcode::GetGlobal(id));
                }
                self.emit(&Opcode::TunnelCallVariable);
            }

            Divert::ExternalFunction {
                name, arg_count, ..
            } => {
                let id = path::external_fn_id(name);
                #[expect(clippy::cast_possible_truncation)]
                let argc = *arg_count as u8;
                self.emit(&Opcode::CallExternal(id, argc));
            }
        }
        Ok(())
    }

    fn emit_variable_assignment(
        &mut self,
        assign: &VariableAssignment,
        temps: &mut TempScope,
    ) -> Result<(), ConvertError> {
        match assign {
            VariableAssignment::GlobalAssignment { variable, .. } => {
                // "VAR=" with "re":true can target either a global or a temp
                // that was previously declared with "temp=".
                if let Some(slot) = temps.get(variable) {
                    self.emit(&Opcode::SetTemp(slot));
                } else {
                    let id = self
                        .index
                        .globals
                        .get(variable.as_str())
                        .copied()
                        .unwrap_or_else(|| path::global_var_id(variable));
                    self.emit(&Opcode::SetGlobal(id));
                }
            }
            VariableAssignment::TemporaryAssignment { variable, reassign } => {
                if *reassign {
                    let slot = temps.get(variable).unwrap_or_else(|| {
                        // Reassignment to unknown temp — treat as new declaration
                        temps.declare(variable).unwrap_or(0)
                    });
                    self.emit(&Opcode::SetTemp(slot));
                } else {
                    let slot = temps.declare(variable)?;
                    self.emit(&Opcode::DeclareTemp(slot));
                }
            }
        }
        Ok(())
    }

    fn emit_choice_point(&mut self, cp: &ChoicePoint) -> Result<(), ConvertError> {
        let id = self.resolve_divert_target(&cp.target)?;

        let flags = ChoiceFlags {
            has_condition: cp.flags.contains(ChoicePointFlags::HAS_CONDITION),
            has_start_content: cp.flags.contains(ChoicePointFlags::HAS_START_CONTENT),
            has_choice_only_content: cp.flags.contains(ChoicePointFlags::HAS_CHOICE_ONLY_CONTENT),
            once_only: cp.flags.contains(ChoicePointFlags::ONCE_ONLY),
            is_invisible_default: cp.flags.contains(ChoicePointFlags::IS_INVISIBLE_DEFAULT),
        };

        self.emit(&Opcode::BeginChoice(flags, id));
        self.emit(&Opcode::EndChoice);
        Ok(())
    }

    fn emit_child_container(&mut self, child: &Container) {
        // Emit an EnterContainer instruction for named children.
        // Indexed children are handled in process_container.
        if let Some(name) = &child.name {
            let child_path = if self.current_path.is_empty() {
                name.clone()
            } else {
                format!("{}.{name}", self.current_path)
            };
            if let Some(&id) = self.index.containers.get(&child_path) {
                self.emit(&Opcode::EnterContainer(id));
            }
        }
    }
}

/// Mutable name table writer used during codegen.
pub struct NameTableWriter {
    strings: Vec<String>,
    index: HashMap<String, u16>,
}

impl NameTableWriter {
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn intern(&mut self, s: &str) -> Result<NameId, ConvertError> {
        if let Some(&idx) = self.index.get(s) {
            return Ok(NameId(idx));
        }
        let idx = u16::try_from(self.strings.len()).map_err(|_| ConvertError::NameTableOverflow)?;
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), idx);
        Ok(NameId(idx))
    }

    pub fn into_vec(self) -> Vec<String> {
        self.strings
    }
}

/// Per-container element offset map: container `DefinitionId` → (element index → byte offset).
///
/// Keyed by `DefinitionId` rather than path string so that containers with
/// both a numeric path ("0.0") and a named alias ("0.g-0") share the same
/// entry — labels that reference the named path can find the offsets recorded
/// during codegen under the numeric path.
pub type ElementOffsets = HashMap<DefinitionId, HashMap<usize, usize>>;

/// Convert ink.json `ContainerFlags` to brink-format `CountingFlags`.
fn convert_counting_flags(flags: Option<ContainerFlags>) -> CountingFlags {
    flags.map_or_else(CountingFlags::empty, |f| {
        let mut cf = CountingFlags::empty();
        if f.contains(ContainerFlags::VISITS) {
            cf |= CountingFlags::VISITS;
        }
        if f.contains(ContainerFlags::TURNS) {
            cf |= CountingFlags::TURNS;
        }
        if f.contains(ContainerFlags::COUNT_START_ONLY) {
            cf |= CountingFlags::COUNT_START_ONLY;
        }
        cf
    })
}

/// Process a container and all sub-containers, returning `ContainerDef`s
/// and populating the scope line tables map.
#[expect(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn process_container(
    index: &StoryIndex,
    container: &Container,
    current_path: &str,
    name_table: &mut NameTableWriter,
    temps: &mut TempScope,
    element_offsets: &mut ElementOffsets,
    list_literals: &mut Vec<ListValue>,
    scope_line_tables: &mut HashMap<DefinitionId, Vec<LineEntry>>,
) -> Result<Vec<ContainerDef>, ConvertError> {
    let mut all_defs = Vec::new();

    // Track where to insert this container so it precedes its inline children.
    // The root container (path "") ends up at index 0 — the linker convention.
    let self_insert_idx = all_defs.len();

    let container_id = index
        .containers
        .get(current_path)
        .copied()
        .ok_or_else(|| ConvertError::UnresolvedPath(current_path.to_string()))?;
    let scope_id = index
        .scope_ids
        .get(&container_id)
        .copied()
        .unwrap_or(container_id);

    // Current offset into this scope's line table.
    #[expect(clippy::cast_possible_truncation)]
    let scope_line_offset = scope_line_tables.get(&scope_id).map_or(0, Vec::len) as u16;

    let mut emitter = ContainerEmitter::new(index, current_path.to_string(), scope_line_offset);
    let mut offsets_for_this_container: HashMap<usize, usize> = HashMap::new();

    // Process contents with index-based iteration for pattern detection.
    let contents = &container.contents;
    let mut i = 0;
    while i < contents.len() {
        // Record byte offset before processing this element.
        offsets_for_this_container.insert(i, emitter.bytecode.len());

        let element = &contents[i];

        if let Element::Container(child) = element {
            let child_path = child_path_for_index(current_path, i);
            if let Some(&child_id) = index.containers.get(&child_path) {
                emitter.emit(&Opcode::EnterContainer(child_id));
            }

            // Flush any accumulated local lines to the scope table before
            // recursing, so child containers see the correct scope offset.
            emitter.flush_lines(scope_line_tables.entry(scope_id).or_default());

            let child_defs = process_container(
                index,
                child,
                &child_path,
                name_table,
                temps,
                element_offsets,
                list_literals,
                scope_line_tables,
            )?;
            all_defs.extend(child_defs);
        } else if let Element::Divert(Divert::Target {
            path,
            conditional: false,
        }) = element
        {
            // Check if this is a divert to a ".s" choice-text container.
            // These should be EnterContainer (push child) instead of Goto.
            #[expect(clippy::case_sensitive_file_extension_comparisons)]
            if path.ends_with(".s") {
                if let Some(id) = index.resolve_container(path) {
                    emitter.emit(&Opcode::EnterContainer(id));
                } else {
                    emitter.emit_element(element, name_table, temps, list_literals)?;
                }
            } else {
                emitter.emit_element(element, name_table, temps, list_literals)?;
            }
        } else if matches!(element, Element::ControlCommand(ControlCommand::Thread))
            && i + 1 < contents.len()
        {
            // Thread pattern: `thread` + `-> target` becomes ThreadCall(target).
            if let Element::Divert(Divert::Target {
                path,
                conditional: false,
            }) = &contents[i + 1]
            {
                let id = emitter.resolve_divert_target(path)?;
                emitter.emit(&Opcode::ThreadCall(id));
                i += 2; // skip both thread and divert
                continue;
            }
            // Not followed by a simple divert — emit ThreadStart as-is
            emitter.emit_element(element, name_table, temps, list_literals)?;
        } else if matches!(
            element,
            Element::ControlCommand(ControlCommand::EndStringEval)
        ) && i + 1 < contents.len()
            && matches!(
                &contents[i + 1],
                Element::ControlCommand(ControlCommand::BeginStringEval)
            )
            && emitter.in_eval_mode
            && has_upcoming_choice_point(contents, i + 2)
        {
            // Merge adjacent string evals in choice eval context.
            // Only merge when a ChoicePoint follows within the same eval block,
            // so we don't accidentally merge string operands (e.g. `str ? str`).
            i += 2;
            continue;
        } else {
            emitter.emit_element(element, name_table, temps, list_literals)?;
        }

        i += 1;
    }

    let counting_flags = convert_counting_flags(container.flags);

    let path_hash: i32 = current_path.chars().map(|c| c as i32).sum();

    // Flush any remaining local line entries to the scope's line table.
    emitter.flush_lines(scope_line_tables.entry(scope_id).or_default());

    let def = ContainerDef {
        id: container_id,
        scope_id,
        bytecode: emitter.bytecode,
        content_hash: 0,
        counting_flags,
        path_hash,
    };

    // Store element offsets for this container, keyed by DefinitionId.
    if !offsets_for_this_container.is_empty() {
        element_offsets.insert(container_id, offsets_for_this_container);
    }

    // Insert this container before its inline children so that the parent
    // always precedes its descendants. In particular, the root container
    // (path "") ends up at index 0 — the convention the linker relies on.
    all_defs.insert(self_insert_idx, def);

    // Process named content
    for (name, element) in &container.named_content {
        if let Element::Container(child) = element {
            let child_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{current_path}.{name}")
            };
            let child_defs = process_container(
                index,
                child,
                &child_path,
                name_table,
                temps,
                element_offsets,
                list_literals,
                scope_line_tables,
            )?;
            all_defs.extend(child_defs);
        }
    }

    Ok(all_defs)
}

/// Check if a `ChoicePoint` element appears in the remaining container elements.
/// Used to guard adjacent string eval merging — we only merge when the strings
/// are choice display content, not standalone string operands (e.g. `?` operator).
///
/// The `ChoicePoint` always appears *after* `/ev` (`EndLogicalEval`), so we scan
/// past it. We stop at `NativeFunction` elements instead — if a native function
/// (like `?` / `Has`) appears between the string evals and the `ChoicePoint`, the
/// strings are operator arguments, not choice display text.
fn has_upcoming_choice_point(contents: &[Element], start: usize) -> bool {
    for element in &contents[start..] {
        match element {
            Element::ChoicePoint(_) => return true,
            Element::NativeFunction(_) => return false,
            _ => {}
        }
    }
    false
}

fn child_path_for_index(current_path: &str, i: usize) -> String {
    if current_path.is_empty() {
        i.to_string()
    } else {
        format!("{current_path}.{i}")
    }
}

/// Extract global variable definitions from the "global decl" container.
pub fn extract_globals(
    index: &StoryIndex,
    root: &Container,
    name_table: &mut NameTableWriter,
) -> Result<Vec<GlobalVarDef>, ConvertError> {
    let mut globals = Vec::new();

    let Some(Element::Container(global_decl)) = root.named_content.get("global decl") else {
        return Ok(globals);
    };

    // Walk the global decl container: values are followed by their VAR= assignments
    let mut pending_value: Option<Value> = None;

    for element in &global_decl.contents {
        match element {
            Element::Value(ink_val) => {
                pending_value = Some(ink_value_to_format_value(ink_val, index));
            }
            Element::VariableAssignment(
                VariableAssignment::GlobalAssignment { variable, .. }
                | VariableAssignment::TemporaryAssignment {
                    variable,
                    reassign: false,
                },
            ) => {
                let value = pending_value.take().unwrap_or(Value::Null);
                let id = index
                    .globals
                    .get(variable.as_str())
                    .copied()
                    .unwrap_or_else(|| path::global_var_id(variable));
                let name_id = name_table.intern(variable)?;
                globals.push(GlobalVarDef {
                    id,
                    name: name_id,
                    value_type: value.value_type(),
                    default_value: value,
                    mutable: true,
                });
            }
            // Don't clear pending_value on control commands (ev, /ev,
            // str, /str) — string constants use `str, ^text, /str`
            // wrappers between the value and the assignment.
            _ => {}
        }
    }

    Ok(globals)
}

/// Convert an ink.json `InkValue` to a brink-format `Value`.
fn ink_value_to_format_value(ink: &InkValue, index: &StoryIndex) -> Value {
    match ink {
        InkValue::Integer(i) => {
            #[expect(clippy::cast_possible_truncation)]
            let val = *i as i32;
            Value::Int(val)
        }
        InkValue::Float(f) => {
            #[expect(clippy::cast_possible_truncation)]
            let val = *f as f32;
            Value::Float(val)
        }
        InkValue::Bool(b) => Value::Bool(*b),
        InkValue::String(s) => Value::String(s.clone().into()),
        InkValue::DivertTarget(p) => Value::DivertTarget(path::address_id(p)),
        InkValue::List(ink_list) => Value::List(ink_list_to_list_value(ink_list, index).into()),
        InkValue::VariablePointer(_) => Value::Null,
    }
}

/// Convert an ink.json `InkList` to a brink-format `ListValue`.
fn ink_list_to_list_value(ink_list: &brink_json::InkList, index: &StoryIndex) -> ListValue {
    // Collect items with ordinals so we can sort deterministically.
    // HashMap iteration order is non-deterministic across processes.
    let mut items_with_ord: Vec<_> = ink_list
        .items
        .keys()
        .filter_map(|qualified| {
            index
                .list_items
                .get(qualified.as_str())
                .map(|&(id, ord)| (id, ord))
        })
        .collect();
    items_with_ord.sort_by_key(|&(_, ord)| ord);
    let items: Vec<_> = items_with_ord.iter().map(|&(id, _)| id).collect();

    // Use explicit origins if present; otherwise derive from item qualified names.
    let mut origins: Vec<_> = ink_list
        .origins
        .iter()
        .filter_map(|name| index.list_defs.get(name.as_str()).copied())
        .collect();
    if origins.is_empty() {
        // Sort keys for deterministic iteration over the HashMap.
        let mut keys: Vec<_> = ink_list.items.keys().collect();
        keys.sort();
        for qualified in keys {
            if let Some(dot) = qualified.find('.') {
                let list_name = &qualified[..dot];
                if let Some(&def_id) = index.list_defs.get(list_name)
                    && !origins.contains(&def_id)
                {
                    origins.push(def_id);
                }
            }
        }
    }

    ListValue { items, origins }
}

/// Build list definitions and items from the story index.
pub fn build_list_defs(
    index: &StoryIndex,
    name_table: &mut NameTableWriter,
) -> Result<(Vec<ListDef>, Vec<ListItemDef>), ConvertError> {
    let mut list_defs = Vec::new();
    let mut list_items = Vec::new();

    for (list_name, &list_id) in &index.list_defs {
        let list_name_id = name_table.intern(list_name)?;
        let mut items = Vec::new();

        for (qualified, &(item_id, ordinal)) in &index.list_items {
            if qualified.starts_with(list_name.as_str())
                && qualified.as_bytes().get(list_name.len()) == Some(&b'.')
            {
                let item_name_id = name_table.intern(qualified)?;
                items.push((item_name_id, ordinal));

                list_items.push(ListItemDef {
                    id: item_id,
                    origin: list_id,
                    ordinal,
                    name: item_name_id,
                });
            }
        }

        list_defs.push(ListDef {
            id: list_id,
            name: list_name_id,
            items,
        });
    }

    Ok((list_defs, list_items))
}

/// Build external function definitions from the story index.
pub fn build_externals(
    index: &StoryIndex,
    name_table: &mut NameTableWriter,
) -> Result<Vec<ExternalFnDef>, ConvertError> {
    let mut externals = Vec::new();

    for (name, &(id, argc)) in &index.externals {
        let name_id = name_table.intern(name)?;
        // If a container with the same name exists, it's the ink fallback body.
        let fallback = index.containers.get(name.as_str()).copied();
        externals.push(ExternalFnDef {
            id,
            name: name_id,
            arg_count: argc,
            fallback,
        });
    }

    Ok(externals)
}
