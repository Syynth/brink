//! Values/literals grammar-rule cluster: `.inkt` value parsing — scalars,
//! collections, and the NS-A5/NS-A7/NS-A8/T1c/T1d/T1e composite value forms.
//!
//! Pure `mod` extraction (issue #685) from the former monolithic `read.rs` —
//! no logic changes, only the module boundary is new.

use super::primitives::{err, parse_def_id, parse_u16, unescape_string};
use super::{InktParseError, P, Rule};
use crate::id::NameId;
use crate::value::{
    ClosureEnvEntry, ListValue, MapKey, OrderedMap, ProjSegment, ShapeId, Value, ValueType,
};

#[expect(clippy::needless_pass_by_value)]
pub(super) fn parse_value_type(pair: P<'_>) -> Result<ValueType, InktParseError> {
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
pub(super) fn parse_value(
    pair: P<'_>,
    type_hint: Option<ValueType>,
) -> Result<Value, InktParseError> {
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

pub(super) fn parse_literal_pool(pair: P<'_>) -> Result<Vec<Value>, InktParseError> {
    let mut pool = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::value {
            pool.push(parse_value(entry, None)?);
        }
    }
    Ok(pool)
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
