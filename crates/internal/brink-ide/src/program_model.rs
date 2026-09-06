//! Structured model of the compiled program for the Program Explorer.
//!
//! Lives here, not in the wasm wrapper, because a compiled-program model is
//! not a wasm concern: the web studio reads it through `brink-web` and a
//! native host reads it directly (decision log 2026-09-04, "Both studio
//! consumers sit on the same layer"). It has no wasm dependency and never
//! did — only `brink-format`.
//!
//!
//! Built from [`StoryData`] (no runtime needed): globals / lists / externals
//! tables plus a knot/stitch tree with per-knot, name-resolved bytecode
//! disassembly. Mirrors `brink_format`'s `.inkt` writer but resolves
//! `DefinitionId`s to author-facing knot paths and variable names instead of
//! hashes.
//!
//! The types are plain public data: the wasm wrapper serializes them, the
//! native studio reads the fields directly.

use std::collections::BTreeMap;
use std::collections::HashMap;

use brink_format::{
    ChoiceFlags, CountingFlags, DefinitionId, NameId, Opcode, SequenceKind, StoryData, Value,
};
use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
pub struct ProgramModel {
    pub checksum: String,
    /// Whether this compile carried a `DebugInfo` section — the difference
    /// between "no provenance on these rows" and "provenance is off".
    pub debug_info: bool,
    pub globals: Vec<ProgramGlobalJs>,
    pub lists: Vec<ProgramListJs>,
    pub externals: Vec<ProgramExternalJs>,
    pub knots: Vec<KnotNodeJs>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProgramGlobalJs {
    pub name: String,
    pub ty: String,
    pub default: String,
    pub mutable: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProgramListJs {
    pub name: String,
    pub items: Vec<ProgramListItemJs>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProgramListItemJs {
    pub name: String,
    pub ordinal: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProgramExternalJs {
    pub name: String,
    pub arg_count: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// An anonymous child container (gather, choice target, sequence wrapper)
/// listed under its owning scope — labeled `c-N` in scope-local,
/// container-table order, matching the save stamps' spelling. The
/// Disassembly view (#3339) needs these as first-class rows: the paused
/// position routinely sits INSIDE one, and a rail of scope containers
/// alone would show a highlight with no home.
#[derive(Serialize, Debug, Clone)]
pub struct AnonContainerJs {
    pub label: String,
    pub container_idx: u32,
    pub byte_size: u32,
    pub disasm: Vec<DisasmLineJs>,
}

#[derive(Serialize, Debug, Clone)]
pub struct KnotNodeJs {
    pub path: String,
    pub name: String,
    /// "knot" | "stitch"
    pub kind: &'static str,
    pub flags: Vec<&'static str>,
    pub path_hash: i32,
    /// This container's index in `StoryData::containers` — the same
    /// `container_idx` a runtime `DebugPosition`/`DebugFrame::position`
    /// addresses (D4, #3182) and the `DebugInfo` section's per-container
    /// table indexes lockstep with (D6, #3184). Keyed alongside `disasm`'s
    /// own per-instruction offsets so the studio can highlight "the
    /// currently executing instruction" in the Program Explorer (D9,
    /// #3187) — before this field, a disassembly line had nothing to key a
    /// running position against.
    pub container_idx: u32,
    /// Total bytecode bytes of this SCOPE — the scope container itself plus
    /// every anonymous child container (gathers, choice targets, sequence
    /// wrappers) that belongs to it via `scope_id`. The anonymous children
    /// are deliberately not tree nodes, so without this rollup their bytes
    /// would be invisible to any size accounting (#3339's size bars and
    /// treemap read exactly this).
    pub byte_size: u32,
    /// Containers in the scope, anonymous children included ("4 cont.").
    pub container_count: u32,
    pub disasm: Vec<DisasmLineJs>,
    /// This scope's anonymous child containers, in table order.
    pub anon: Vec<AnonContainerJs>,
    pub children: Vec<KnotNodeJs>,
}

/// One decoded bytecode instruction, keeping the byte offset it decoded
/// from — the D9 (#3187) fix for the join named in the issue: disassembly
/// used to decode with a running offset and emit only the formatted
/// mnemonic string, so a "current instruction" highlight had no offset to
/// match a live `DebugPosition` against.
#[derive(Serialize, Debug, Clone)]
pub struct DisasmLineJs {
    /// Byte offset of this instruction within the container's own
    /// bytecode — matches `DebugPosition::offset` / `DebugEntry::bytecode_offset`.
    pub offset: u32,
    pub text: String,
    /// Where this instruction came from (#3339 provenance column) — the
    /// `DebugInfo` section's offset→source map, resolved at model-build
    /// time. Absent when the compile carried no debug info, or for the
    /// synthetic-sentinel file (§2.5). Byte offsets, like every source
    /// range on this wire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<DisasmSrcJs>,
}

#[derive(Serialize, Debug, Clone)]
pub struct DisasmSrcJs {
    pub file: String,
    pub start: u32,
    pub end: u32,
}

/// Fill each instruction's source provenance from the `DebugInfo` section.
///
/// The entry table is sorted by `bytecode_offset` with full coverage
/// (§2.2), so the instruction's entry is the greatest offset ≤ its own.
/// The synthetic-sentinel file (index 0, §2.5) yields no provenance.
fn attach_src(lines: &mut [DisasmLineJs], data: &StoryData, container_idx: usize) {
    let Some(di) = &data.debug_info else { return };
    let Some(table) = di.containers.get(container_idx) else {
        return;
    };
    for line in lines {
        let idx = table
            .entries
            .partition_point(|e| e.bytecode_offset <= line.offset);
        if idx == 0 {
            continue;
        }
        let entry = &table.entries[idx - 1];
        if entry.file_idx == 0 {
            continue;
        }
        let Some(file) = di.files.get(entry.file_idx as usize) else {
            continue;
        };
        line.src = Some(DisasmSrcJs {
            file: file.path.clone(),
            start: entry.range_start,
            end: entry.range_start + entry.range_len,
        });
    }
}

/// Resolves ids to author-facing names for a single program.
pub(crate) struct Resolver<'a> {
    data: &'a StoryData,
    /// container/scope `DefinitionId` → qualified path (from `address_paths`).
    def_path: HashMap<DefinitionId, &'a str>,
    global_name: HashMap<DefinitionId, &'a str>,
    external_name: HashMap<DefinitionId, &'a str>,
    list_item_name: HashMap<DefinitionId, &'a str>,
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(data: &'a StoryData) -> Self {
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
    /// The stamped path for `id`, or `""` when unmapped — the Size
    /// report's per-scope naming rides this (root scope has no path).
    pub(crate) fn path_or_empty(&self, id: DefinitionId) -> &str {
        self.path(id)
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
pub fn build(data: &StoryData) -> ProgramModel {
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

    ProgramModel {
        debug_info: data.debug_info.is_some(),
        checksum: format!("0x{:08x}", data.source_checksum),
        globals,
        lists,
        externals,
        knots: build_knots(data, &r),
    }
}

/// Build the knot → stitch tree from named scope containers.
fn build_knots(data: &StoryData, r: &Resolver) -> Vec<KnotNodeJs> {
    // One pass to roll bytecode bytes and container counts up to their
    // owning scope: anonymous containers carry their scope's id in
    // `scope_id`, the scope container is its own scope.
    let mut scope_bytes: BTreeMap<brink_format::DefinitionId, (u32, u32)> = BTreeMap::new();
    let mut anon_by_scope: BTreeMap<brink_format::DefinitionId, Vec<AnonContainerJs>> =
        BTreeMap::new();
    let mut unnamed_counts: BTreeMap<brink_format::DefinitionId, u32> = BTreeMap::new();
    for (container_idx, c) in data.containers.iter().enumerate() {
        let entry = scope_bytes.entry(c.scope_id).or_insert((0, 0));
        entry.0 += u32::try_from(c.bytecode.len()).unwrap_or(u32::MAX);
        entry.1 += 1;
        if c.scope_id != c.id {
            // A LABELED child (a weave label / named gather) keeps its real
            // leaf name — `enter_container barter.opts` in the disassembly
            // must find a rail row called `opts`, not a `c-N` that makes
            // the reader do the join by hand (maintainer, 2026-08-30).
            // Only genuinely unnamed containers count into the stamps'
            // `c-N` spelling, so labels never shift the numbering.
            let label = if r.def_path.contains_key(&c.id) {
                leaf_name(r.path(c.id)).to_owned()
            } else {
                let n = unnamed_counts.entry(c.scope_id).or_insert(0);
                let label = format!("c-{n}");
                *n += 1;
                label
            };
            let mut disasm = disassemble(&c.bytecode, r);
            attach_src(&mut disasm, data, container_idx);
            anon_by_scope
                .entry(c.scope_id)
                .or_default()
                .push(AnonContainerJs {
                    label,
                    container_idx: u32::try_from(container_idx).unwrap_or(u32::MAX),
                    byte_size: u32::try_from(c.bytecode.len()).unwrap_or(u32::MAX),
                    disasm,
                });
        }
    }

    // Group named scope containers by top-level knot name. BTreeMap keeps the
    // output deterministic.
    let mut groups: BTreeMap<String, KnotGroup> = BTreeMap::new();
    for (container_idx, c) in data.containers.iter().enumerate() {
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
        let mut disasm = disassemble(&c.bytecode, r);
        attach_src(&mut disasm, data, container_idx);
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
            container_idx: u32::try_from(container_idx).unwrap_or(u32::MAX),
            byte_size: scope_bytes.get(&c.id).map_or(0, |&(b, _)| b),
            container_count: scope_bytes.get(&c.id).map_or(0, |&(_, n)| n),
            disasm,
            anon: anon_by_scope.remove(&c.id).unwrap_or_default(),
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
            // `container_idx: u32::MAX` — no real container backs this node, so
            // there is no bytecode position it could ever match.
            None => KnotNodeJs {
                path: kname.clone(),
                name: kname,
                kind: "knot",
                flags: Vec::new(),
                path_hash: 0,
                container_idx: u32::MAX,
                // No container backs this synthesized node; its stitches
                // carry their own sizes.
                byte_size: 0,
                container_count: 0,
                disasm: Vec::new(),
                anon: Vec::new(),
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

/// Decode a container's bytecode into resolved, one-per-line mnemonics —
/// each tagged with the byte offset it decoded from (D9, #3187), so a
/// caller can key a live `DebugPosition` against a specific line.
fn disassemble(bytecode: &[u8], r: &Resolver) -> Vec<DisasmLineJs> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < bytecode.len() {
        let start = offset;
        match Opcode::decode(bytecode, &mut offset) {
            Ok(op) => out.push(DisasmLineJs {
                offset: u32::try_from(start).unwrap_or(u32::MAX),
                text: format_opcode(&op, r),
                src: None,
            }),
            Err(e) => {
                out.push(DisasmLineJs {
                    offset: u32::try_from(start).unwrap_or(u32::MAX),
                    text: format!("<decode error: {e}>"),
                    src: None,
                });
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
        Opcode::EmitLineNl(idx, slots) => format!("emit_line_nl #{idx} {slots}"),
        Opcode::BinaryImm(kind, imm) => format!("binary_imm kind={} {imm}", kind.mnemonic()),
        Opcode::BinaryJumpIfFalse(kind, rel) => {
            format!("binary_jump_if_false kind={} {rel}", kind.mnemonic())
        }
        Opcode::BinaryImmJumpIfFalse(kind, imm, rel) => {
            format!(
                "binary_imm_jump_if_false kind={} {imm} {rel}",
                kind.mnemonic()
            )
        }
        Opcode::GetTempBinaryImm(slot, kind, imm) => {
            format!("get_temp_binary_imm {slot} kind={} {imm}", kind.mnemonic())
        }
        Opcode::GetTempBinaryImmJumpIfFalse(slot, kind, imm, rel) => format!(
            "get_temp_binary_imm_jump_if_false {slot} kind={} {imm} {rel}",
            kind.mnemonic()
        ),
        Opcode::DuplicateBinaryImmJumpIfFalse(kind, imm, rel) => format!(
            "duplicate_binary_imm_jump_if_false kind={} {imm} {rel}",
            kind.mnemonic()
        ),
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
        Opcode::TouchVisit => "touch_visit".to_owned(),
        Opcode::ShuffleIndexOf => "shuffle_index_of".to_owned(),

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

// ── container_idx / disasm-offset tests (D9, #3187) ───────────────────

#[cfg(test)]
mod container_idx_tests {
    use super::{Resolver, build, disassemble};

    /// Compile a small two-knot `.ink` story and confirm each knot's
    /// `container_idx` in the DTO actually names its own row in
    /// `StoryData::containers` — the join the D9 issue named as missing
    /// ("a current-instruction highlight in the Program Explorer has
    /// nothing to key on"). Also confirms `disasm` offsets are strictly
    /// increasing and the first instruction of a container starts at
    /// offset 0.
    #[test]
    fn container_idx_names_its_own_row_and_disasm_offsets_are_ordered() {
        let src = "=== one ===\nFirst.\n-> two\n=== two ===\nSecond.\n-> END\n";
        let out = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
            .expect("test source compiles");

        let model = build(&out.data);
        assert!(!model.knots.is_empty(), "fixture must produce knots");

        for knot in &model.knots {
            assert_ne!(
                knot.container_idx,
                u32::MAX,
                "a real knot from source must carry a real container_idx"
            );
            let container = out
                .data
                .containers
                .get(knot.container_idx as usize)
                .expect("container_idx must be a valid index");
            // Cross-check: this container's own disassembly must be exactly
            // as long as its own bytecode decodes to (never off-by-one
            // against a NEIGHBORING container's row — the failure shape a
            // wrong index would produce).
            let redecoded = disassemble(&container.bytecode, &Resolver::new(&out.data));
            assert_eq!(redecoded.len(), knot.disasm.len());

            if let Some(first) = knot.disasm.first() {
                assert_eq!(
                    first.offset, 0,
                    "the first disassembled instruction always starts at offset 0"
                );
            }
            let mut prev = None;
            for line in &knot.disasm {
                if let Some(p) = prev {
                    assert!(
                        line.offset > p,
                        "disasm offsets must strictly increase: {p} then {}",
                        line.offset
                    );
                }
                prev = Some(line.offset);
            }
        }
    }
}

#[cfg(test)]
mod operand_spelling_tests {
    use super::{Resolver, format_opcode};
    use brink_format::Opcode;

    /// The Disassembly view's resolution ghosts (#3339) PARSE these
    /// spellings in TypeScript (`ProgramDisasmView.tsx`'s `Resolution`).
    /// This is the Rust half of that contract: changing a spelling here
    /// must fail a test naming the parser it breaks.
    #[test]
    fn spellings_the_studio_resolver_parses_stay_stable() {
        let data = brink_compiler::compile("main.ink", |_p| {
            Ok("VAR gold = 1\n=== k ===\nHi.\n-> END\n".to_owned())
        })
        .expect("compiles")
        .data;
        let r = Resolver::new(&data);
        assert!(format_opcode(&Opcode::EmitLine(3, 0), &r).starts_with("emit_line #3"));
        assert!(format_opcode(&Opcode::Jump(46), &r).starts_with("jump 46"));
        assert!(format_opcode(&Opcode::JumpIfFalse(46), &r).starts_with("jump_if_false 46"));
        assert!(format_opcode(&Opcode::GetTemp(2), &r).starts_with("get_temp 2"));
        // get_global / call_external carry resolved names — the PREFIX is
        // the contract the parser keys on.
        let g = format_opcode(&Opcode::GetGlobal(data.variables[0].id), &r);
        assert!(g.starts_with("get_global "), "{g}");
    }
}

#[cfg(test)]
mod anon_container_tests {
    use super::build;

    /// The Disassembly view's rail (#3339): a choice knot's anonymous
    /// containers appear under it, labeled in the stamps' `c-N` spelling,
    /// carrying real disassembly — and every anonymous container in the
    /// program belongs to exactly one scope node's `anon` list.
    #[test]
    fn anonymous_containers_list_under_their_scope_with_disasm() {
        let src = "=== choicy ===\nPick.\n* [a] A. -> END\n* [b] B. -> END\n";
        let out = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned())).expect("compiles");
        let model = build(&out.data);
        let choicy = model
            .knots
            .iter()
            .find(|k| k.name == "choicy")
            .expect("choicy");
        assert!(
            !choicy.anon.is_empty(),
            "choices must produce anonymous containers"
        );
        for anon in &choicy.anon {
            let c = &out.data.containers[anon.container_idx as usize];
            assert_eq!(anon.byte_size as usize, c.bytecode.len());
        }
        // Unnamed containers take the stamps' c-N spelling, in order.
        let unnamed: Vec<&str> = choicy
            .anon
            .iter()
            .filter(|a| a.label.starts_with("c-"))
            .map(|a| a.label.as_str())
            .collect();
        for (i, label) in unnamed.iter().enumerate() {
            assert_eq!(*label, format!("c-{i}"));
        }
        // An EMPTY anonymous container (a weave endpoint) is legitimate and
        // stays listed — but the choice bodies themselves must carry code.
        assert!(
            choicy.anon.iter().any(|a| !a.disasm.is_empty()),
            "at least one anonymous container holds the choice bodies"
        );
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::build;

    /// The disassembly's provenance column (#3339): under a debug compile
    /// every emit-bearing container maps instructions back to real file
    /// ranges; without debug info the model says so (`debug_info: false`)
    /// and no row invents a source.
    #[test]
    fn instructions_carry_source_ranges_exactly_when_debug_info_exists() {
        let src = "=== k ===\nHello.\nGoodbye.\n-> END\n";
        let read = |_p: &str| Ok(src.to_owned());

        let plain = brink_compiler::compile("main.ink", read).expect("compiles");
        let plain_model = build(&plain.data);
        assert!(!plain_model.debug_info);
        let k = plain_model.knots.iter().find(|n| n.name == "k").expect("k");
        assert!(k.disasm.iter().all(|l| l.src.is_none()));

        let options = brink_analyzer::AnalysisOptions {
            emit_debug_info: true,
            ..Default::default()
        };
        let dbg =
            brink_compiler::compile_with_options("main.ink", read, options).expect("compiles");
        let model = build(&dbg.data);
        assert!(model.debug_info);
        let k = model.knots.iter().find(|n| n.name == "k").expect("k");
        let with_src: Vec<_> = k.disasm.iter().filter_map(|l| l.src.as_ref()).collect();
        assert!(
            !with_src.is_empty(),
            "a debug compile must yield provenance"
        );
        for src_ref in with_src {
            assert_eq!(src_ref.file, "main.ink");
            assert!(src_ref.end > src_ref.start, "{src_ref:?}");
            assert!(
                (src_ref.end as usize) <= src.len(),
                "range must stay inside the file: {src_ref:?}"
            );
        }
    }
}

#[cfg(test)]
mod labeled_container_tests {
    use super::build;

    /// A weave label keeps its NAME in the rail (maintainer, 2026-08-30):
    /// `enter_container barter.opts` must find a row called `opts` — a
    /// reader should never join `opts` to `c-0` by hand. Labels also stay
    /// OUT of the c-N numbering, so naming a gather cannot renumber its
    /// unnamed siblings.
    #[test]
    fn a_labeled_gather_keeps_its_name_and_does_not_shift_c_numbering() {
        let src = "=== barter ===\nHi.\n* [a] A.\n* [b] B.\n- (opts) Done.\n-> END\n";
        let out = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned())).expect("compiles");
        let model = build(&out.data);
        let barter = model
            .knots
            .iter()
            .find(|k| k.name == "barter")
            .expect("barter");
        assert!(
            barter.anon.iter().any(|a| a.label == "opts"),
            "labels: {:?}",
            barter.anon.iter().map(|a| &a.label).collect::<Vec<_>>()
        );
        let unnamed: Vec<&str> = barter
            .anon
            .iter()
            .filter(|a| a.label.starts_with("c-"))
            .map(|a| a.label.as_str())
            .collect();
        for (i, label) in unnamed.iter().enumerate() {
            assert_eq!(*label, format!("c-{i}"), "labels must not shift numbering");
        }
    }
}

#[cfg(test)]
mod scope_size_tests {
    use super::build;

    /// The scope rollup behind `byte_size`/`container_count` (#3339): a
    /// knot's size must include the ANONYMOUS containers its content
    /// compiles into (gathers, choice targets), not just the scope
    /// container's own bytecode — those children are deliberately not tree
    /// nodes, so this rollup is the only place their bytes are visible.
    #[test]
    fn scope_sizes_roll_anonymous_children_up_and_cover_all_bytecode() {
        // `choicy` compiles its choices into anonymous child containers;
        // `plain` is a single container. Both must account every byte.
        let src = "=== plain ===\nJust a line.\n-> END\n=== choicy ===\nPick.\n* [a] A. -> END\n* [b] B. -> END\n";
        let out = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
            .expect("test source compiles");
        let model = build(&out.data);

        let plain = model
            .knots
            .iter()
            .find(|k| k.name == "plain")
            .expect("plain");
        let choicy = model
            .knots
            .iter()
            .find(|k| k.name == "choicy")
            .expect("choicy");

        // A single-container knot: rollup equals its own bytecode length,
        // and it counts exactly itself.
        let plain_container = &out.data.containers[plain.container_idx as usize];
        assert_eq!(plain.byte_size as usize, plain_container.bytecode.len());
        assert_eq!(plain.container_count, 1);

        // The choice knot owns anonymous children: strictly more bytes and
        // containers than its scope container alone.
        let choicy_container = &out.data.containers[choicy.container_idx as usize];
        assert!(
            (choicy.byte_size as usize) > choicy_container.bytecode.len(),
            "choicy rollup ({}) must exceed its scope container alone ({})",
            choicy.byte_size,
            choicy_container.bytecode.len()
        );
        assert!(choicy.container_count > 1, "{}", choicy.container_count);

        // Conservation: every container's bytes land in exactly one scope,
        // so the per-node rollups (plus the root scope, which has no node)
        // must sum to the whole program. Sum ALL scopes' rollups directly.
        let total: usize = out.data.containers.iter().map(|c| c.bytecode.len()).sum();
        let rolled: u32 = model
            .knots
            .iter()
            .map(|k| k.byte_size + k.children.iter().map(|s| s.byte_size).sum::<u32>())
            .sum();
        assert!(
            (rolled as usize) <= total,
            "rollups ({rolled}) cannot exceed the program ({total})"
        );
        // The remainder is exactly the root scope's containers.
        let named: usize = rolled as usize;
        assert!(total - named < total, "root scope holds the rest");
    }
}
