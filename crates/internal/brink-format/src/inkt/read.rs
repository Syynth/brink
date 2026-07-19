//! Pest-based reader for the `.inkt` textual format.

use pest::Parser;
use pest_derive::Parser;

use crate::counting::CountingFlags;
use crate::definition::{
    AddressDef, AddressPath, AliasEntry, CallAtom, CapabilityParam, ContainerDef, DirectEffects,
    DispatchEntry, EffectRowEntry, ExternalFnDef, FrameShapeDef, GlobalVarDef, LineEntry, ListDef,
    ListItemDef, ParamMeta, ScopeLineTable, SlotInfo, SourceLocation, StructShapeDef,
};
use crate::id::{DefinitionId, NameId};
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};
use crate::opcode::{ChoiceFlags, Opcode, SequenceKind};
use crate::story::StoryData;
use crate::value::{
    ClosureEnvEntry, ListValue, MapKey, OrderedMap, ProjSegment, ShapeId, Value, ValueType,
};

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
    // Fuzz-found (#1102): a `.inkt` document declaring the same container
    // address twice is malformed input and must be rejected at read time.
    // Accepting it poisons the roundtrip downstream: `write_inkt` collapses
    // line tables through a `scope_id`-keyed `HashMap`, so the later
    // duplicate's lines silently replace the earlier one's on the next write
    // (same admission-check posture as the duplicate map key rejection, #985).
    let mut seen_container_ids = std::collections::HashSet::new();
    let mut variables = Vec::new();
    let mut list_defs = Vec::new();
    let mut list_items = Vec::new();
    let mut externals = Vec::new();
    let mut addresses = Vec::new();
    let mut address_paths = Vec::new();
    let mut containers = Vec::new();
    let mut line_tables = Vec::new();
    let mut list_literals = Vec::new();
    let mut literal_pool = Vec::new();
    let mut private_defs = Vec::new();
    let mut alias_table = Vec::new();
    let mut effect_rows = Vec::new();
    let mut frame_shapes = Vec::new();
    let mut struct_shapes = Vec::new();
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
            Rule::address_paths => address_paths = parse_address_paths(inner)?,
            Rule::list_literals => list_literals = parse_list_literals(inner)?,
            Rule::literal_pool => literal_pool = parse_literal_pool(inner)?,
            Rule::struct_shapes => struct_shapes = parse_struct_shapes(inner)?,
            Rule::visibility => private_defs = parse_visibility(inner)?,
            Rule::alias_table => alias_table = parse_alias_table(inner)?,
            Rule::effect_rows => effect_rows = parse_effect_rows(inner)?,
            Rule::frame_shapes => frame_shapes = parse_frame_shapes(inner)?,
            Rule::container => {
                let (line, col) = inner.line_col();
                let (container, lt) = parse_container(inner)?;
                if !seen_container_ids.insert(container.id) {
                    return Err(InktParseError {
                        message: format!("duplicate container address: {}", container.id),
                        line,
                        col,
                    });
                }
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
        address_paths,
        name_table,
        list_literals,
        literal_pool,
        struct_shapes,
        private_defs,
        alias_table,
        effect_rows,
        frame_shapes,
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
    let mut local = false;
    let mut name = NameId(0);

    for remaining in inner {
        match remaining.as_rule() {
            Rule::mutable_flag => mutable = true,
            Rule::local_flag => local = true,
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
        local,
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
        "array" => Ok(ValueType::Array),
        "map" => Ok(ValueType::Map),
        "handle" => Ok(ValueType::Handle),
        "record" => Ok(ValueType::Record),
        "fn_ref" => Ok(ValueType::FnRef),
        "closure" => Ok(ValueType::Closure),
        "projection" => Ok(ValueType::Projection),
        "option" => Ok(ValueType::Option),
        "range" => Ok(ValueType::Range),
        "vec2" => Ok(ValueType::Vec2),
        "vec3" => Ok(ValueType::Vec3),
        "vec4" => Ok(ValueType::Vec4),
        "quat" => Ok(ValueType::Quat),
        "mat2" => Ok(ValueType::Mat2),
        "mat3" => Ok(ValueType::Mat3),
        "mat4" => Ok(ValueType::Mat4),
        "weighted" => Ok(ValueType::Weighted),
        _ => Err(err(&pair, format!("unknown value type: {s}"))),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one match arm per value rule — the NS-A1 option_value arm pushed this past 100"
)]
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
        Rule::array_value => {
            let mut items = Vec::new();
            for child in inner.into_inner() {
                items.push(parse_value(child, None)?);
            }
            Ok(Value::array(items))
        }
        Rule::map_value => parse_map_value(inner),
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
        // Handle values (T1d, `docs/t1d-spec.md` §2): `(handle <kind> <id>)`
        // — reader lands with the writer in this same PR (never repeat the
        // #742 write/read asymmetry).
        Rule::handle_value => parse_handle_value(inner),
        // T1e projection values (docs/t1e-spec.md §3) — reader lands with the
        // writer in this same PR (never repeat the #742 write/read asymmetry).
        Rule::projection_value => parse_projection_value(inner),
        // T1c function values (docs/t1c-spec.md §1/§6) — the read-side legs
        // paired with `write_value`'s `record`/`fn_ref`/`closure` atoms
        // (issue #742: writer/reader must stay in sync, dump-parity rule).
        Rule::record_value => parse_record_value(inner),
        // NS-A1 Option values (docs/stdlib-spec.md §1.4): `(some <value>)` /
        // `(option_none)` — read-side leg paired with `write_value`'s Option
        // atom (the #742 dump/reader parity lesson).
        Rule::option_value => {
            let variant = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "empty option value".into(),
                line: 0,
                col: 0,
            })?;
            match variant.as_rule() {
                Rule::some_value => {
                    let inner_value =
                        variant.into_inner().next().ok_or_else(|| InktParseError {
                            message: "expected value in some".into(),
                            line: 0,
                            col: 0,
                        })?;
                    Ok(Value::some(parse_value(inner_value, None)?))
                }
                Rule::none_value => Ok(Value::none()),
                other => Err(InktParseError {
                    message: format!("unexpected option variant: {other:?}"),
                    line: 0,
                    col: 0,
                }),
            }
        }
        // NS-A8 tower values (docs/tower-mini-spec.md T5): `(vec2 <x> <y>)`
        // … `(mat4 <16 lanes>)` — read-side leg paired with `write_value`'s
        // tower atoms (the #742 dump/reader parity lesson). Lanes rebuild
        // through glam's explicit array constructors.
        Rule::tower_value => parse_tower_value(inner),
        // NS-A7 weighted tables (docs/stdlib-spec.md §8): `(weighted
        // (<weight> <value>)+)` — read-side leg paired with `write_value`'s
        // Weighted atom (the #742 dump/reader parity lesson). The §8
        // evidence-by-construction invariant is enforced here: an empty
        // table or a non-positive weight is a parse error.
        Rule::weighted_value => parse_weighted_value(inner),
        // NS-A5 range values (docs/stdlib-spec.md §7, F7): `(range <start>
        // <end> incl|excl)` — read-side leg paired with `write_value`'s
        // Range atom (the #742 dump/reader parity lesson).
        Rule::range_value => {
            let mut parts = inner.into_inner();
            let mut next_part = |what: &str| {
                parts.next().ok_or_else(|| InktParseError {
                    message: format!("expected {what} in range"),
                    line: 0,
                    col: 0,
                })
            };
            let start_pair = next_part("start bound")?;
            let end_pair = next_part("end bound")?;
            let form_pair = next_part("incl/excl form")?;
            let start: i32 = start_pair
                .as_str()
                .trim()
                .parse()
                .map_err(|_| InktParseError {
                    message: format!("invalid range start: {}", start_pair.as_str()),
                    line: 0,
                    col: 0,
                })?;
            let end: i32 = end_pair
                .as_str()
                .trim()
                .parse()
                .map_err(|_| InktParseError {
                    message: format!("invalid range end: {}", end_pair.as_str()),
                    line: 0,
                    col: 0,
                })?;
            let inclusive = match form_pair.as_str().trim() {
                "incl" => true,
                "excl" => false,
                other => {
                    return Err(InktParseError {
                        message: format!("unexpected range form: {other}"),
                        line: 0,
                        col: 0,
                    });
                }
            };
            Ok(Value::range(start, end, inclusive))
        }
        Rule::fn_ref_value => {
            let id_pair = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected def_id in fn_ref".into(),
                line: 0,
                col: 0,
            })?;
            Ok(Value::FnRef(parse_def_id(id_pair)?))
        }
        Rule::closure_value => parse_closure_value(inner),
        _ => Err(err(
            &inner,
            format!("unexpected value rule: {:?}", inner.as_rule()),
        )),
    }
}

/// Parse a `handle_value` node (`"(" ~ "handle" ~ integer ~ integer ~ ")"`)
/// into a [`Value::handle`] (T1d, `docs/t1d-spec.md` §2).
fn parse_handle_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let mut parts = pair.into_inner();
    let kind_pair = parts.next().ok_or_else(|| InktParseError {
        message: "expected kind integer in handle".into(),
        line: 0,
        col: 0,
    })?;
    let kind = parse_u16(&kind_pair)?;
    let id_pair = parts.next().ok_or_else(|| InktParseError {
        message: "expected id integer in handle".into(),
        line: 0,
        col: 0,
    })?;
    let id: u64 = id_pair
        .as_str()
        .parse()
        .map_err(|_| err(&id_pair, "invalid handle id"))?;
    Ok(Value::handle(NameId(kind), id))
}

/// Parse a `tower_value` node (NS-A8, `docs/tower-mini-spec.md` T5): the
/// inner rule picks the kind, its children are the flat f32 lanes in the
/// pinned order (vec/quat `x y (z w)`; matrices column-major). The lane
/// count is enforced by the grammar; the glam value is rebuilt through the
/// explicit `from_array`/`from_cols_array` constructors.
fn parse_tower_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty tower value".into(),
        line: 0,
        col: 0,
    })?;
    let rule = inner.as_rule();
    let mut lanes = Vec::new();
    for lane_pair in inner.into_inner() {
        let n: f32 = lane_pair
            .as_str()
            .parse()
            .map_err(|_| err(&lane_pair, "invalid tower lane"))?;
        lanes.push(n);
    }
    let lane_array = |want: usize| -> Result<&[f32], InktParseError> {
        if lanes.len() == want {
            Ok(&lanes)
        } else {
            Err(InktParseError {
                message: format!("expected {want} tower lanes, got {}", lanes.len()),
                line: 0,
                col: 0,
            })
        }
    };
    match rule {
        Rule::vec2_value => {
            let l = lane_array(2)?;
            Ok(Value::Vec2(glam::Vec2::new(l[0], l[1])))
        }
        Rule::vec3_value => {
            let l = lane_array(3)?;
            Ok(Value::Vec3(glam::Vec3::new(l[0], l[1], l[2])))
        }
        Rule::vec4_value => {
            let l = lane_array(4)?;
            Ok(Value::Vec4(glam::Vec4::new(l[0], l[1], l[2], l[3])))
        }
        Rule::quat_value => {
            let l = lane_array(4)?;
            Ok(Value::Quat(glam::Quat::from_xyzw(l[0], l[1], l[2], l[3])))
        }
        Rule::mat2_value => {
            let mut cols = [0.0f32; 4];
            cols.copy_from_slice(lane_array(4)?);
            Ok(Value::Mat2(glam::Mat2::from_cols_array(&cols)))
        }
        Rule::mat3_value => {
            let mut cols = [0.0f32; 9];
            cols.copy_from_slice(lane_array(9)?);
            Ok(Value::Mat3(glam::Mat3::from_cols_array(&cols)))
        }
        Rule::mat4_value => {
            let mut cols = [0.0f32; 16];
            cols.copy_from_slice(lane_array(16)?);
            Ok(Value::Mat4(glam::Mat4::from_cols_array(&cols)))
        }
        other => Err(InktParseError {
            message: format!("unexpected tower variant: {other:?}"),
            line: 0,
            col: 0,
        }),
    }
}

/// Parse a `projection_value` node (`(projection <cell> (segments
/// <segment>…))`) into a [`Value::projection`] — the read-side leg paired
/// with `write_value`'s `(projection …)` atom (issue #742 discipline,
/// `docs/t1e-spec.md` §3).
fn parse_projection_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let mut children = pair.into_inner();
    let cell_pair = children.next().ok_or_else(|| InktParseError {
        message: "expected cell def_id in projection".into(),
        line: 0,
        col: 0,
    })?;
    let cell = parse_def_id(cell_pair)?;
    let segs_pair = children.next().ok_or_else(|| InktParseError {
        message: "expected segments in projection".into(),
        line: 0,
        col: 0,
    })?;
    let mut segments = Vec::new();
    for seg in segs_pair.into_inner() {
        // `proj_segment` wraps exactly one of `index_segment`/`key_segment`.
        let inner = seg.into_inner().next().ok_or_else(|| InktParseError {
            message: "empty projection segment".into(),
            line: 0,
            col: 0,
        })?;
        match inner.as_rule() {
            Rule::index_segment => {
                let n_pair = inner.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected integer in index segment".into(),
                    line: 0,
                    col: 0,
                })?;
                let n: i32 = n_pair
                    .as_str()
                    .parse()
                    .map_err(|_| err(&n_pair, "invalid index segment"))?;
                segments.push(ProjSegment::Index(n));
            }
            Rule::key_segment => {
                let v_pair = inner.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected value in key segment".into(),
                    line: 0,
                    col: 0,
                })?;
                segments.push(ProjSegment::Key(parse_value(v_pair, None)?));
            }
            other => {
                return Err(err(
                    &inner,
                    format!("unexpected projection segment rule: {other:?}"),
                ));
            }
        }
    }
    Ok(Value::projection(cell, segments))
}

/// Parse a `map_value` node (`(map (key value)…)`) into a [`Value::Map`].
fn parse_map_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let mut map = OrderedMap::new();
    for entry in pair.into_inner() {
        if entry.as_rule() != Rule::map_entry {
            continue;
        }
        let mut children = entry.into_inner();
        let key_pair = children.next().ok_or_else(|| InktParseError {
            message: "expected map key".into(),
            line: 0,
            col: 0,
        })?;
        let value_pair = children.next().ok_or_else(|| InktParseError {
            message: "expected map value".into(),
            line: 0,
            col: 0,
        })?;
        let (line, col) = key_pair.line_col();
        let key = parse_map_key(key_pair)?;
        let value = parse_value(value_pair, None)?;
        // A repeated key would violate the content-based `OrderedMap` `Eq`
        // (#909); reject rather than silently keeping the last occurrence
        // (#985).
        if map.contains_key(&key) {
            return Err(InktParseError {
                message: "duplicate key in map value".into(),
                line,
                col,
            });
        }
        map.insert(key, value);
    }
    Ok(Value::map(map))
}

/// Parse a `record_value` node (`(record <shape> <field>…)`) into a
/// [`Value::Record`] — the read-side leg paired with `write_value`'s
/// `(record …)` atom (issue #742).
fn parse_record_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let mut children = pair.into_inner();
    let shape_pair = children.next().ok_or_else(|| InktParseError {
        message: "expected shape id in record".into(),
        line: 0,
        col: 0,
    })?;
    let shape_id: u32 = shape_pair.as_str().parse().map_err(|_| InktParseError {
        message: "invalid record shape id".into(),
        line: 0,
        col: 0,
    })?;
    let mut fields = Vec::new();
    for field in children {
        fields.push(parse_value(field, None)?);
    }
    Ok(Value::record(ShapeId(shape_id), fields))
}

/// Parse a `closure_value` node (`(closure <target> (val|ref <name> <value>)…)`)
/// into a [`Value::Closure`] — the read-side leg paired with `write_value`'s
/// `(closure …)` atom (issue #742).
fn parse_closure_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let mut children = pair.into_inner();
    let target_pair = children.next().ok_or_else(|| InktParseError {
        message: "expected def_id in closure".into(),
        line: 0,
        col: 0,
    })?;
    let target = parse_def_id(target_pair)?;
    let mut env = Vec::new();
    for entry in children {
        if entry.as_rule() != Rule::closure_entry {
            continue;
        }
        let mut ei = entry.into_inner();
        let mode = ei.next().ok_or_else(|| InktParseError {
            message: "expected mode in closure entry".into(),
            line: 0,
            col: 0,
        })?;
        let is_ref = mode.as_str() == "ref";
        let name_int = ei.next().ok_or_else(|| InktParseError {
            message: "expected name id in closure entry".into(),
            line: 0,
            col: 0,
        })?;
        let name = NameId(parse_u16(&name_int)?);
        let payload_pair = ei.next().ok_or_else(|| InktParseError {
            message: "expected payload value in closure entry".into(),
            line: 0,
            col: 0,
        })?;
        let payload = parse_value(payload_pair, None)?;
        env.push(ClosureEnvEntry {
            name,
            is_ref,
            payload,
        });
    }
    Ok(Value::closure(target, env))
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

/// Parse a `map_key` node (`string | integer | bool_value`) into a
/// [`MapKey`] — the ratified scalar key domain (value-model-spec §4).
fn parse_map_key(pair: P<'_>) -> Result<MapKey, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty map key".into(),
        line: 0,
        col: 0,
    })?;
    match inner.as_rule() {
        Rule::string => Ok(MapKey::Str(unescape_string(inner.as_str()).into())),
        Rule::integer => {
            let n: i32 = inner
                .as_str()
                .parse()
                .map_err(|_| err(&inner, "invalid integer map key"))?;
            Ok(MapKey::Int(n))
        }
        Rule::bool_value => Ok(MapKey::Bool(inner.as_str() == "true")),
        _ => Err(err(
            &inner,
            format!("unexpected map key rule: {:?}", inner.as_rule()),
        )),
    }
}

// ── Literal pool ─────────────────────────────────────────────────────────────

fn parse_literal_pool(pair: P<'_>) -> Result<Vec<Value>, InktParseError> {
    let mut pool = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::value {
            pool.push(parse_value(entry, None)?);
        }
    }
    Ok(pool)
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

// ── Struct shapes (TM-4, docs/format-v4-rfc.md §1) ───────────────────────────
// Mirrors the `.inkb` `StructShapes` section reader (the #742/#883 lesson —
// the writer and reader land together).

fn parse_struct_shapes(pair: P<'_>) -> Result<Vec<StructShapeDef>, InktParseError> {
    let mut shapes = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::struct_shape_entry {
            shapes.push(parse_struct_shape_entry(entry)?);
        }
    }
    Ok(shapes)
}

fn parse_struct_shape_entry(pair: P<'_>) -> Result<StructShapeDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = ShapeId(
        inner
            .next()
            .ok_or_else(|| InktParseError {
                message: "expected shape id in struct".into(),
                line: 0,
                col: 0,
            })?
            .as_str()
            .parse()
            .map_err(|_| InktParseError {
                message: "invalid struct shape id".into(),
                line: 0,
                col: 0,
            })?,
    );

    let name_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected name integer in struct".into(),
        line: 0,
        col: 0,
    })?;
    let name = NameId(parse_u16(&name_int)?);

    let mut fields = Vec::new();
    for remaining in inner {
        if remaining.as_rule() == Rule::struct_field {
            let field_int = remaining
                .into_inner()
                .next()
                .ok_or_else(|| InktParseError {
                    message: "expected name integer in struct field".into(),
                    line: 0,
                    col: 0,
                })?;
            fields.push(NameId(parse_u16(&field_int)?));
        }
    }

    Ok(StructShapeDef { id, name, fields })
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
    let name_val =
        next_rule(&mut inner, Rule::integer, "list_item name").map_or(Ok(0), |p| parse_u16(&p))?;
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

fn parse_visibility(pair: P<'_>) -> Result<Vec<DefinitionId>, InktParseError> {
    let mut ids = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::private_entry {
            let id_pair = entry.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected def_id in private entry".into(),
                line: 1,
                col: 1,
            })?;
            ids.push(parse_def_id(id_pair)?);
        }
    }
    Ok(ids)
}

/// M-3 (`docs/modules-spec.md` §5): parse `(alias_table (alias $old -> $new) …)`.
fn parse_alias_table(pair: P<'_>) -> Result<Vec<AliasEntry>, InktParseError> {
    let mut aliases = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::alias_entry {
            aliases.push(parse_alias_entry(entry)?);
        }
    }
    Ok(aliases)
}

fn parse_alias_entry(pair: P<'_>) -> Result<AliasEntry, InktParseError> {
    let mut inner = pair.into_inner();
    let old = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected old def_id in alias".into(),
        line: 0,
        col: 0,
    })?)?;
    let new = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected new def_id in alias".into(),
        line: 0,
        col: 0,
    })?)?;
    Ok(AliasEntry { old, new })
}

/// FS-3 (`docs/flow-suspension-spec.md` §4/§11): parse
/// `(frame_shapes (frame $site $slot …) …)`. Each `frame` entry is the
/// `await` site's stable `DefinitionId` followed by its name-keyed
/// crossing-local slots (interned `NameId`s).
fn parse_frame_shapes(pair: P<'_>) -> Result<Vec<FrameShapeDef>, InktParseError> {
    let mut shapes = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::frame_shape_entry {
            shapes.push(parse_frame_shape_entry(entry)?);
        }
    }
    Ok(shapes)
}

fn parse_frame_shape_entry(pair: P<'_>) -> Result<FrameShapeDef, InktParseError> {
    let mut inner = pair.into_inner();
    let site = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected site def_id in frame shape".into(),
        line: 0,
        col: 0,
    })?)?;
    let mut slots = Vec::new();
    for slot in inner {
        slots.push(NameId(parse_u16(&slot)?));
    }
    Ok(FrameShapeDef { site, slots })
}

/// T2-3 (`docs/effects-spec.md` §11): parse `(effect_rows (row …) …)`.
fn parse_effect_rows(pair: P<'_>) -> Result<Vec<EffectRowEntry>, InktParseError> {
    let mut rows = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::effect_row {
            rows.push(parse_effect_row(&entry)?);
        }
    }
    Ok(rows)
}

fn parse_effect_row(pair: &P<'_>) -> Result<EffectRowEntry, InktParseError> {
    let mut inner = pair.clone().into_inner();
    let def = parse_def_id(
        inner
            .next()
            .ok_or_else(|| err(pair, "expected row def_id"))?,
    )?;
    // #882 freeze bit: defaults to `true` (host entry point) — `internal_flag`
    // is present only for a `#@private` def (see `EffectRowEntry::is_entry`'s
    // doc). Parsed rule-by-rule (not positionally) because this optional
    // token sits before the mandatory reads/writes/calls triple.
    let mut is_entry = true;
    let mut reads = None;
    let mut writes = None;
    let mut calls = None;
    let mut opaque = false;
    let mut emits = false;
    let mut tags = false;
    let mut faults = false;
    let mut dispatches = Vec::new();
    for rest in inner {
        match rest.as_rule() {
            Rule::internal_flag => is_entry = false,
            Rule::effects_reads => reads = Some(parse_effect_cells(rest)?),
            Rule::effects_writes => writes = Some(parse_effect_cells(rest)?),
            Rule::effects_calls => calls = Some(parse_effect_calls(rest)?),
            Rule::opaque_flag => opaque = true,
            Rule::emits_flag => emits = true,
            Rule::tags_flag => tags = true,
            Rule::faults_flag => faults = true,
            Rule::dispatch_entry => dispatches.push(parse_dispatch_entry(&rest)?),
            _ => {}
        }
    }
    let reads = reads.ok_or_else(|| err(pair, "expected reads"))?;
    let writes = writes.ok_or_else(|| err(pair, "expected writes"))?;
    let calls = calls.ok_or_else(|| err(pair, "expected calls"))?;
    Ok(EffectRowEntry {
        def,
        is_entry,
        direct: DirectEffects {
            reads,
            writes,
            calls,
            opaque,
            emits,
            tags,
            faults,
        },
        dispatches,
    })
}

fn parse_dispatch_entry(pair: &P<'_>) -> Result<DispatchEntry, InktParseError> {
    let mut cell = None;
    let mut narrowable = false;
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut calls = Vec::new();
    let mut opaque = false;
    let mut emits = false;
    let mut tags = false;
    let mut faults = false;
    for part in pair.clone().into_inner() {
        match part.as_rule() {
            Rule::def_id => cell = Some(parse_def_id(part)?),
            Rule::narrowable_flag => narrowable = true,
            Rule::effects_reads => reads = parse_effect_cells(part)?,
            Rule::effects_writes => writes = parse_effect_cells(part)?,
            Rule::effects_calls => calls = parse_effect_calls(part)?,
            Rule::opaque_flag => opaque = true,
            Rule::emits_flag => emits = true,
            Rule::tags_flag => tags = true,
            Rule::faults_flag => faults = true,
            _ => {}
        }
    }
    let cell = cell.ok_or_else(|| err(pair, "expected dispatch cell def_id"))?;
    Ok(DispatchEntry {
        cell,
        narrowable,
        fallback: DirectEffects {
            reads,
            writes,
            calls,
            opaque,
            emits,
            tags,
            faults,
        },
    })
}

/// Parse a `(reads …)` / `(writes …)` cell list — a run of `def_id`s.
fn parse_effect_cells(pair: P<'_>) -> Result<Vec<DefinitionId>, InktParseError> {
    let mut cells = Vec::new();
    for id in pair.into_inner() {
        if id.as_rule() == Rule::def_id {
            cells.push(parse_def_id(id)?);
        }
    }
    Ok(cells)
}

/// Parse a `(calls (call <name> any) …)` atom list.
fn parse_effect_calls(pair: P<'_>) -> Result<Vec<CallAtom>, InktParseError> {
    let mut calls = Vec::new();
    for atom in pair.into_inner() {
        if atom.as_rule() == Rule::call_atom {
            calls.push(parse_call_atom(&atom)?);
        }
    }
    Ok(calls)
}

fn parse_call_atom(pair: &P<'_>) -> Result<CallAtom, InktParseError> {
    let mut inner = pair.clone().into_inner();
    let name_pair = inner
        .next()
        .ok_or_else(|| err(pair, "expected call atom name"))?;
    let name = NameId(parse_u16(&name_pair)?);
    // The capability-parameter slot: `any` is the only v1 value (the grammar's
    // `cap_param` rule accepts only that literal). The reserved handle-parameter
    // slot is `None` in v1 — nothing textual carries a bound handle.
    let capability = CapabilityParam::Any;
    Ok(CallAtom {
        name,
        capability,
        handle_param: None,
    })
}

fn parse_address_paths(pair: P<'_>) -> Result<Vec<AddressPath>, InktParseError> {
    let mut paths = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::address_path_entry {
            paths.push(parse_address_path_entry(entry)?);
        }
    }
    Ok(paths)
}

fn parse_address_path_entry(pair: P<'_>) -> Result<AddressPath, InktParseError> {
    let mut inner = pair.into_inner();
    let path_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected path index in address_path".into(),
        line: 0,
        col: 0,
    })?;
    let path = NameId(parse_u16(&path_int)?);
    let target = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected target def_id in address_path".into(),
        line: 0,
        col: 0,
    })?)?;
    Ok(AddressPath { path, target })
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

#[expect(
    clippy::too_many_lines,
    reason = "one-arm-per-field container parser; splitting would scatter the field table"
)]
fn parse_container(pair: P<'_>) -> Result<(ContainerDef, ScopeLineTable), InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in container".into(),
        line: 0,
        col: 0,
    })?)?;

    let mut counting_flags = CountingFlags::empty();
    let mut path_hash = 0i32;
    let mut param_count = 0u8;
    let mut params: Vec<ParamMeta> = Vec::new();
    let mut local = false;
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
                            "invisible" => counting_flags |= CountingFlags::INVISIBLE,
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
            Rule::params_field => {
                let mut fields = child.into_inner();
                let val = fields.next().ok_or_else(|| InktParseError {
                    message: "expected integer in params".into(),
                    line: 0,
                    col: 0,
                })?;
                param_count = val.as_str().parse().map_err(|_| InktParseError {
                    message: "invalid params integer".into(),
                    line: 0,
                    col: 0,
                })?;
                // Per-param name/mode metadata (T1c, #700): `(val id)` / `(ref
                // id)` entries after the count, in declared order.
                for meta in fields {
                    if meta.as_rule() != Rule::param_meta {
                        continue;
                    }
                    let mut mi = meta.into_inner();
                    let mode = mi.next().ok_or_else(|| InktParseError {
                        message: "expected mode in param_meta".into(),
                        line: 0,
                        col: 0,
                    })?;
                    let is_ref = mode.as_str() == "ref";
                    let name_int = mi.next().ok_or_else(|| InktParseError {
                        message: "expected name id in param_meta".into(),
                        line: 0,
                        col: 0,
                    })?;
                    params.push(ParamMeta {
                        name: NameId(parse_u16(&name_int)?),
                        is_ref,
                    });
                }
                // `ContainerDef::params`'s doc invariant: `params.len()`
                // always equals `param_count` whenever per-param metadata is
                // present at all (empty `params` is the separate, legitimate
                // "count only, no metadata" case — e.g. the converter
                // pipeline). A `.inkt` file asserting otherwise (fuzz-found,
                // #745) is malformed input, not silently-acceptable data:
                // `write_inkt`'s `(params N …)` clause is gated on
                // `param_count != 0`, so an inconsistent `param_count: 0` with
                // non-empty `params` would round-trip by silently dropping
                // the params entirely on the next write.
                if !params.is_empty() && params.len() != usize::from(param_count) {
                    return Err(InktParseError {
                        message: format!(
                            "params metadata count ({}) does not match declared param_count ({param_count})",
                            params.len()
                        ),
                        line: 0,
                        col: 0,
                    });
                }
            }
            Rule::local_flag => local = true,
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
        param_count,
        // Per-param name/mode metadata (T1c, #700), reconstructed from the
        // `(params N (mode id)…)` dump so the `.inkt` round-trip is lossless
        // (matches the binary `.inkb` path used by persistence/rehydration).
        params,
        local,
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
        "call_variable" => {
            // "argc=N" is parsed as a kv_operand. Extract the value after "=".
            let kv_str = operand_str(&operands, 0, mnemonic)?;
            let argc_str = kv_str.strip_prefix("argc=").unwrap_or(kv_str);
            let argc: u8 = argc_str.parse().map_err(|_| InktParseError {
                message: format!("invalid argc in call_variable: {kv_str}"),
                line: 0,
                col: 0,
            })?;
            Ok(Opcode::CallVariable(argc))
        }

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

        // Collections (T1b)
        "array_new" => Ok(Opcode::ArrayNew(parse_operand_u32(&operands, 0, mnemonic)?)),
        "map_new" => Ok(Opcode::MapNew(parse_operand_u32(&operands, 0, mnemonic)?)),
        "index_get" => Ok(Opcode::IndexGet),
        "index_set" => Ok(Opcode::IndexSet),
        "collection_len" => Ok(Opcode::CollectionLen),
        "map_get" => Ok(Opcode::MapGet),
        "map_insert" => Ok(Opcode::MapInsert),
        "map_remove" => Ok(Opcode::MapRemove),
        "map_contains" => Ok(Opcode::MapContains),
        "collection_keys" => Ok(Opcode::CollectionKeys),
        "collection_values" => Ok(Opcode::CollectionValues),
        "push_literal" => Ok(Opcode::PushLiteral(parse_operand_u32(
            &operands, 0, mnemonic,
        )?)),

        // Sharing discipline (T1b-4)
        "take_global" => Ok(Opcode::TakeGlobal(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "take_temp" => Ok(Opcode::TakeTemp(parse_operand_u16(&operands, 0, mnemonic)?)),

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

        // Records (TM-4, `docs/typed-mode-spec.md` §6) — read-side leg paired
        // with `write_opcode`'s `record_new`/`record_get_dyn`/`record_set_dyn`/
        // `record_get`/`record_set` mnemonics (issue #871, the #742 write/read
        // asymmetry class).
        "record_new" => Ok(Opcode::RecordNew(parse_operand_u32(
            &operands, 0, mnemonic,
        )?)),
        "record_get_dyn" => Ok(Opcode::RecordGetDyn(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "record_set_dyn" => Ok(Opcode::RecordSetDyn(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "record_get" => Ok(Opcode::RecordGet(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "record_set" => Ok(Opcode::RecordSet(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),

        // Conversion intrinsics (TM-3 completion, issue #659/#871)
        "convert_int" => Ok(Opcode::ConvertInt),
        "convert_float" => Ok(Opcode::ConvertFloat),
        "convert_string" => Ok(Opcode::ConvertString),

        // Function values (T1c, `docs/t1c-spec.md` §3/§6, issue #871) —
        // read-side leg paired with `write_opcode`'s `push_fn_ref`/
        // `make_closure`/`call_value`/`bind_value` mnemonics.
        "push_fn_ref" => Ok(Opcode::PushFnRef(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "make_closure" => {
            let target = parse_operand_def_id(&operands, 0, mnemonic)?;
            let bound_count = parse_kv_operand_u8(&operands, 1, "bound=", mnemonic)?;
            Ok(Opcode::MakeClosure {
                target,
                bound_count,
            })
        }
        "call_value" => Ok(Opcode::CallValue(parse_kv_operand_u8(
            &operands, 0, "argc=", mnemonic,
        )?)),
        "bind_value" => Ok(Opcode::BindValue(parse_kv_operand_u8(
            &operands, 0, "argc=", mnemonic,
        )?)),

        // Path projections (T1e, `docs/t1e-spec.md` §3, issue #871) —
        // read-side leg paired with `write_opcode`'s `make_projection`/
        // `proj_read`/`proj_write` mnemonics.
        "make_projection" => {
            let root = parse_operand_def_id(&operands, 0, mnemonic)?;
            let segment_count = parse_kv_operand_u8(&operands, 1, "segments=", mnemonic)?;
            Ok(Opcode::MakeProjection {
                root,
                segment_count,
            })
        }
        "proj_read" => Ok(Opcode::ProjRead),
        "proj_write" => Ok(Opcode::ProjWrite),

        // Stdlib slice 1 completion (#857)
        "char_at" => Ok(Opcode::CharAt),

        // NS-A1 Option + stdlib flips
        "push_none" => Ok(Opcode::PushNone),
        "make_some" => Ok(Opcode::MakeSome),
        "str_find" => Ok(Opcode::StrFind),
        "seq_index_of" => Ok(Opcode::SeqIndexOf),
        "seq_min" => Ok(Opcode::SeqMin),
        "seq_max" => Ok(Opcode::SeqMax),
        "seq_first" => Ok(Opcode::SeqFirst),
        "seq_last" => Ok(Opcode::SeqLast),
        "seq_pop" => Ok(Opcode::SeqPop),
        "map_get_opt" => Ok(Opcode::MapGetOpt),
        "map_contains_value" => Ok(Opcode::MapContainsValue),
        "map_clear" => Ok(Opcode::MapClear),

        // NS-A6 rand verbs
        "rand_float" => Ok(Opcode::RandFloat),
        "rand_chance" => Ok(Opcode::RandChance),
        "rand_pick" => Ok(Opcode::RandPick),
        "rand_shuffle" => Ok(Opcode::RandShuffle),
        "range_make_excl" => Ok(Opcode::RangeMakeExcl),
        "range_make_incl" => Ok(Opcode::RangeMakeIncl),
        "range_non_empty" => Ok(Opcode::RangeNonEmpty),

        // NS-A4 ordering verbs (#1110)
        "seq_sorted" => Ok(Opcode::SeqSorted),
        "seq_sorted_by" => Ok(Opcode::SeqSortedBy),

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

        // NS-A8 numeric tower: the TowerOp mnemonic IS the instruction word
        // (`make_vec2` … `tower_lerp`) — one wire opcode, thirteen
        // spellings, `TowerOp::mnemonic`/`from_mnemonic` the single pairing.
        _ => tower_mnemonic_opcode(mnemonic)
            .or_else(|| collect_mnemonic_opcode(mnemonic))
            .ok_or_else(|| InktParseError {
                message: format!("unknown opcode: {mnemonic}"),
                line: mnemonic_pair.line_col().0,
                col: mnemonic_pair.line_col().1,
            }),
    }
}

/// The `.inkt` reader leg for the NS-A8 `Tower` opcode family: resolve a
/// mnemonic to `Opcode::Tower(kind)` via [`crate::TowerOp::from_mnemonic`].
fn tower_mnemonic_opcode(mnemonic: &str) -> Option<Opcode> {
    crate::TowerOp::from_mnemonic(mnemonic).map(Opcode::Tower)
}

/// The `.inkt` reader leg for the NS-A7 `Collect` opcode family: resolve a
/// mnemonic to `Opcode::Collect(kind)` via [`crate::CollectOp::from_mnemonic`].
fn collect_mnemonic_opcode(mnemonic: &str) -> Option<Opcode> {
    crate::CollectOp::from_mnemonic(mnemonic).map(Opcode::Collect)
}

/// Parse a `weighted_value` node (NS-A7, `docs/stdlib-spec.md` §8): each
/// child is a `weighted_entry` — an integer weight then a recursively-parsed
/// value. Enforces the evidence-by-construction invariant (non-empty,
/// weights ≥ 1) with targeted messages.
fn parse_weighted_value(pair: P<'_>) -> Result<Value, InktParseError> {
    let mut entries = Vec::new();
    for entry_pair in pair.into_inner() {
        let mut parts = entry_pair.clone().into_inner();
        let weight_pair = parts.next().ok_or_else(|| InktParseError {
            message: "weighted entry missing weight".into(),
            line: 0,
            col: 0,
        })?;
        let weight: i32 = weight_pair
            .as_str()
            .trim()
            .parse()
            .map_err(|_| err(&weight_pair, "invalid weighted entry weight"))?;
        if weight < 1 {
            return Err(err(
                &weight_pair,
                "weighted entry weight must be a positive int",
            ));
        }
        let value_pair = parts.next().ok_or_else(|| InktParseError {
            message: "weighted entry missing value".into(),
            line: 0,
            col: 0,
        })?;
        entries.push((weight, parse_value(value_pair, None)?));
    }
    if entries.is_empty() {
        return Err(InktParseError {
            message: "weighted table must have at least one entry".into(),
            line: 0,
            col: 0,
        });
    }
    Ok(Value::weighted(entries))
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

fn parse_operand_u32(operands: &[P<'_>], idx: usize, context: &str) -> Result<u32, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid u32 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

/// Parse a `kv_operand` of the form `"<prefix><value>"` (e.g. `"bound=3"`,
/// `"segments=2"`) into its numeric value. Mirrors the inline `argc=`
/// stripping already used by `call_variable`/`call_external`, generalized so
/// `make_closure`'s `bound=` and `make_projection`'s `segments=` operands
/// (issue #871) don't each duplicate it.
fn parse_kv_operand_u8(
    operands: &[P<'_>],
    idx: usize,
    prefix: &str,
    context: &str,
) -> Result<u8, InktParseError> {
    let kv_str = operand_str(operands, idx, context)?;
    let value_str = kv_str.strip_prefix(prefix).unwrap_or(kv_str);
    value_str.parse().map_err(|_| InktParseError {
        message: format!("invalid {prefix}value in {context}: {kv_str}"),
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
