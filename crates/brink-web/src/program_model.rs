//! Structured model of the compiled program for the studio Program Explorer.
//!
//! Built from [`StoryData`] (no runtime needed): globals / lists / externals
//! tables plus a knot/stitch tree with per-knot, name-resolved bytecode
//! disassembly. Mirrors `brink_format`'s `.inkt` writer but resolves
//! `DefinitionId`s to author-facing knot paths and variable names instead of
//! hashes.

use std::collections::BTreeMap;
use std::collections::HashMap;

use brink_format::{
    ChoiceFlags, CountingFlags, DefinitionId, NameId, Opcode, SequenceKind, StoryData, Value,
};
use serde::Serialize;

#[derive(Serialize)]
pub struct ProgramModelJs {
    checksum: String,
    globals: Vec<ProgramGlobalJs>,
    lists: Vec<ProgramListJs>,
    externals: Vec<ProgramExternalJs>,
    knots: Vec<KnotNodeJs>,
}

#[derive(Serialize)]
struct ProgramGlobalJs {
    name: String,
    ty: String,
    default: String,
    mutable: bool,
}

#[derive(Serialize)]
struct ProgramListJs {
    name: String,
    items: Vec<ProgramListItemJs>,
}

#[derive(Serialize)]
struct ProgramListItemJs {
    name: String,
    ordinal: i32,
}

#[derive(Serialize)]
struct ProgramExternalJs {
    name: String,
    arg_count: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback: Option<String>,
}

#[derive(Serialize)]
struct KnotNodeJs {
    path: String,
    name: String,
    /// "knot" | "stitch"
    kind: &'static str,
    flags: Vec<&'static str>,
    path_hash: i32,
    disasm: Vec<String>,
    children: Vec<KnotNodeJs>,
}

/// Resolves ids to author-facing names for a single program.
struct Resolver<'a> {
    data: &'a StoryData,
    /// container/scope `DefinitionId` → qualified path (from `address_paths`).
    def_path: HashMap<DefinitionId, &'a str>,
    global_name: HashMap<DefinitionId, &'a str>,
    external_name: HashMap<DefinitionId, &'a str>,
    list_item_name: HashMap<DefinitionId, &'a str>,
}

impl<'a> Resolver<'a> {
    fn new(data: &'a StoryData) -> Self {
        let nm = |id: NameId| data.name_table.get(id.0 as usize).map(String::as_str);
        Self {
            data,
            def_path: data
                .address_paths
                .iter()
                .filter_map(|ap| nm(ap.path).map(|p| (ap.target, p)))
                .collect(),
            global_name: data
                .variables
                .iter()
                .filter_map(|v| nm(v.name).map(|n| (v.id, n)))
                .collect(),
            external_name: data
                .externals
                .iter()
                .filter_map(|e| nm(e.name).map(|n| (e.id, n)))
                .collect(),
            list_item_name: data
                .list_items
                .iter()
                .filter_map(|i| nm(i.name).map(|n| (i.id, n)))
                .collect(),
        }
    }

    fn name(&self, id: NameId) -> &str {
        self.data
            .name_table
            .get(id.0 as usize)
            .map_or("?", String::as_str)
    }
    fn path(&self, id: DefinitionId) -> &str {
        self.def_path.get(&id).copied().unwrap_or("?")
    }
    fn gname(&self, id: DefinitionId) -> &str {
        self.global_name.get(&id).copied().unwrap_or("?")
    }
    fn ename(&self, id: DefinitionId) -> &str {
        self.external_name.get(&id).copied().unwrap_or("?")
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one display arm per Value variant — the NS-A8 tower arms \
                  pushed this past 100; splitting would scatter the single \
                  source of truth for the JS-facing display forms"
    )]
    fn format_value(&self, v: &Value) -> String {
        match v {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => format!("\"{s}\""),
            Value::Null => "null".to_owned(),
            Value::List(list) => {
                let members: Vec<&str> = list
                    .items
                    .iter()
                    .map(|id| self.list_item_name.get(id).copied().unwrap_or("?"))
                    .collect();
                format!("({})", members.join(", "))
            }
            Value::DivertTarget(id) => format!("-> {}", self.path(*id)),
            Value::VariablePointer(id) => format!("ref {}", self.gname(*id)),
            Value::TempPointer { slot, frame_depth } => format!("temp[{slot}]@{frame_depth}"),
            Value::FragmentRef(idx) => format!("<fragment {idx}>"),
            // Collections are runtime-only until T1b emits their opcodes, so
            // these arms are unreachable and JS-unobservable in this version.
            Value::Array(items) => {
                let parts: Vec<String> = items.iter().map(|v| self.format_value(v)).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Map(map) => {
                let parts: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{k:?}: {}", self.format_value(v)))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            // TM-4 records: same "runtime-only, no compiler surface yet"
            // rationale as the collection arms above.
            Value::Record { shape, fields } => {
                let parts: Vec<String> = fields.iter().map(|v| self.format_value(v)).collect();
                format!("Record#{}{{{}}}", shape.0, parts.join(", "))
            }
            // Function values (T1c, #700): a `#fn(…)` baked into a declaration
            // default reaches the program model as a global's initial value.
            Value::FnRef(target) => format!("fn {}", self.path(*target)),
            Value::Closure(c) => {
                let parts: Vec<String> = c
                    .env
                    .iter()
                    .map(|e| {
                        let mode = if e.is_ref { "ref" } else { "val" };
                        format!(
                            "{mode} {} = {}",
                            self.name(e.name),
                            self.format_value(&e.payload)
                        )
                    })
                    .collect();
                format!("fn {}({})", self.path(c.target), parts.join(", "))
            }
            // Handle values (T1d, `docs/t1d-spec.md` §6): no literal syntax
            // constructs one, but this arm keeps `format_value` exhaustive —
            // same display form as the runtime's authoritative `string(h)`.
            Value::Handle { kind, id } => format!("handle {}#{id}", self.name(*kind)),
            // Projection values (T1e, `docs/t1e-spec.md` §4): same display
            // form as the runtime's authoritative `string(p)` — `ref
            // <root><path>`.
            // Option values (NS-A1): same display form as the runtime's
            // authoritative `string(x)` — `none` / `some(<inner>)`.
            Value::OptionVal(inner) => match inner {
                None => "none".to_owned(),
                Some(v) => format!("some({})", self.format_value(v)),
            },
            // Range values (NS-A5, F7): same display form as the runtime's
            // authoritative `string(r)` — the written `0..10` / `1..=6`.
            Value::Range {
                start,
                end,
                inclusive,
            } => {
                if *inclusive {
                    format!("{start}..={end}")
                } else {
                    format!("{start}..{end}")
                }
            }
            // Weighted tables (NS-A7): same construction-literal display
            // form as the runtime's authoritative `string(v)`
            // (`value_ops::stringify`) — entries in construction order.
            Value::Weighted(w) => {
                let parts: Vec<String> = w
                    .entries
                    .iter()
                    .map(|(weight, val)| format!("{weight}: {}", self.format_value(val)))
                    .collect();
                format!("Weighted {{ {} }}", parts.join(", "))
            }
            // Tower values (NS-A8): same structural display form as the
            // runtime's authoritative `string(v)` (`value_ops::stringify`)
            // — kind name + named components in glam's declared order.
            Value::Vec2(v) => format!("vec2 {{ x: {}, y: {} }}", v.x, v.y),
            Value::Vec3(v) => format!("vec3 {{ x: {}, y: {}, z: {} }}", v.x, v.y, v.z),
            Value::Vec4(v) => format!("vec4 {{ x: {}, y: {}, z: {}, w: {} }}", v.x, v.y, v.z, v.w),
            Value::Quat(q) => format!("quat {{ x: {}, y: {}, z: {}, w: {} }}", q.x, q.y, q.z, q.w),
            Value::Mat2(m) => format!(
                "mat2 {{ x_axis: {}, y_axis: {} }}",
                self.format_value(&Value::Vec2(m.x_axis)),
                self.format_value(&Value::Vec2(m.y_axis))
            ),
            Value::Mat3(m) => format!(
                "mat3 {{ x_axis: {}, y_axis: {}, z_axis: {} }}",
                self.format_value(&Value::Vec3(m.x_axis)),
                self.format_value(&Value::Vec3(m.y_axis)),
                self.format_value(&Value::Vec3(m.z_axis))
            ),
            Value::Mat4(m) => format!(
                "mat4 {{ x_axis: {}, y_axis: {}, z_axis: {}, w_axis: {} }}",
                self.format_value(&Value::Vec4(m.x_axis)),
                self.format_value(&Value::Vec4(m.y_axis)),
                self.format_value(&Value::Vec4(m.z_axis)),
                self.format_value(&Value::Vec4(m.w_axis))
            ),
            Value::Projection(p) => {
                let mut out = format!("ref {}", self.gname(p.cell));
                for seg in &p.segments {
                    match seg {
                        brink_format::ProjSegment::Index(n) => {
                            out.push('[');
                            out.push_str(&n.to_string());
                            out.push(']');
                        }
                        brink_format::ProjSegment::Key(v) => {
                            out.push('[');
                            out.push_str(&self.format_value(v));
                            out.push(']');
                        }
                    }
                }
                out
            }
        }
    }
}

/// Build the structured program model from decoded story data.
pub fn build(data: &StoryData) -> ProgramModelJs {
    let r = Resolver::new(data);

    let globals = data
        .variables
        .iter()
        .map(|v| ProgramGlobalJs {
            name: r.name(v.name).to_owned(),
            ty: format!("{:?}", v.value_type).to_lowercase(),
            default: r.format_value(&v.default_value),
            mutable: v.mutable,
        })
        .collect();

    let lists = data
        .list_defs
        .iter()
        .map(|ld| ProgramListJs {
            name: r.name(ld.name).to_owned(),
            items: ld
                .items
                .iter()
                .map(|(nid, ord)| ProgramListItemJs {
                    name: r.name(*nid).to_owned(),
                    ordinal: *ord,
                })
                .collect(),
        })
        .collect();

    let externals = data
        .externals
        .iter()
        .map(|e| ProgramExternalJs {
            name: r.name(e.name).to_owned(),
            arg_count: e.arg_count,
            fallback: e.fallback.map(|id| r.path(id).to_owned()),
        })
        .collect();

    ProgramModelJs {
        checksum: format!("0x{:08x}", data.source_checksum),
        globals,
        lists,
        externals,
        knots: build_knots(data, &r),
    }
}

/// Build the knot → stitch tree from named scope containers.
fn build_knots(data: &StoryData, r: &Resolver) -> Vec<KnotNodeJs> {
    // Group named scope containers by top-level knot name. BTreeMap keeps the
    // output deterministic.
    let mut groups: BTreeMap<String, KnotGroup> = BTreeMap::new();
    for c in &data.containers {
        // Only named scope containers (knots / stitches), skip the root scope
        // and anonymous child containers.
        let Some(name_id) = c.name else { continue };
        if c.scope_id != c.id {
            continue;
        }
        let path = if r.def_path.contains_key(&c.id) {
            r.path(c.id).to_owned()
        } else {
            // Root or unmapped scope — skip (no qualified path).
            continue;
        };
        // Skip the root/global-decl scope (empty path or empty name).
        if path.is_empty() || r.name(name_id).is_empty() {
            continue;
        }
        let flags = counting_flags(c.counting_flags);
        let disasm = disassemble(&c.bytecode, r);
        let (knot, leaf) = match path.split_once('.') {
            Some((k, _)) => (k.to_owned(), false),
            None => (path.clone(), true),
        };
        let node = KnotNodeJs {
            path: path.clone(),
            name: leaf_name(&path).to_owned(),
            kind: if leaf { "knot" } else { "stitch" },
            flags,
            path_hash: c.path_hash,
            disasm,
            children: Vec::new(),
        };
        let group = groups.entry(knot).or_default();
        if leaf {
            group.knot = Some(node);
        } else {
            group.stitches.push(node);
        }
    }

    groups
        .into_iter()
        .map(|(kname, group)| match group.knot {
            Some(mut node) => {
                node.children = group.stitches;
                node
            }
            // A knot with stitches but no own scope container (rare): synthesize.
            None => KnotNodeJs {
                path: kname.clone(),
                name: kname,
                kind: "knot",
                flags: Vec::new(),
                path_hash: 0,
                disasm: Vec::new(),
                children: group.stitches,
            },
        })
        .collect()
}

#[derive(Default)]
struct KnotGroup {
    knot: Option<KnotNodeJs>,
    stitches: Vec<KnotNodeJs>,
}

fn leaf_name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn counting_flags(flags: CountingFlags) -> Vec<&'static str> {
    let mut out = Vec::new();
    if flags.contains(CountingFlags::VISITS) {
        out.push("visits");
    }
    if flags.contains(CountingFlags::TURNS) {
        out.push("turns");
    }
    if flags.contains(CountingFlags::COUNT_START_ONLY) {
        out.push("start_only");
    }
    out
}

/// Decode a container's bytecode into resolved, one-per-line mnemonics.
fn disassemble(bytecode: &[u8], r: &Resolver) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < bytecode.len() {
        match Opcode::decode(bytecode, &mut offset) {
            Ok(op) => out.push(format_opcode(&op, r)),
            Err(e) => {
                out.push(format!("<decode error: {e}>"));
                break;
            }
        }
    }
    out
}

#[expect(
    clippy::too_many_lines,
    reason = "exhaustive one-arm-per-opcode formatter"
)]
fn format_opcode(op: &Opcode, r: &Resolver) -> String {
    match op {
        // Stack & literals
        Opcode::PushInt(v) => format!("push_int {v}"),
        Opcode::PushFloat(v) => format!("push_float {v}"),
        Opcode::PushBool(v) => format!("push_bool {v}"),
        Opcode::PushString(idx) => format!("push_string #{idx}"),
        Opcode::PushList(idx) => format!("push_list #{idx}"),
        Opcode::PushDivertTarget(id) => format!("push_divert_target {}", r.path(*id)),
        Opcode::PushNull => "push_null".to_owned(),
        Opcode::Pop => "pop".to_owned(),
        Opcode::Duplicate => "duplicate".to_owned(),

        // Arithmetic
        Opcode::Add => "add".to_owned(),
        Opcode::Subtract => "subtract".to_owned(),
        Opcode::Multiply => "multiply".to_owned(),
        Opcode::Divide => "divide".to_owned(),
        Opcode::Modulo => "modulo".to_owned(),
        Opcode::Negate => "negate".to_owned(),

        // Comparison
        Opcode::Equal => "equal".to_owned(),
        Opcode::NotEqual => "not_equal".to_owned(),
        Opcode::Greater => "greater".to_owned(),
        Opcode::GreaterOrEqual => "greater_or_equal".to_owned(),
        Opcode::Less => "less".to_owned(),
        Opcode::LessOrEqual => "less_or_equal".to_owned(),

        // Logic
        Opcode::Not => "not".to_owned(),
        Opcode::And => "and".to_owned(),
        Opcode::Or => "or".to_owned(),

        // Global vars (resolved to variable names)
        Opcode::GetGlobal(id) => format!("get_global {}", r.gname(*id)),
        Opcode::SetGlobal(id) => format!("set_global {}", r.gname(*id)),

        // Temp vars
        Opcode::DeclareTemp(idx) => format!("declare_temp {idx}"),
        Opcode::GetTemp(idx) => format!("get_temp {idx}"),
        Opcode::SetTemp(idx) => format!("set_temp {idx}"),
        Opcode::GetTempRaw(idx) => format!("get_temp_raw {idx}"),

        // Variable pointers
        Opcode::PushVarPointer(id) => format!("push_var_pointer {}", r.gname(*id)),
        Opcode::PushTempPointer(slot) => format!("push_temp_pointer {slot}"),

        // Control flow (targets resolved to knot/stitch paths)
        Opcode::Jump(off) => format!("jump {off}"),
        Opcode::JumpIfFalse(off) => format!("jump_if_false {off}"),
        Opcode::Goto(id) => format!("goto {}", r.path(*id)),
        Opcode::GotoIf(id) => format!("goto_if {}", r.path(*id)),
        Opcode::GotoVariable => "goto_variable".to_owned(),

        // Container flow
        Opcode::EnterContainer(id) => format!("enter_container {}", r.path(*id)),
        Opcode::ExitContainer => "exit_container".to_owned(),

        // Functions / tunnels
        Opcode::Call(id) => format!("call {}", r.path(*id)),
        Opcode::Return => "return".to_owned(),
        Opcode::TunnelCall(id) => format!("tunnel_call {}", r.path(*id)),
        Opcode::TunnelReturn => "tunnel_return".to_owned(),
        Opcode::TunnelCallVariable => "tunnel_call_variable".to_owned(),
        Opcode::CallVariable(argc) => format!("call_variable argc={argc}"),

        // Threads
        Opcode::ThreadCall(id) => format!("thread_call {}", r.path(*id)),
        Opcode::ThreadStart => "thread_start".to_owned(),
        Opcode::ThreadDone => "thread_done".to_owned(),

        // Output
        Opcode::EmitLine(idx, slots) => format!("emit_line #{idx} {slots}"),
        Opcode::EmitValue => "emit_value".to_owned(),
        Opcode::EmitNewline => "emit_newline".to_owned(),
        Opcode::Spring => "spring".to_owned(),
        Opcode::Glue => "glue".to_owned(),
        Opcode::BeginTag => "begin_tag".to_owned(),
        Opcode::EndTag => "end_tag".to_owned(),
        Opcode::EvalLine(idx, slots) => format!("eval_line #{idx} {slots}"),
        Opcode::BeginFragment => "begin_fragment".to_owned(),
        Opcode::EndFragment => "end_fragment".to_owned(),
        Opcode::AttachElement => "attach_element".to_owned(),
        Opcode::EndElementRun => "end_element_run".to_owned(),

        // Choices (target resolved)
        Opcode::BeginChoice(flags, target) => {
            format!(
                "begin_choice {} -> {}",
                format_choice_flags(*flags),
                r.path(*target)
            )
        }
        Opcode::EndChoice => "end_choice".to_owned(),

        // Sequences
        Opcode::Sequence(kind, count) => format!("sequence {} {count}", sequence_kind(*kind)),
        Opcode::SequenceBranch(off) => format!("sequence_branch {off}"),

        // Intrinsics
        Opcode::VisitCount => "visit_count".to_owned(),
        Opcode::TurnsSince => "turns_since".to_owned(),
        Opcode::TurnIndex => "turn_index".to_owned(),
        Opcode::ChoiceCount => "choice_count".to_owned(),
        Opcode::Random => "random".to_owned(),
        Opcode::SeedRandom => "seed_random".to_owned(),

        // Casts / math
        Opcode::CastToInt => "cast_to_int".to_owned(),
        Opcode::CastToFloat => "cast_to_float".to_owned(),
        Opcode::Floor => "floor".to_owned(),
        Opcode::Ceiling => "ceiling".to_owned(),
        Opcode::Pow => "pow".to_owned(),
        Opcode::Min => "min".to_owned(),
        Opcode::Max => "max".to_owned(),

        // External fns (resolved to external name)
        Opcode::CallExternal(id, argc) => format!("call_external {} argc={argc}", r.ename(*id)),

        // List ops
        Opcode::ListContains => "list_contains".to_owned(),
        Opcode::ListNotContains => "list_not_contains".to_owned(),
        Opcode::ListIntersect => "list_intersect".to_owned(),
        Opcode::ListAll => "list_all".to_owned(),
        Opcode::ListInvert => "list_invert".to_owned(),
        Opcode::ListCount => "list_count".to_owned(),
        Opcode::ListMin => "list_min".to_owned(),
        Opcode::ListMax => "list_max".to_owned(),
        Opcode::ListValue => "list_value".to_owned(),
        Opcode::ListRange => "list_range".to_owned(),
        Opcode::ListFromInt => "list_from_int".to_owned(),
        Opcode::ListRandom => "list_random".to_owned(),

        // Collections (T1b)
        Opcode::ArrayNew(n) => format!("array_new {n}"),
        Opcode::MapNew(n) => format!("map_new {n}"),
        Opcode::IndexGet => "index_get".to_owned(),
        Opcode::IndexSet => "index_set".to_owned(),
        Opcode::CollectionLen => "collection_len".to_owned(),
        Opcode::MapGet => "map_get".to_owned(),
        Opcode::MapInsert => "map_insert".to_owned(),
        Opcode::MapRemove => "map_remove".to_owned(),
        Opcode::MapContains => "map_contains".to_owned(),
        Opcode::CollectionKeys => "collection_keys".to_owned(),
        Opcode::CollectionValues => "collection_values".to_owned(),
        Opcode::PushLiteral(idx) => format!("push_literal {idx}"),

        // Sharing discipline (T1b-4)
        Opcode::TakeGlobal(id) => format!("take_global {id}"),
        Opcode::TakeTemp(idx) => format!("take_temp {idx}"),

        // Lifecycle
        Opcode::Done => "done".to_owned(),
        Opcode::Yield => "yield".to_owned(),
        Opcode::End => "end".to_owned(),
        Opcode::Nop => "nop".to_owned(),

        // String eval
        Opcode::BeginStringEval => "begin_string_eval".to_owned(),
        Opcode::EndStringEval => "end_string_eval".to_owned(),

        // Visit
        Opcode::CurrentVisitCount => "current_visit_count".to_owned(),

        // Debug
        Opcode::SourceLocation(line, col) => format!("source_location {line}:{col}"),

        // Records (TM-4)
        Opcode::RecordNew(shape_id) => format!("record_new {shape_id}"),
        Opcode::RecordGetDyn(name_id) => format!("record_get_dyn {name_id}"),
        Opcode::RecordSetDyn(name_id) => format!("record_set_dyn {name_id}"),
        Opcode::RecordGet(offset) => format!("record_get {offset}"),
        Opcode::RecordSet(offset) => format!("record_set {offset}"),

        // Conversion intrinsics (TM-3 completion, #659)
        Opcode::ConvertInt => "convert_int".to_owned(),
        Opcode::ConvertFloat => "convert_float".to_owned(),
        Opcode::ConvertString => "convert_string".to_owned(),

        // Function values (T1c, #700)
        Opcode::PushFnRef(id) => format!("push_fn_ref {}", r.path(*id)),
        Opcode::MakeClosure {
            target,
            bound_count,
        } => format!("make_closure {} bound={bound_count}", r.path(*target)),
        Opcode::CallValue(argc) => format!("call_value argc={argc}"),
        Opcode::BindValue(argc) => format!("bind_value argc={argc}"),

        // Path projections (T1e)
        Opcode::MakeProjection {
            root,
            segment_count,
        } => format!(
            "make_projection {} segments={segment_count}",
            r.gname(*root)
        ),
        Opcode::ProjRead => "proj_read".to_owned(),
        Opcode::ProjWrite => "proj_write".to_owned(),

        // Stdlib slice 1 completion (#857)
        Opcode::CharAt => "char_at".to_owned(),

        // NS-A1 Option + stdlib flips (#1107)
        Opcode::PushNone => "push_none".to_owned(),
        Opcode::MakeSome => "make_some".to_owned(),
        Opcode::StrFind => "str_find".to_owned(),
        Opcode::SeqIndexOf => "seq_index_of".to_owned(),
        Opcode::SeqMin => "seq_min".to_owned(),
        Opcode::SeqMax => "seq_max".to_owned(),
        Opcode::SeqFirst => "seq_first".to_owned(),
        Opcode::SeqLast => "seq_last".to_owned(),
        Opcode::SeqPop => "seq_pop".to_owned(),
        Opcode::MapGetOpt => "map_get_opt".to_owned(),
        Opcode::MapContainsValue => "map_contains_value".to_owned(),
        Opcode::MapClear => "map_clear".to_owned(),
        // B1 `or`-coalescing, short-circuited (issue #1471).
        Opcode::CoalesceSome(off) => format!("coalesce_some {off}"),
        Opcode::OptionBind(slot) => format!("option_bind {slot}"),
        // Seq `remove_at` (issue #1484).
        Opcode::SeqRemoveAt => "seq_remove_at".to_owned(),
        // NS-A6 rand verbs (#1112).
        Opcode::RandFloat => "rand_float".to_owned(),
        Opcode::RandChance => "rand_chance".to_owned(),
        Opcode::RandPick => "rand_pick".to_owned(),
        Opcode::RandShuffle => "rand_shuffle".to_owned(),
        // NS-A5 range ops (#1111).
        Opcode::RangeMakeExcl => "range_make_excl".to_owned(),
        Opcode::RangeMakeIncl => "range_make_incl".to_owned(),
        Opcode::RangeNonEmpty => "range_non_empty".to_owned(),
        // NS-A4 ordering verbs (#1110).
        Opcode::SeqSorted => "seq_sorted".to_owned(),
        Opcode::SeqSortedBy => "seq_sorted_by".to_owned(),

        // NS-A8 numeric tower (#1114): one opcode, per-kind mnemonic —
        // same text as the `.inkt` disassembly.
        Opcode::Tower(op) => op.mnemonic().to_owned(),

        // NS-A7 collections+ (#1113): one opcode, per-kind mnemonic —
        // mirrors the `.inkt` disassembly (`CollectOp::mnemonic`).
        Opcode::Collect(op) => op.mnemonic().to_owned(),

        // The fn-value verbs (#1679): one opcode, per-kind mnemonic —
        // mirrors the `.inkt` disassembly (`SeqVerbOp::mnemonic`).
        Opcode::SeqVerb(op) => op.mnemonic().to_owned(),
    }
}

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

fn sequence_kind(kind: SequenceKind) -> &'static str {
    match kind {
        SequenceKind::Cycle => "cycle",
        SequenceKind::Stopping => "stopping",
        SequenceKind::OnceOnly => "once_only",
        SequenceKind::Shuffle => "shuffle",
    }
}
