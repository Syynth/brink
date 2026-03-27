//! Pest-based reader for the `.inkt` textual format.

use pest::Parser;
use pest_derive::Parser;

use crate::counting::CountingFlags;
use crate::definition::{
    AddressDef, ContainerDef, ExternalFnDef, GlobalVarDef, LineEntry, ListDef, ListItemDef,
    ScopeLineTable, SlotInfo, SourceLocation,
};
use crate::id::{DefinitionId, NameId};
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};
use crate::opcode::{ChoiceFlags, Opcode, SequenceKind};
use crate::story::StoryData;
use crate::value::{ListValue, Value, ValueType};

#[derive(Parser)]
#[grammar = "inkt/inkt.pest"]
struct InktParser;

/// Error returned when parsing `.inkt` text fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InktParseError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl core::fmt::Display for InktParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for InktParseError {}

/// Parse `.inkt` text into a [`StoryData`].
pub fn read_inkt(input: &str) -> Result<StoryData, InktParseError> {
    let pairs = InktParser::parse(Rule::story, input).map_err(|e| {
        let (line, col) = match e.line_col {
            pest::error::LineColLocation::Pos(pos) => pos,
            pest::error::LineColLocation::Span(start, _) => start,
        };
        InktParseError {
            message: e.to_string(),
            line,
            col,
        }
    })?;

    let story_pair = pairs.into_iter().next().ok_or_else(|| InktParseError {
        message: "no story node".into(),
        line: 1,
        col: 1,
    })?;

    parse_story(story_pair)
}

type P<'a> = pest::iterators::Pair<'a, Rule>;

fn err(pair: &P<'_>, msg: impl Into<String>) -> InktParseError {
    let (line, col) = pair.line_col();
    InktParseError {
        message: msg.into(),
        line,
        col,
    }
}

fn parse_story(pair: P<'_>) -> Result<StoryData, InktParseError> {
    let mut name_table = Vec::new();
    let mut variables = Vec::new();
    let mut list_defs = Vec::new();
    let mut list_items = Vec::new();
    let mut externals = Vec::new();
    let mut addresses = Vec::new();
    let mut containers = Vec::new();
    let mut line_tables = Vec::new();
    let mut list_literals = Vec::new();
    let mut source_checksum = 0u32;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::story_checksum => {
                if let Some(hex_pair) = inner.into_inner().next() {
                    source_checksum = parse_hex_u32(hex_pair.as_str());
                }
            }
            Rule::name_table => name_table = parse_name_table(inner)?,
            Rule::globals => variables = parse_globals(inner)?,
            Rule::lists => list_defs = parse_lists(inner)?,
            Rule::list_items => list_items = parse_list_items(inner)?,
            Rule::externals => externals = parse_externals(inner)?,
            Rule::addresses => addresses = parse_addresses(inner)?,
            Rule::list_literals => list_literals = parse_list_literals(inner)?,
            Rule::container => {
                let (container, lt) = parse_container(inner)?;
                let is_scope_owner = container.scope_id == container.id;
                containers.push(container);
                // Only add line tables for scope-owning containers.
                // Child containers (scope_id != id) have no lines in the text.
                if is_scope_owner {
                    line_tables.push(lt);
                }
            }
            _ => {}
        }
    }

    // Sort line tables by scope_id for deterministic ordering,
    // matching the converter's output.
    line_tables.sort_by_key(|lt| lt.scope_id.to_raw());

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        addresses,
        name_table,
        list_literals,
        source_checksum,
    })
}

// ── Name table ──────────────────────────────────────────────────────────────

fn parse_name_table(pair: P<'_>) -> Result<Vec<String>, InktParseError> {
    let mut names = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::name_entry {
            let mut inner = entry.into_inner();
            let _index = inner.next(); // integer index (implied by position)
            let s = inner.next().ok_or_else(|| InktParseError {
                message: "expected string in name_entry".into(),
                line: 0,
                col: 0,
            })?;
            names.push(unescape_string(s.as_str()));
        }
    }
    Ok(names)
}

// ── Globals ─────────────────────────────────────────────────────────────────

fn parse_globals(pair: P<'_>) -> Result<Vec<GlobalVarDef>, InktParseError> {
    let mut vars = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::global_entry {
            vars.push(parse_global_entry(entry)?);
        }
    }
    Ok(vars)
}

fn parse_global_entry(pair: P<'_>) -> Result<GlobalVarDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in global".into(),
        line: 0,
        col: 0,
    })?)?;

    let type_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected type_name in global".into(),
        line: 0,
        col: 0,
    })?;
    let value_type = parse_value_type(type_pair)?;

    let value_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected value in global".into(),
        line: 0,
        col: 0,
    })?;
    let default_value = parse_value(value_pair, Some(value_type))?;

    let mut mutable = false;
    let mut name = NameId(0);

    for remaining in inner {
        match remaining.as_rule() {
            Rule::mutable_flag => mutable = true,
            Rule::integer => {
                name = NameId(parse_u16(&remaining)?);
            }
            _ => {}
        }
    }

    Ok(GlobalVarDef {
        id,
        name,
        value_type,
        default_value,
        mutable,
    })
}

#[expect(clippy::needless_pass_by_value)]
fn parse_value_type(pair: P<'_>) -> Result<ValueType, InktParseError> {
    let s = pair.as_str();
    match s {
        "int" => Ok(ValueType::Int),
        "float" => Ok(ValueType::Float),
        "bool" => Ok(ValueType::Bool),
        "string" => Ok(ValueType::String),
        "list" => Ok(ValueType::List),
        "divert_target" => Ok(ValueType::DivertTarget),
        "var_pointer" => Ok(ValueType::VariablePointer),
        "temp_pointer" => Ok(ValueType::TempPointer),
        "fragment_ref" => Ok(ValueType::FragmentRef),
        "null" => Ok(ValueType::Null),
        _ => Err(err(&pair, format!("unknown value type: {s}"))),
    }
}

fn parse_value(pair: P<'_>, type_hint: Option<ValueType>) -> Result<Value, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty value".into(),
        line: 0,
        col: 0,
    })?;

    match inner.as_rule() {
        Rule::integer => {
            // Use the type hint to disambiguate: an integer literal can represent
            // Int, Float, or Bool depending on the declared value_type.
            match type_hint {
                Some(ValueType::Float) => {
                    let n: f32 = inner
                        .as_str()
                        .parse()
                        .map_err(|_| err(&inner, "invalid float"))?;
                    Ok(Value::Float(n))
                }
                Some(ValueType::Bool) => {
                    let n: i32 = inner
                        .as_str()
                        .parse()
                        .map_err(|_| err(&inner, "invalid integer"))?;
                    Ok(Value::Bool(n != 0))
                }
                _ => {
                    let n: i32 = inner
                        .as_str()
                        .parse()
                        .map_err(|_| err(&inner, "invalid integer"))?;
                    Ok(Value::Int(n))
                }
            }
        }
        Rule::float => {
            let n: f32 = inner
                .as_str()
                .parse()
                .map_err(|_| err(&inner, "invalid float"))?;
            Ok(Value::Float(n))
        }
        Rule::bool_value => Ok(Value::Bool(inner.as_str() == "true")),
        Rule::string => Ok(Value::String(unescape_string(inner.as_str()).into())),
        Rule::def_id => Ok(Value::DivertTarget(parse_def_id(inner)?)),
        Rule::null_value => Ok(Value::Null),
        Rule::list_value => parse_list_value(inner),
        Rule::var_pointer_value => {
            let id_pair = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected def_id in var_pointer".into(),
                line: 0,
                col: 0,
            })?;
            Ok(Value::VariablePointer(parse_def_id(id_pair)?))
        }
        Rule::fragment_ref_value => {
            let idx_pair = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected integer in fragment_ref".into(),
                line: 0,
                col: 0,
            })?;
            let idx: u32 = idx_pair.as_str().parse().map_err(|_| InktParseError {
                message: "invalid fragment_ref index".into(),
                line: 0,
                col: 0,
            })?;
            Ok(Value::FragmentRef(idx))
        }
        _ => Err(err(
            &inner,
            format!("unexpected value rule: {:?}", inner.as_rule()),
        )),
    }
}

fn parse_list_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let mut items = Vec::new();
    let mut origins = Vec::new();

    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::list_value_items => {
                for def_pair in child.into_inner() {
                    if def_pair.as_rule() == Rule::def_id {
                        items.push(parse_def_id(def_pair)?);
                    }
                }
            }
            Rule::list_value_origins => {
                for def_pair in child.into_inner() {
                    if def_pair.as_rule() == Rule::def_id {
                        origins.push(parse_def_id(def_pair)?);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Value::List(ListValue { items, origins }.into()))
}

// ── Lists ───────────────────────────────────────────────────────────────────

fn parse_lists(pair: P<'_>) -> Result<Vec<ListDef>, InktParseError> {
    let mut defs = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::list_entry {
            defs.push(parse_list_entry(entry)?);
        }
    }
    Ok(defs)
}

fn parse_list_entry(pair: P<'_>) -> Result<ListDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in list".into(),
        line: 0,
        col: 0,
    })?)?;

    let name_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected name integer in list".into(),
        line: 0,
        col: 0,
    })?;
    let name = NameId(parse_u16(&name_int)?);

    let mut items = Vec::new();
    for remaining in inner {
        if remaining.as_rule() == Rule::list_item_inline {
            let mut li_inner = remaining.into_inner();
            let item_name_id = parse_u16(&li_inner.next().ok_or_else(|| InktParseError {
                message: "expected name in list item".into(),
                line: 0,
                col: 0,
            })?)?;
            let ordinal: i32 = li_inner
                .next()
                .ok_or_else(|| InktParseError {
                    message: "expected ordinal in list item".into(),
                    line: 0,
                    col: 0,
                })?
                .as_str()
                .parse()
                .map_err(|_| InktParseError {
                    message: "invalid ordinal".into(),
                    line: 0,
                    col: 0,
                })?;
            items.push((NameId(item_name_id), ordinal));
        }
    }

    Ok(ListDef { id, name, items })
}

// ── List items ──────────────────────────────────────────────────────────────

fn parse_list_items(pair: P<'_>) -> Result<Vec<ListItemDef>, InktParseError> {
    let mut items = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::list_item_entry {
            items.push(parse_list_item_entry(entry)?);
        }
    }
    Ok(items)
}

fn parse_list_item_entry(pair: P<'_>) -> Result<ListItemDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(next_rule(&mut inner, Rule::def_id, "list_item id")?)?;
    let origin = parse_def_id(next_rule(&mut inner, Rule::def_id, "list_item origin")?)?;
    let ordinal: i32 = next_rule(&mut inner, Rule::integer, "list_item ordinal")?
        .as_str()
        .parse()
        .map_err(|_| InktParseError {
            message: "invalid ordinal".into(),
            line: 0,
            col: 0,
        })?;
    let name_val = next_rule(&mut inner, Rule::integer, "list_item name")
        .map(|p| parse_u16(&p))
        .unwrap_or(Ok(0))?;
    Ok(ListItemDef {
        id,
        origin,
        ordinal,
        name: NameId(name_val),
    })
}

// ── Externals ───────────────────────────────────────────────────────────────

fn parse_externals(pair: P<'_>) -> Result<Vec<ExternalFnDef>, InktParseError> {
    let mut exts = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::extern_entry {
            exts.push(parse_extern_entry(entry)?);
        }
    }
    Ok(exts)
}

fn parse_extern_entry(pair: P<'_>) -> Result<ExternalFnDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in extern".into(),
        line: 0,
        col: 0,
    })?)?;

    let argc_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected argc in extern".into(),
        line: 0,
        col: 0,
    })?;
    let arg_count: u8 = argc_pair
        .as_str()
        .parse()
        .map_err(|_| err(&argc_pair, "invalid argc"))?;

    let name_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected name in extern".into(),
        line: 0,
        col: 0,
    })?;
    let name = NameId(parse_u16(&name_int)?);

    let mut fallback = None;
    for remaining in inner {
        if remaining.as_rule() == Rule::fallback {
            let fb_inner = remaining
                .into_inner()
                .next()
                .ok_or_else(|| InktParseError {
                    message: "expected def_id in fallback".into(),
                    line: 0,
                    col: 0,
                })?;
            fallback = Some(parse_def_id(fb_inner)?);
        }
    }

    Ok(ExternalFnDef {
        id,
        name,
        arg_count,
        fallback,
    })
}

// ── Addresses ───────────────────────────────────────────────────────────────

fn parse_addresses(pair: P<'_>) -> Result<Vec<AddressDef>, InktParseError> {
    let mut addresses = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::address_entry {
            addresses.push(parse_address_entry(entry)?);
        }
    }
    Ok(addresses)
}

fn parse_address_entry(pair: P<'_>) -> Result<AddressDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in address".into(),
        line: 0,
        col: 0,
    })?)?;
    let container_id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected container_id in address".into(),
        line: 0,
        col: 0,
    })?)?;
    let offset_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected byte_offset in address".into(),
        line: 0,
        col: 0,
    })?;
    let byte_offset: u32 = offset_pair
        .as_str()
        .parse()
        .map_err(|_| err(&offset_pair, "invalid byte_offset"))?;
    Ok(AddressDef {
        id,
        container_id,
        byte_offset,
    })
}

// ── List literals ────────────────────────────────────────────────────────────

fn parse_list_literals(pair: P<'_>) -> Result<Vec<ListValue>, InktParseError> {
    let mut literals = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::list_literal_entry {
            literals.push(parse_list_literal_entry(entry)?);
        }
    }
    Ok(literals)
}

fn parse_list_literal_entry(pair: P<'_>) -> Result<ListValue, InktParseError> {
    let mut items = Vec::new();
    let mut origins = Vec::new();

    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::list_value_items => {
                for def_pair in child.into_inner() {
                    if def_pair.as_rule() == Rule::def_id {
                        items.push(parse_def_id(def_pair)?);
                    }
                }
            }
            Rule::list_value_origins => {
                for def_pair in child.into_inner() {
                    if def_pair.as_rule() == Rule::def_id {
                        origins.push(parse_def_id(def_pair)?);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(ListValue { items, origins })
}

// ── Containers ──────────────────────────────────────────────────────────────

fn parse_container(pair: P<'_>) -> Result<(ContainerDef, ScopeLineTable), InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in container".into(),
        line: 0,
        col: 0,
    })?)?;

    let mut counting_flags = CountingFlags::empty();
    let mut path_hash = 0i32;
    let mut lines = Vec::new();
    let mut bytecode = Vec::new();
    let mut name: Option<NameId> = None;

    let mut scope_id = id;

    for child in inner {
        match child.as_rule() {
            Rule::scope_field => {
                let scope_pair = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected def_id in scope".into(),
                    line: 0,
                    col: 0,
                })?;
                scope_id = parse_def_id(scope_pair)?;
            }
            Rule::container_name_field => {
                let val = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected integer in container name".into(),
                    line: 0,
                    col: 0,
                })?;
                name = Some(NameId(parse_u16(&val)?));
            }
            Rule::flags_field => {
                for flag in child.into_inner() {
                    if flag.as_rule() == Rule::flag_name {
                        match flag.as_str() {
                            "visits" => counting_flags |= CountingFlags::VISITS,
                            "turns" => counting_flags |= CountingFlags::TURNS,
                            "start_only" => counting_flags |= CountingFlags::COUNT_START_ONLY,
                            _ => {}
                        }
                    }
                }
            }
            Rule::path_hash_field => {
                let val = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected integer in path_hash".into(),
                    line: 0,
                    col: 0,
                })?;
                path_hash = val.as_str().parse().map_err(|_| InktParseError {
                    message: "invalid path_hash integer".into(),
                    line: 0,
                    col: 0,
                })?;
            }
            Rule::lines_field => {
                lines = parse_lines_field(child)?;
            }
            Rule::code_field => {
                bytecode = parse_code_field(child)?;
            }
            _ => {}
        }
    }

    let container = ContainerDef {
        id,
        scope_id,
        name,
        bytecode,
        counting_flags,
        path_hash,
    };
    let line_table = ScopeLineTable { scope_id, lines };
    Ok((container, line_table))
}

fn parse_lines_field(pair: P<'_>) -> Result<Vec<LineEntry>, InktParseError> {
    let mut entries = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::line_entry {
            entries.push(parse_line_entry(entry)?);
        }
    }
    Ok(entries)
}

fn parse_line_entry(pair: P<'_>) -> Result<LineEntry, InktParseError> {
    let mut inner = pair.into_inner();
    let _index = inner.next(); // integer index (implied by position)
    let content_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected line content".into(),
        line: 0,
        col: 0,
    })?;
    let content = parse_line_content(content_pair)?;
    let hash_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected source_hash".into(),
        line: 0,
        col: 0,
    })?;
    // source_hash is @HHHHHHHHHHHHHHHH
    let hash_str = hash_pair.as_str();
    let source_hash = parse_hex_u64(&format!("0x{}", &hash_str[1..]))?;

    let mut audio_ref = None;
    let mut slot_info = Vec::new();
    let mut source_location = None;

    for remaining in inner {
        match remaining.as_rule() {
            Rule::audio_field => {
                let s = remaining
                    .into_inner()
                    .next()
                    .ok_or_else(|| InktParseError {
                        message: "expected audio string".into(),
                        line: 0,
                        col: 0,
                    })?;
                audio_ref = Some(unescape_string(s.as_str()));
            }
            Rule::slots_field => {
                for slot_entry in remaining.into_inner() {
                    if slot_entry.as_rule() == Rule::slot_entry {
                        let mut parts = slot_entry.into_inner();
                        let idx_str = parts.next().map_or("0", |p| p.as_str());
                        let idx: u8 = idx_str.parse().unwrap_or(0);
                        let name_str = parts
                            .next()
                            .map_or_else(String::new, |p| unescape_string(p.as_str()));
                        slot_info.push(SlotInfo {
                            index: idx,
                            name: name_str,
                        });
                    }
                }
            }
            Rule::source_field => {
                let mut parts = remaining.into_inner();
                let file = parts
                    .next()
                    .map_or_else(String::new, |p| unescape_string(p.as_str()));
                let start: u32 = parts
                    .next()
                    .and_then(|p| p.as_str().parse().ok())
                    .unwrap_or(0);
                let end: u32 = parts
                    .next()
                    .and_then(|p| p.as_str().parse().ok())
                    .unwrap_or(0);
                source_location = Some(SourceLocation {
                    file,
                    range_start: start,
                    range_end: end,
                });
            }
            _ => {}
        }
    }

    let flags = crate::LineFlags::from_content(&content);
    Ok(LineEntry {
        content,
        flags,
        source_hash,
        audio_ref,
        slot_info,
        source_location,
    })
}

fn parse_line_content(pair: P<'_>) -> Result<LineContent, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty line content".into(),
        line: 0,
        col: 0,
    })?;
    match inner.as_rule() {
        Rule::string => Ok(LineContent::Plain(unescape_string(inner.as_str()))),
        Rule::template => parse_template(inner),
        _ => Err(err(
            &inner,
            format!("unexpected line content: {:?}", inner.as_rule()),
        )),
    }
}

fn parse_template(pair: P<'_>) -> Result<LineContent, InktParseError> {
    let mut parts = Vec::new();
    for child in pair.into_inner() {
        if child.as_rule() == Rule::template_part {
            parts.push(parse_template_part(child)?);
        }
    }
    Ok(LineContent::Template(parts))
}

fn parse_template_part(pair: P<'_>) -> Result<LinePart, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty template part".into(),
        line: 0,
        col: 0,
    })?;
    match inner.as_rule() {
        Rule::literal_part => {
            let s = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected string in literal".into(),
                line: 0,
                col: 0,
            })?;
            Ok(LinePart::Literal(unescape_string(s.as_str())))
        }
        Rule::slot_part => {
            let idx = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected integer in slot".into(),
                line: 0,
                col: 0,
            })?;
            let n: u8 = idx
                .as_str()
                .parse()
                .map_err(|_| err(&idx, "invalid slot index"))?;
            Ok(LinePart::Slot(n))
        }
        Rule::select_part => parse_select_part(inner),
        _ => Err(err(
            &inner,
            format!("unexpected template part: {:?}", inner.as_rule()),
        )),
    }
}

fn parse_select_part(pair: P<'_>) -> Result<LinePart, InktParseError> {
    let mut inner = pair.into_inner();
    let slot_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected slot in select".into(),
        line: 0,
        col: 0,
    })?;
    let slot: u8 = slot_pair
        .as_str()
        .parse()
        .map_err(|_| err(&slot_pair, "invalid slot"))?;

    let mut variants = Vec::new();
    let mut default = String::new();

    for child in inner {
        match child.as_rule() {
            Rule::select_variant => {
                let mut vi = child.into_inner();
                let key_pair = vi.next().ok_or_else(|| InktParseError {
                    message: "expected key in variant".into(),
                    line: 0,
                    col: 0,
                })?;
                let key = parse_select_key(key_pair)?;
                let text = vi.next().ok_or_else(|| InktParseError {
                    message: "expected text in variant".into(),
                    line: 0,
                    col: 0,
                })?;
                variants.push((key, unescape_string(text.as_str())));
            }
            Rule::select_default => {
                let s = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected string in default".into(),
                    line: 0,
                    col: 0,
                })?;
                default = unescape_string(s.as_str());
            }
            _ => {}
        }
    }

    Ok(LinePart::Select {
        slot,
        variants,
        default,
    })
}

fn parse_select_key(pair: P<'_>) -> Result<SelectKey, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty select key".into(),
        line: 0,
        col: 0,
    })?;
    match inner.as_rule() {
        Rule::cardinal_key => {
            let cat = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected plural_cat".into(),
                line: 0,
                col: 0,
            })?;
            Ok(SelectKey::Cardinal(parse_plural_cat(cat)?))
        }
        Rule::ordinal_key => {
            let cat = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected plural_cat".into(),
                line: 0,
                col: 0,
            })?;
            Ok(SelectKey::Ordinal(parse_plural_cat(cat)?))
        }
        Rule::exact_key => {
            let n = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected integer".into(),
                line: 0,
                col: 0,
            })?;
            let v: i32 = n
                .as_str()
                .parse()
                .map_err(|_| err(&n, "invalid exact key"))?;
            Ok(SelectKey::Exact(v))
        }
        Rule::keyword_key => {
            let ident = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected ident".into(),
                line: 0,
                col: 0,
            })?;
            Ok(SelectKey::Keyword(ident.as_str().to_owned()))
        }
        _ => Err(err(
            &inner,
            format!("unexpected select key: {:?}", inner.as_rule()),
        )),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn parse_plural_cat(pair: P<'_>) -> Result<PluralCategory, InktParseError> {
    match pair.as_str() {
        "Zero" => Ok(PluralCategory::Zero),
        "One" => Ok(PluralCategory::One),
        "Two" => Ok(PluralCategory::Two),
        "Few" => Ok(PluralCategory::Few),
        "Many" => Ok(PluralCategory::Many),
        "Other" => Ok(PluralCategory::Other),
        _ => Err(err(
            &pair,
            format!("unknown plural category: {}", pair.as_str()),
        )),
    }
}

// ── Code field ──────────────────────────────────────────────────────────────

fn parse_code_field(pair: P<'_>) -> Result<Vec<u8>, InktParseError> {
    let mut bytecode = Vec::new();
    for child in pair.into_inner() {
        if child.as_rule() == Rule::instruction {
            let op = parse_instruction(child)?;
            op.encode(&mut bytecode);
        }
    }
    Ok(bytecode)
}

#[expect(clippy::too_many_lines)]
fn parse_instruction(pair: P<'_>) -> Result<Opcode, InktParseError> {
    let mut inner = pair.into_inner();
    let mnemonic_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected opcode mnemonic".into(),
        line: 0,
        col: 0,
    })?;
    let mnemonic = mnemonic_pair.as_str();

    let operands: Vec<P<'_>> = inner.collect();

    match mnemonic {
        // Stack & literals
        "push_int" => Ok(Opcode::PushInt(parse_operand_i32(&operands, 0, mnemonic)?)),
        "push_float" => Ok(Opcode::PushFloat(parse_operand_f32(
            &operands, 0, mnemonic,
        )?)),
        "push_bool" => {
            let s = operand_str(&operands, 0, mnemonic)?;
            Ok(Opcode::PushBool(s == "true"))
        }
        "push_string" => Ok(Opcode::PushString(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "push_list" => Ok(Opcode::PushList(parse_operand_u16(&operands, 0, mnemonic)?)),
        "push_divert_target" => Ok(Opcode::PushDivertTarget(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "push_null" => Ok(Opcode::PushNull),
        "pop" => Ok(Opcode::Pop),
        "duplicate" => Ok(Opcode::Duplicate),

        // Arithmetic
        "add" => Ok(Opcode::Add),
        "subtract" => Ok(Opcode::Subtract),
        "multiply" => Ok(Opcode::Multiply),
        "divide" => Ok(Opcode::Divide),
        "modulo" => Ok(Opcode::Modulo),
        "negate" => Ok(Opcode::Negate),

        // Comparison
        "equal" => Ok(Opcode::Equal),
        "not_equal" => Ok(Opcode::NotEqual),
        "greater" => Ok(Opcode::Greater),
        "greater_or_equal" => Ok(Opcode::GreaterOrEqual),
        "less" => Ok(Opcode::Less),
        "less_or_equal" => Ok(Opcode::LessOrEqual),

        // Logic
        "not" => Ok(Opcode::Not),
        "and" => Ok(Opcode::And),
        "or" => Ok(Opcode::Or),

        // Global vars
        "get_global" => Ok(Opcode::GetGlobal(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "set_global" => Ok(Opcode::SetGlobal(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),

        // Temp vars
        "declare_temp" => Ok(Opcode::DeclareTemp(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "get_temp" => Ok(Opcode::GetTemp(parse_operand_u16(&operands, 0, mnemonic)?)),
        "set_temp" => Ok(Opcode::SetTemp(parse_operand_u16(&operands, 0, mnemonic)?)),
        "get_temp_raw" => Ok(Opcode::GetTempRaw(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),

        // Variable pointers
        "push_var_pointer" => Ok(Opcode::PushVarPointer(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "push_temp_pointer" => Ok(Opcode::PushTempPointer(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),

        // Control flow
        "jump" => Ok(Opcode::Jump(parse_operand_i32(&operands, 0, mnemonic)?)),
        "jump_if_false" => Ok(Opcode::JumpIfFalse(parse_operand_i32(
            &operands, 0, mnemonic,
        )?)),
        "goto" => Ok(Opcode::Goto(parse_operand_def_id(&operands, 0, mnemonic)?)),
        "goto_if" => Ok(Opcode::GotoIf(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "goto_variable" => Ok(Opcode::GotoVariable),

        // Container flow
        "enter_container" => Ok(Opcode::EnterContainer(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "exit_container" => Ok(Opcode::ExitContainer),

        // Functions / tunnels
        "call" => Ok(Opcode::Call(parse_operand_def_id(&operands, 0, mnemonic)?)),
        "return" => Ok(Opcode::Return),
        "tunnel_call" => Ok(Opcode::TunnelCall(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "tunnel_return" => Ok(Opcode::TunnelReturn),
        "tunnel_call_variable" => Ok(Opcode::TunnelCallVariable),
        "call_variable" => Ok(Opcode::CallVariable),

        // Threads
        "thread_call" => Ok(Opcode::ThreadCall(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "thread_start" => Ok(Opcode::ThreadStart),
        "thread_done" => Ok(Opcode::ThreadDone),

        // Output
        "emit_line" => {
            let idx = parse_operand_u16(&operands, 0, mnemonic)?;
            let slots = parse_operand_u8(&operands, 1, mnemonic)?;
            Ok(Opcode::EmitLine(idx, slots))
        }
        "emit_value" => Ok(Opcode::EmitValue),
        "emit_newline" => Ok(Opcode::EmitNewline),
        "spring" => Ok(Opcode::Spring),
        "glue" => Ok(Opcode::Glue),
        "begin_tag" => Ok(Opcode::BeginTag),
        "end_tag" => Ok(Opcode::EndTag),
        "eval_line" => {
            let idx = parse_operand_u16(&operands, 0, mnemonic)?;
            let slots = parse_operand_u8(&operands, 1, mnemonic)?;
            Ok(Opcode::EvalLine(idx, slots))
        }

        // Choices
        "begin_choice" => {
            let flags = parse_choice_flags_operand(&operands, 0, mnemonic)?;
            let target = parse_operand_def_id(&operands, 1, mnemonic)?;
            Ok(Opcode::BeginChoice(flags, target))
        }
        "end_choice" => Ok(Opcode::EndChoice),

        // Sequences
        "sequence" => {
            let kind_str = operand_str(&operands, 0, mnemonic)?;
            let kind = match kind_str {
                "cycle" => SequenceKind::Cycle,
                "stopping" => SequenceKind::Stopping,
                "once_only" => SequenceKind::OnceOnly,
                "shuffle" => SequenceKind::Shuffle,
                _ => {
                    return Err(InktParseError {
                        message: format!("unknown sequence kind: {kind_str}"),
                        line: 0,
                        col: 0,
                    });
                }
            };
            let count: u8 =
                operand_str(&operands, 1, mnemonic)?
                    .parse()
                    .map_err(|_| InktParseError {
                        message: "invalid sequence count".into(),
                        line: 0,
                        col: 0,
                    })?;
            Ok(Opcode::Sequence(kind, count))
        }
        "sequence_branch" => Ok(Opcode::SequenceBranch(parse_operand_i32(
            &operands, 0, mnemonic,
        )?)),

        // Intrinsics
        "visit_count" => Ok(Opcode::VisitCount),
        "current_visit_count" => Ok(Opcode::CurrentVisitCount),
        "turns_since" => Ok(Opcode::TurnsSince),
        "turn_index" => Ok(Opcode::TurnIndex),
        "choice_count" => Ok(Opcode::ChoiceCount),
        "random" => Ok(Opcode::Random),
        "seed_random" => Ok(Opcode::SeedRandom),

        // Casts / math
        "cast_to_int" => Ok(Opcode::CastToInt),
        "cast_to_float" => Ok(Opcode::CastToFloat),
        "floor" => Ok(Opcode::Floor),
        "ceiling" => Ok(Opcode::Ceiling),
        "pow" => Ok(Opcode::Pow),
        "min" => Ok(Opcode::Min),
        "max" => Ok(Opcode::Max),

        // External fns
        "call_external" => {
            let id = parse_operand_def_id(&operands, 0, mnemonic)?;
            // "argc=N" is parsed as a kv_operand. Extract the value after "=".
            let kv_str = operand_str(&operands, 1, mnemonic)?;
            let argc_str = kv_str.strip_prefix("argc=").unwrap_or(kv_str);
            let argc: u8 = argc_str.parse().map_err(|_| InktParseError {
                message: format!("invalid argc in call_external: {kv_str}"),
                line: 0,
                col: 0,
            })?;
            Ok(Opcode::CallExternal(id, argc))
        }

        // List ops
        "list_contains" => Ok(Opcode::ListContains),
        "list_not_contains" => Ok(Opcode::ListNotContains),
        "list_intersect" => Ok(Opcode::ListIntersect),
        "list_all" => Ok(Opcode::ListAll),
        "list_invert" => Ok(Opcode::ListInvert),
        "list_count" => Ok(Opcode::ListCount),
        "list_min" => Ok(Opcode::ListMin),
        "list_max" => Ok(Opcode::ListMax),
        "list_value" => Ok(Opcode::ListValue),
        "list_range" => Ok(Opcode::ListRange),
        "list_from_int" => Ok(Opcode::ListFromInt),
        "list_random" => Ok(Opcode::ListRandom),

        // Lifecycle
        "done" => Ok(Opcode::Done),
        "yield" => Ok(Opcode::Yield),
        "end" => Ok(Opcode::End),
        "nop" => Ok(Opcode::Nop),

        // String eval
        "begin_string_eval" => Ok(Opcode::BeginStringEval),
        "end_string_eval" => Ok(Opcode::EndStringEval),

        // Fragment capture
        "begin_fragment" => Ok(Opcode::BeginFragment),
        "end_fragment" => Ok(Opcode::EndFragment),

        // Debug
        "source_location" => {
            // Written as "source_location LINE:COL" — parsed as source_loc operand
            let s = operand_str(&operands, 0, mnemonic)?;
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() != 2 {
                return Err(InktParseError {
                    message: format!("invalid source_location: {s}"),
                    line: 0,
                    col: 0,
                });
            }
            let line: u32 = parts[0].parse().map_err(|_| InktParseError {
                message: "invalid line".into(),
                line: 0,
                col: 0,
            })?;
            let col: u32 = parts[1].parse().map_err(|_| InktParseError {
                message: "invalid col".into(),
                line: 0,
                col: 0,
            })?;
            Ok(Opcode::SourceLocation(line, col))
        }

        _ => Err(InktParseError {
            message: format!("unknown opcode: {mnemonic}"),
            line: mnemonic_pair.line_col().0,
            col: mnemonic_pair.line_col().1,
        }),
    }
}

fn parse_choice_flags_operand(
    operands: &[P<'_>],
    idx: usize,
    context: &str,
) -> Result<ChoiceFlags, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    let mut flags = ChoiceFlags {
        has_condition: false,
        has_start_content: false,
        has_choice_only_content: false,
        once_only: false,
        is_invisible_default: false,
    };
    if s == "none" {
        return Ok(flags);
    }
    for part in s.split('+') {
        match part {
            "cond" => flags.has_condition = true,
            "start" => flags.has_start_content = true,
            "choice_only" => flags.has_choice_only_content = true,
            "once" => flags.once_only = true,
            "invis_default" => flags.is_invisible_default = true,
            _ => {
                return Err(InktParseError {
                    message: format!("unknown choice flag: {part}"),
                    line: 0,
                    col: 0,
                });
            }
        }
    }
    Ok(flags)
}

// ── Operand helpers ─────────────────────────────────────────────────────────

fn operand_str<'a>(
    operands: &'a [P<'_>],
    idx: usize,
    context: &str,
) -> Result<&'a str, InktParseError> {
    let op = operands.get(idx).ok_or_else(|| InktParseError {
        message: format!("missing operand {idx} for {context}"),
        line: 0,
        col: 0,
    })?;
    // The operand rule wraps the actual value. Get the inner pair.
    let inner = op.clone().into_inner().next();
    match inner {
        Some(p) => Ok(p.as_str()),
        None => Ok(op.as_str()),
    }
}

fn parse_operand_i32(operands: &[P<'_>], idx: usize, context: &str) -> Result<i32, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid i32 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_f32(operands: &[P<'_>], idx: usize, context: &str) -> Result<f32, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid f32 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_u8(operands: &[P<'_>], idx: usize, context: &str) -> Result<u8, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid u8 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_u16(operands: &[P<'_>], idx: usize, context: &str) -> Result<u16, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid u16 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_def_id(
    operands: &[P<'_>],
    idx: usize,
    context: &str,
) -> Result<DefinitionId, InktParseError> {
    let op = operands.get(idx).ok_or_else(|| InktParseError {
        message: format!("missing operand {idx} for {context}"),
        line: 0,
        col: 0,
    })?;
    // Drill into the operand to get the def_id inner pair
    let inner = op.clone().into_inner().next().unwrap_or_else(|| op.clone());
    parse_def_id(inner)
}

// ── Shared parse helpers ────────────────────────────────────────────────────

#[expect(clippy::needless_pass_by_value)]
fn parse_def_id(pair: P<'_>) -> Result<DefinitionId, InktParseError> {
    let s = pair.as_str();
    // Format: $TT_HHHHHHHHHHHHHH
    if !s.starts_with('$') || s.len() < 4 {
        return Err(err(&pair, format!("invalid def_id: {s}")));
    }
    let tag_str = &s[1..3];
    let hash_str = &s[4..]; // skip $TT_

    let tag_byte = u8::from_str_radix(tag_str, 16)
        .map_err(|_| err(&pair, format!("invalid tag: {tag_str}")))?;
    let hash = u64::from_str_radix(hash_str, 16)
        .map_err(|_| err(&pair, format!("invalid hash: {hash_str}")))?;

    let tag = crate::id::DefinitionTag::from_u8(tag_byte)
        .ok_or_else(|| err(&pair, format!("unknown tag byte: {tag_byte:#04x}")))?;

    Ok(DefinitionId::new(tag, hash))
}

fn parse_hex_u32(s: &str) -> u32 {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(hex, 16).unwrap_or(0)
}

fn parse_hex_u64(s: &str) -> Result<u64, InktParseError> {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(hex, 16).map_err(|_| InktParseError {
        message: format!("invalid hex: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_u16(pair: &P<'_>) -> Result<u16, InktParseError> {
    pair.as_str().parse().map_err(|_| err(pair, "invalid u16"))
}

fn unescape_string(s: &str) -> String {
    // Strip surrounding quotes
    let inner = &s[1..s.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') | None => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn next_rule<'a>(
    iter: &mut impl Iterator<Item = P<'a>>,
    expected: Rule,
    context: &str,
) -> Result<P<'a>, InktParseError> {
    for pair in iter.by_ref() {
        if pair.as_rule() == expected {
            return Ok(pair);
        }
    }
    Err(InktParseError {
        message: format!("expected {expected:?} in {context}"),
        line: 0,
        col: 0,
    })
}
