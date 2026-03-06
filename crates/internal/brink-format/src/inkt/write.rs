//! Textual (.inkt) writer for `StoryData`.
//!
//! Produces a WAT-inspired, section-based, indented mnemonic representation
//! of compiled story data for debugging and inspection.
//!
//! The output is lossless — every field in `StoryData` is represented so that
//! `read_inkt(write_inkt(story))` is an exact roundtrip.

use core::fmt;

use std::collections::HashMap;

use crate::counting::CountingFlags;
use crate::definition::{
    ContainerDef, ExternalFnDef, GlobalVarDef, LabelDef, LineEntry, ListDef, ListItemDef,
};
use crate::id::DefinitionId;
use crate::line::{LineContent, LinePart, SelectKey};
use crate::opcode::{ChoiceFlags, Opcode, SequenceKind};
use crate::story::StoryData;
use crate::value::{ListValue, Value, ValueType};

/// Write the textual (.inkt) representation of a compiled story.
pub fn write_inkt(story: &StoryData, w: &mut dyn fmt::Write) -> fmt::Result {
    writeln!(w, "(story")?;

    write_name_table(w, &story.name_table)?;
    write_globals(w, &story.variables)?;
    write_lists(w, &story.list_defs)?;
    write_list_items(w, &story.list_items)?;
    write_externals(w, &story.externals)?;
    write_labels(w, &story.labels)?;
    write_list_literals(w, &story.list_literals)?;

    // Build a lookup from container_id → line table for writing
    let line_map: HashMap<DefinitionId, &[LineEntry]> = story
        .line_tables
        .iter()
        .map(|lt| (lt.container_id, lt.lines.as_slice()))
        .collect();

    for container in &story.containers {
        let lines = line_map.get(&container.id).copied().unwrap_or(&[]);
        write_container(w, container, lines)?;
    }

    write!(w, ")")
}

impl fmt::Display for StoryData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_inkt(self, f)
    }
}

// ── Sections ─────────────────────────────────────────────────────────────────

fn write_name_table(w: &mut dyn fmt::Write, names: &[String]) -> fmt::Result {
    if names.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "  (name_table")?;
    for (i, name) in names.iter().enumerate() {
        writeln!(w, "    {i} \"{}\"", escape_string(name))?;
    }
    writeln!(w, "  )")
}

fn write_globals(w: &mut dyn fmt::Write, globals: &[GlobalVarDef]) -> fmt::Result {
    if globals.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "  (globals")?;
    for g in globals {
        write!(
            w,
            "    (global {} :{} ",
            g.id,
            value_type_name(g.value_type)
        )?;
        write_value(w, &g.default_value)?;
        if g.mutable {
            write!(w, " mutable")?;
        }
        writeln!(w)?;
        writeln!(w, "      (name {}))", g.name.0)?;
    }
    writeln!(w, "  )")
}

fn write_lists(w: &mut dyn fmt::Write, list_defs: &[ListDef]) -> fmt::Result {
    if list_defs.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "  (lists")?;
    for ld in list_defs {
        writeln!(w, "    (list {}", ld.id)?;
        writeln!(w, "      (name {})", ld.name.0)?;
        for (item_name, ordinal) in &ld.items {
            writeln!(w, "      (item name={} ordinal={ordinal})", item_name.0)?;
        }
        writeln!(w, "    )")?;
    }
    writeln!(w, "  )")
}

fn write_list_items(w: &mut dyn fmt::Write, list_items: &[ListItemDef]) -> fmt::Result {
    if list_items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "  (list_items")?;
    for li in list_items {
        writeln!(
            w,
            "    (list_item {} (origin {}) (ordinal {}) (name {}))",
            li.id, li.origin, li.ordinal, li.name.0
        )?;
    }
    writeln!(w, "  )")
}

fn write_list_literals(w: &mut dyn fmt::Write, list_literals: &[ListValue]) -> fmt::Result {
    if list_literals.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "  (list_literals")?;
    for lv in list_literals {
        write!(w, "    (list (items")?;
        for item in &lv.items {
            write!(w, " {item}")?;
        }
        write!(w, ") (origins")?;
        for origin in &lv.origins {
            write!(w, " {origin}")?;
        }
        writeln!(w, "))")?;
    }
    writeln!(w, "  )")
}

fn write_externals(w: &mut dyn fmt::Write, externals: &[ExternalFnDef]) -> fmt::Result {
    if externals.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "  (externals")?;
    for ext in externals {
        write!(w, "    (extern {} argc={}", ext.id, ext.arg_count)?;
        writeln!(w)?;
        writeln!(w, "      (name {})", ext.name.0)?;
        if let Some(fb) = ext.fallback {
            writeln!(w, "      (fallback {fb})")?;
        }
        writeln!(w, "    )")?;
    }
    writeln!(w, "  )")
}

fn write_labels(w: &mut dyn fmt::Write, labels: &[LabelDef]) -> fmt::Result {
    if labels.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "  (labels")?;
    for label in labels {
        writeln!(
            w,
            "    (label {} -> {} +{})",
            label.id, label.container_id, label.byte_offset
        )?;
    }
    writeln!(w, "  )")
}

fn write_container(w: &mut dyn fmt::Write, c: &ContainerDef, lines: &[LineEntry]) -> fmt::Result {
    writeln!(w)?;
    writeln!(w, "  (container {}", c.id)?;

    // Content hash
    writeln!(w, "    (hash 0x{:016x})", c.content_hash)?;

    // Counting flags
    if !c.counting_flags.is_empty() {
        write!(w, "    (flags")?;
        if c.counting_flags.contains(CountingFlags::VISITS) {
            write!(w, " visits")?;
        }
        if c.counting_flags.contains(CountingFlags::TURNS) {
            write!(w, " turns")?;
        }
        if c.counting_flags.contains(CountingFlags::COUNT_START_ONLY) {
            write!(w, " start_only")?;
        }
        writeln!(w, ")")?;
    }

    // Path hash (for shuffle RNG seeding)
    if c.path_hash != 0 {
        writeln!(w, "    (path_hash {})", c.path_hash)?;
    }

    // Line table
    if !lines.is_empty() {
        writeln!(w, "    (lines")?;
        for (i, entry) in lines.iter().enumerate() {
            write!(w, "      {i} ")?;
            write_line_content(w, &entry.content)?;
            writeln!(w, " @{:016x}", entry.source_hash)?;
        }
        writeln!(w, "    )")?;
    }

    // Bytecode
    if !c.bytecode.is_empty() {
        writeln!(w, "    (code")?;
        write_bytecode(w, &c.bytecode)?;
        writeln!(w, "    )")?;
    }

    writeln!(w, "  )")
}

// ── Line content ─────────────────────────────────────────────────────────────

fn write_line_content(w: &mut dyn fmt::Write, content: &LineContent) -> fmt::Result {
    match content {
        LineContent::Plain(s) => write!(w, "\"{}\"", escape_string(s)),
        LineContent::Template(parts) => {
            write!(w, "(template")?;
            for part in parts {
                write!(w, " ")?;
                match part {
                    LinePart::Literal(s) => write!(w, "(lit \"{}\")", escape_string(s))?,
                    LinePart::Slot(idx) => write!(w, "(slot {idx})")?,
                    LinePart::Select {
                        slot,
                        variants,
                        default,
                    } => {
                        write!(w, "(select slot={slot}")?;
                        for (key, text) in variants {
                            write!(w, " (")?;
                            write_select_key(w, key)?;
                            write!(w, " \"{}\")", escape_string(text))?;
                        }
                        write!(w, " (default \"{}\"))", escape_string(default))?;
                    }
                }
            }
            write!(w, ")")
        }
    }
}

fn write_select_key(w: &mut dyn fmt::Write, key: &SelectKey) -> fmt::Result {
    match key {
        SelectKey::Cardinal(cat) => write!(w, "cardinal:{cat:?}"),
        SelectKey::Ordinal(cat) => write!(w, "ordinal:{cat:?}"),
        SelectKey::Exact(n) => write!(w, "={n}"),
        SelectKey::Keyword(k) => write!(w, "keyword:{k}"),
    }
}

// ── Bytecode disassembly ─────────────────────────────────────────────────────

fn write_bytecode(w: &mut dyn fmt::Write, bytecode: &[u8]) -> fmt::Result {
    let mut offset = 0;
    while offset < bytecode.len() {
        match Opcode::decode(bytecode, &mut offset) {
            Ok(op) => {
                write!(w, "      ")?;
                write_opcode(w, &op)?;
                writeln!(w)?;
            }
            Err(e) => {
                writeln!(w, "      <decode error: {e}>")?;
                break;
            }
        }
    }
    Ok(())
}

#[expect(clippy::too_many_lines)]
fn write_opcode(w: &mut dyn fmt::Write, op: &Opcode) -> fmt::Result {
    match op {
        // Stack & literals
        Opcode::PushInt(v) => write!(w, "push_int {v}"),
        Opcode::PushFloat(v) => write!(w, "push_float {v}"),
        Opcode::PushBool(v) => write!(w, "push_bool {v}"),
        Opcode::PushString(idx) => write!(w, "push_string {idx}"),
        Opcode::PushList(idx) => write!(w, "push_list {idx}"),
        Opcode::PushDivertTarget(id) => write!(w, "push_divert_target {id}"),
        Opcode::PushNull => write!(w, "push_null"),
        Opcode::Pop => write!(w, "pop"),
        Opcode::Duplicate => write!(w, "duplicate"),

        // Arithmetic
        Opcode::Add => write!(w, "add"),
        Opcode::Subtract => write!(w, "subtract"),
        Opcode::Multiply => write!(w, "multiply"),
        Opcode::Divide => write!(w, "divide"),
        Opcode::Modulo => write!(w, "modulo"),
        Opcode::Negate => write!(w, "negate"),

        // Comparison
        Opcode::Equal => write!(w, "equal"),
        Opcode::NotEqual => write!(w, "not_equal"),
        Opcode::Greater => write!(w, "greater"),
        Opcode::GreaterOrEqual => write!(w, "greater_or_equal"),
        Opcode::Less => write!(w, "less"),
        Opcode::LessOrEqual => write!(w, "less_or_equal"),

        // Logic
        Opcode::Not => write!(w, "not"),
        Opcode::And => write!(w, "and"),
        Opcode::Or => write!(w, "or"),

        // Global vars
        Opcode::GetGlobal(id) => write!(w, "get_global {id}"),
        Opcode::SetGlobal(id) => write!(w, "set_global {id}"),

        // Temp vars
        Opcode::DeclareTemp(idx) => write!(w, "declare_temp {idx}"),
        Opcode::GetTemp(idx) => write!(w, "get_temp {idx}"),
        Opcode::SetTemp(idx) => write!(w, "set_temp {idx}"),
        Opcode::GetTempRaw(idx) => write!(w, "get_temp_raw {idx}"),

        // Variable pointers
        Opcode::PushVarPointer(id) => write!(w, "push_var_pointer {id}"),
        Opcode::PushTempPointer(slot) => write!(w, "push_temp_pointer {slot}"),

        // Control flow
        Opcode::Jump(off) => write!(w, "jump {off}"),
        Opcode::JumpIfFalse(off) => write!(w, "jump_if_false {off}"),
        Opcode::Goto(id) => write!(w, "goto {id}"),
        Opcode::GotoIf(id) => write!(w, "goto_if {id}"),
        Opcode::GotoVariable => write!(w, "goto_variable"),

        // Container flow
        Opcode::EnterContainer(id) => write!(w, "enter_container {id}"),
        Opcode::ExitContainer => write!(w, "exit_container"),

        // Functions / tunnels
        Opcode::Call(id) => write!(w, "call {id}"),
        Opcode::Return => write!(w, "return"),
        Opcode::TunnelCall(id) => write!(w, "tunnel_call {id}"),
        Opcode::TunnelReturn => write!(w, "tunnel_return"),
        Opcode::TunnelCallVariable => write!(w, "tunnel_call_variable"),
        Opcode::CallVariable => write!(w, "call_variable"),

        // Threads
        Opcode::ThreadCall(id) => write!(w, "thread_call {id}"),
        Opcode::ThreadStart => write!(w, "thread_start"),
        Opcode::ThreadDone => write!(w, "thread_done"),

        // Output
        Opcode::EmitLine(idx) => write!(w, "emit_line {idx}"),
        Opcode::EmitValue => write!(w, "emit_value"),
        Opcode::EmitNewline => write!(w, "emit_newline"),
        Opcode::Glue => write!(w, "glue"),
        Opcode::BeginTag => write!(w, "begin_tag"),
        Opcode::EndTag => write!(w, "end_tag"),
        Opcode::EvalLine(idx) => write!(w, "eval_line {idx}"),

        // Choices
        Opcode::BeginChoiceSet => write!(w, "begin_choice_set"),
        Opcode::EndChoiceSet => write!(w, "end_choice_set"),
        Opcode::BeginChoice(flags, target) => {
            write!(w, "begin_choice {} {target}", format_choice_flags(*flags))
        }
        Opcode::EndChoice => write!(w, "end_choice"),
        Opcode::ChoiceOutput(idx) => write!(w, "choice_output {idx}"),

        // Sequences
        Opcode::Sequence(kind, count) => {
            write!(w, "sequence {} {count}", format_sequence_kind(*kind))
        }
        Opcode::SequenceBranch(off) => write!(w, "sequence_branch {off}"),

        // Intrinsics
        Opcode::VisitCount => write!(w, "visit_count"),
        Opcode::TurnsSince => write!(w, "turns_since"),
        Opcode::TurnIndex => write!(w, "turn_index"),
        Opcode::ChoiceCount => write!(w, "choice_count"),
        Opcode::Random => write!(w, "random"),
        Opcode::SeedRandom => write!(w, "seed_random"),

        // Casts / math
        Opcode::CastToInt => write!(w, "cast_to_int"),
        Opcode::CastToFloat => write!(w, "cast_to_float"),
        Opcode::Floor => write!(w, "floor"),
        Opcode::Ceiling => write!(w, "ceiling"),
        Opcode::Pow => write!(w, "pow"),
        Opcode::Min => write!(w, "min"),
        Opcode::Max => write!(w, "max"),

        // External fns
        Opcode::CallExternal(id, argc) => write!(w, "call_external {id} argc={argc}"),

        // List ops
        Opcode::ListContains => write!(w, "list_contains"),
        Opcode::ListNotContains => write!(w, "list_not_contains"),
        Opcode::ListIntersect => write!(w, "list_intersect"),
        Opcode::ListAll => write!(w, "list_all"),
        Opcode::ListInvert => write!(w, "list_invert"),
        Opcode::ListCount => write!(w, "list_count"),
        Opcode::ListMin => write!(w, "list_min"),
        Opcode::ListMax => write!(w, "list_max"),
        Opcode::ListValue => write!(w, "list_value"),
        Opcode::ListRange => write!(w, "list_range"),
        Opcode::ListFromInt => write!(w, "list_from_int"),
        Opcode::ListRandom => write!(w, "list_random"),

        // Lifecycle
        Opcode::Done => write!(w, "done"),
        Opcode::End => write!(w, "end"),
        Opcode::Nop => write!(w, "nop"),

        // String eval
        Opcode::BeginStringEval => write!(w, "begin_string_eval"),
        Opcode::EndStringEval => write!(w, "end_string_eval"),

        // Visit
        Opcode::CurrentVisitCount => write!(w, "current_visit_count"),

        // Debug
        Opcode::SourceLocation(line, col) => write!(w, "source_location {line}:{col}"),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn format_choice_flags(flags: ChoiceFlags) -> String {
    let mut parts = Vec::new();
    if flags.has_condition {
        parts.push("cond");
    }
    if flags.has_start_content {
        parts.push("start");
    }
    if flags.has_choice_only_content {
        parts.push("choice_only");
    }
    if flags.once_only {
        parts.push("once");
    }
    if flags.is_invisible_default {
        parts.push("invis_default");
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join("+")
    }
}

fn format_sequence_kind(kind: SequenceKind) -> &'static str {
    match kind {
        SequenceKind::Cycle => "cycle",
        SequenceKind::Stopping => "stopping",
        SequenceKind::OnceOnly => "once_only",
        SequenceKind::Shuffle => "shuffle",
    }
}

fn value_type_name(vt: ValueType) -> &'static str {
    match vt {
        ValueType::Int => "int",
        ValueType::Float => "float",
        ValueType::Bool => "bool",
        ValueType::String => "string",
        ValueType::List => "list",
        ValueType::DivertTarget => "divert_target",
        ValueType::VariablePointer => "var_pointer",
        ValueType::TempPointer => "temp_pointer",
        ValueType::Null => "null",
    }
}

fn write_value(w: &mut dyn fmt::Write, v: &Value) -> fmt::Result {
    match v {
        Value::Int(n) => write!(w, "{n}"),
        Value::Float(n) => {
            // Ensure float always has a decimal point for unambiguous parsing.
            let s = format!("{n}");
            if s.contains('.') || s.contains("inf") || s.contains("NaN") {
                write!(w, "{s}")
            } else {
                write!(w, "{s}.0")
            }
        }
        Value::Bool(b) => write!(w, "{b}"),
        Value::String(s) => write!(w, "\"{}\"", escape_string(s)),
        Value::List(lv) => {
            write!(w, "(list (items")?;
            for item in &lv.items {
                write!(w, " {item}")?;
            }
            write!(w, ") (origins")?;
            for origin in &lv.origins {
                write!(w, " {origin}")?;
            }
            write!(w, "))")
        }
        Value::DivertTarget(id) => write!(w, "{id}"),
        Value::VariablePointer(id) => write!(w, "(var_pointer {id})"),
        Value::TempPointer { slot, frame_depth } => {
            write!(w, "(temp_pointer {slot} {frame_depth})")
        }
        Value::Null => write!(w, "null"),
    }
}

pub(crate) fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{DefinitionId, DefinitionTag};

    #[test]
    fn definition_id_display() {
        let id = DefinitionId::new(DefinitionTag::Container, 0xDEAD_BEEF);
        assert_eq!(format!("{id}"), "$01_000000deadbeef");
    }

    #[test]
    fn escape_special_chars() {
        assert_eq!(escape_string("hello"), "hello");
        assert_eq!(escape_string("a\"b"), "a\\\"b");
        assert_eq!(escape_string("a\\b"), "a\\\\b");
        assert_eq!(escape_string("a\nb"), "a\\nb");
        assert_eq!(escape_string("a\tb"), "a\\tb");
    }

    #[test]
    fn empty_story() {
        let story = StoryData {
            containers: vec![],
            line_tables: vec![],
            variables: vec![],
            list_defs: vec![],
            list_items: vec![],
            externals: vec![],
            labels: vec![],
            name_table: vec![],
            list_literals: vec![],
        };
        let mut buf = String::new();
        write_inkt(&story, &mut buf).unwrap();
        assert_eq!(buf, "(story\n)");
    }

    #[test]
    fn choice_flags_formatting() {
        let flags = ChoiceFlags {
            has_condition: true,
            has_start_content: false,
            has_choice_only_content: false,
            once_only: true,
            is_invisible_default: false,
        };
        assert_eq!(format_choice_flags(flags), "cond+once");

        let empty = ChoiceFlags {
            has_condition: false,
            has_start_content: false,
            has_choice_only_content: false,
            once_only: false,
            is_invisible_default: false,
        };
        assert_eq!(format_choice_flags(empty), "none");
    }
}
