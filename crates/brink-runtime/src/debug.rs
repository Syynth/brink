//! Read-only debug introspection for the studio State View.
//!
//! [`Story::debug_snapshot`](crate::Story::debug_snapshot) produces a
//! [`DebugSnapshot`] — a name-resolved, structured view of the runtime's
//! current state (location, globals, call stack, visit counts, pending
//! choices, rng). Unlike the VM internals, everything here is resolved to
//! author-facing knot/stitch paths and variable names.
//!
//! This is built on demand and is not on any hot path.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use brink_format::{DefinitionId, Value};

use crate::collections::Map as HashMap;
use crate::program::Program;
use crate::value_ops;

/// Precise execution position: a container index plus the byte offset of
/// the next instruction to execute inside that container's bytecode.
///
/// A public mirror of the runtime-internal
/// `story::call_stack::ContainerPosition` — deliberately a distinct type
/// (issue #3182) rather than that type made `pub`, so the VM's internal
/// call-frame layout stays free to change without turning into a de facto
/// compatibility surface. `container_idx` is stable within one linked
/// [`Program`] (it indexes the same `Containers` table
/// [`Program::container_bytecode`] reads and the table
/// `docs/debugger-spec.md` §2.2's `DebugInfo` section addresses
/// lockstep-by-index); `offset` is a byte offset into that container's
/// bytecode, not a source location — resolving position to source is a
/// later workstream (D6/D9, `docs/debugger-spec.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugPosition {
    /// Index into the linked program's container table.
    pub container_idx: u32,
    /// Byte offset into that container's bytecode.
    pub offset: usize,
}

/// The result of resolving a [`DebugPosition`] to source via the program's
/// `DebugInfo` section (D6, `docs/debugger-spec.md` §2.2) — see
/// [`crate::Program::resolve_debug_position`] (D9, issue #3187). This is
/// the "program → source" half of the studio's Location protocol
/// (`docs/studio-shell-spec.md` §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSourceLocation {
    /// Project-root-relative source path, or `None` for the reserved
    /// synthetic sentinel file (compiler-generated content with no author
    /// source).
    pub file: Option<String>,
    /// Absolute source byte offset within `file`.
    pub range_start: u32,
    /// Length in bytes of the source range (`range_start + range_len` is
    /// the exclusive end).
    pub range_len: u32,
}

/// A structured, read-only snapshot of the runtime's current state.
pub struct DebugSnapshot {
    /// Execution status: `active` / `waiting_for_choice` / `done` / `ended`.
    pub status: &'static str,
    /// Nearest named knot/stitch the cursor is currently in, if resolvable.
    pub current_location: Option<String>,
    /// Precise execution position for the active flow: the innermost call
    /// frame's current `(container_idx, offset)`. `None` when the call
    /// stack has no frame with an open container (e.g. `Ended`/`Done` with
    /// nothing left to run) — mirrors `call_stack[0].position` when the
    /// call stack is non-empty. Also `None` whenever the innermost frame is
    /// a `CallFrameType::External` frame: those are pushed with an empty
    /// `container_stack` (there is no bytecode position "inside" a
    /// deferred external call), and an external frame stays the top frame
    /// for as long as `has_pending_external()` is true — so a flow parked
    /// on a deferred external (`StepOutcome::AwaitingExternal`, status
    /// `active`, nothing exhausted) reports `position: None` here too. The
    /// call site that invoked the external lives on that frame's own
    /// `return_address`, or equivalently on the position of the caller
    /// frame just below it.
    pub position: Option<DebugPosition>,
    /// Current turn index.
    pub turn_index: u32,
    /// Global variables and their current values (display strings).
    pub globals: Vec<DebugGlobal>,
    /// Active call frames, innermost (current) first.
    pub call_stack: Vec<DebugFrame>,
    /// Per-knot/stitch visit counts, sorted by path.
    pub visit_counts: Vec<DebugVisit>,
    /// EVERY container visit count keyed by `DefinitionId` display string
    /// (W11/#3304): unlike [`Self::visit_counts`] — which resolves through
    /// named paths and silently drops anonymous containers — this carries
    /// the anonymous choice/gather bodies too, so a consumer can join
    /// against the HIR overlay projection's `def_id` (#3234's identity).
    /// Sorted by id for determinism.
    pub visit_ids: Vec<DebugVisitId>,
    /// Choices currently offered to the player.
    pub pending_choices: Vec<DebugChoice>,
    /// Story RNG state.
    pub rng: DebugRng,
}

/// A global variable and its current value.
pub struct DebugGlobal {
    pub name: String,
    pub value: String,
}

/// One call frame, resolved to a knot/stitch path.
pub struct DebugFrame {
    /// Frame kind: `root` / `function` / `tunnel` / `thread` / `external` / `eval`.
    pub kind: &'static str,
    /// Nearest named container for this frame, if resolvable.
    pub location: Option<String>,
    /// This frame's current `(container_idx, offset)` — the next
    /// instruction that will execute if/when this frame becomes active
    /// again. `None` for a frame whose container stack is empty (exhausted,
    /// nothing left to run in it) — which includes every
    /// `CallFrameType::External` frame: those are pushed with an empty
    /// `container_stack` (there is no bytecode position "inside" a
    /// deferred external call), so a frame with `kind == "external"`
    /// always carries `position: None`. The call site that invoked it
    /// lives on this frame's own `return_address`, or equivalently on the
    /// position of the caller frame just below it.
    pub position: Option<DebugPosition>,
    /// Number of temporary (local) variables in this frame.
    pub temps: usize,
    /// D7 (`docs/debugger-spec.md` §3, #3185): this frame's named locals —
    /// every declared parameter/`~ temp` slot in the frame's own scope,
    /// bound to its live value. Additive alongside `temps` (D4's bare
    /// count stays, unchanged, for any existing consumer). `None` when
    /// this artifact carries no `DebugInfo` (a release-exported story, or
    /// one compiled before D6) **or** the frame's container stack is
    /// empty (nothing left to run in it — e.g. every `external` frame, or
    /// an exhausted frame — so there is no current position to resolve a
    /// scope from). A frame with a `DebugInfo`-backed program, a live
    /// position, but genuinely zero declared locals reports `Some(vec![])`,
    /// not `None`, so a consumer can tell "no debug info available" (or
    /// "nothing to resolve from") apart from "this frame really has no
    /// locals."
    ///
    /// Resolved via [`crate::program::Program::scope_debug_locals`] against
    /// the frame's *current leaf* container, grouping by lexical **scope**
    /// (`ContainerDef::scope_id`) rather than by whatever containers
    /// currently happen to be on `container_stack` — deliberately: a call
    /// frame's `container_stack` can legitimately shrink back to a single
    /// entry mid-frame (`vm::goto_target`'s "target not already on the
    /// stack" branch clears and replaces it wholesale, e.g. entering a
    /// `{? … }` choice-target body from its enclosing knot), which would
    /// silently drop an enclosing container's own parameters/`~ temp`s
    /// from view the instant the leaf position moved into a *sibling*
    /// child container — even though the call frame's `temps` are
    /// completely unaffected by that navigation (`docs/debugger-spec.md`
    /// §3: VM temp slots "are allocated per active call frame, not
    /// lexically nested"). On a slot collision (only possible if a future
    /// codegen change reuses a slot number across sibling scopes — not
    /// confirmed to happen today, same spec section) the entry from
    /// whichever sibling container was iterated last wins; see
    /// `scope_debug_locals`'s own doc for the full reasoning.
    pub locals: Option<Vec<DebugLocal>>,
}

/// One named local variable and its current value (`docs/debugger-spec.md`
/// §3, D7/#3185).
#[derive(Debug, Clone)]
pub struct DebugLocal {
    /// The VM temp slot this local occupies in the call frame — matches
    /// `DeclareTemp`/`GetTemp`/`SetTemp`'s `u16` operand.
    pub slot: u16,
    pub name: String,
    pub value: DebugValue,
}

/// A structured, read-only view of a runtime [`Value`] for the debugger's
/// locals panel (`docs/debugger-spec.md` §3, D7/#3185).
///
/// **Why structured, not another display string.** [`DebugGlobal::value`]
/// is a display string (`String`) — that shape predates this ticket and
/// stays as-is (globals are out of D7's scope, and changing an existing
/// public field is not "additive"). For locals, a display string is the
/// wrong shape to repeat: the issue's own bar is that a debugger which can
/// only show `"[list]"` for a list value is not the target, and a plain
/// string can't do better than that — a studio locals panel needs to tell
/// "this is a list with these members" from "this is a string that reads
/// like a list" to render either one correctly (or let a user expand a
/// struct's fields, or distinguish a null from an empty string). So D7
/// exposes the value's *kind* structurally for every kind the runtime
/// distinguishes that the issue calls out by name (int, float, string,
/// list, divert target, struct, handle), plus the common `bool`/`null`
/// cases that cost nothing extra to model — and falls back to the existing
/// [`NameResolver::format_value`] display string ([`DebugValue::Other`])
/// for the long tail of kinds this ticket has no author-facing UI need to
/// special-case yet (closures, arrays, maps, weighted tables, function
/// refs, variable/temp pointers, fragment refs, vector/matrix/quaternion,
/// ranges, options, projections). `Other` is not a cop-out for the kinds
/// the issue named — those all get real variants below.
#[derive(Debug, Clone)]
pub enum DebugValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    Str(String),
    Null,
    /// A list value's member names, in list order (unresolvable items are
    /// skipped, matching `NameResolver::format_value`'s existing behavior).
    List(Vec<String>),
    /// A divert-target value, resolved to its author-facing knot/stitch
    /// path where possible. `None` when the target isn't resolvable to a
    /// named scope (matches `format_value`'s `"-> ?"` case).
    DivertTarget(Option<String>),
    /// A struct (`Value::Record`) value: its declared shape name (if
    /// resolvable) and its fields, named and recursively structured.
    Struct {
        name: Option<String>,
        fields: Vec<(String, DebugValue)>,
    },
    /// A handle value (T1d): its manifest-declared kind name and the
    /// host-allocated token id.
    Handle {
        kind: String,
        id: u64,
    },
    /// Every other value kind's existing display-string form
    /// ([`NameResolver::format_value`]) — see this enum's own doc for why.
    Other(String),
}

/// A visit count for a named knot/stitch.
pub struct DebugVisit {
    pub path: String,
    pub count: u32,
}

/// A visit count keyed by `DefinitionId` (W11/#3304) — includes anonymous
/// choice/gather body containers, which have no path.
pub struct DebugVisitId {
    /// `DefinitionId` display form (`$tt_hash`) — string-equal to the HIR
    /// overlay projection's `def_id` for the same container.
    pub def_id: String,
    pub count: u32,
}

/// A pending choice and the knot it targets.
pub struct DebugChoice {
    pub text: String,
    /// `+` (sticky) vs `*` (once-only), as written (#3435) — see
    /// [`Choice::sticky`](crate::story::Choice::sticky).
    pub sticky: bool,
    /// The choice text's source location (#3435) — see
    /// [`Choice::source`](crate::story::Choice::source).
    pub source: Option<brink_format::SourceLocation>,
    pub target: Option<String>,
    /// The choice target's `DefinitionId` display form (W11/#3304) —
    /// string-equal to the overlay projection's `def_id` for the choice's
    /// own container, joining a PRESENTED choice back to its source span.
    pub def_id: String,
    /// The raw `flow.pending_choices` index — the same pre-filter position
    /// the visible [`Choice`](crate::story::Choice)'s `index` carries and
    /// that `select_choice`/`choose` expects. Not a post-filter enumeration
    /// position: invisible-default choices are filtered out of what's shown
    /// but still occupy a slot in `pending_choices`, so this can skip values.
    pub index: usize,
}

/// Story RNG state.
pub struct DebugRng {
    pub seed: i32,
    pub previous: i32,
}

/// Resolves container indices / definition ids to author-facing paths and
/// formats values for display. Holds a one-time reverse map of the program's
/// `address_by_path` table.
pub(crate) struct NameResolver<'p> {
    program: &'p Program,
    /// `container_idx → shortest knot/stitch path` (offset-0 scope entries).
    rev: HashMap<u32, String>,
}

impl<'p> NameResolver<'p> {
    pub(crate) fn new(program: &'p Program) -> Self {
        let mut rev: HashMap<u32, String> = HashMap::new();
        for (path, target) in &program.address_by_path {
            if target.byte_offset != 0 {
                continue;
            }
            let idx = &target.container_idx;
            // Deterministic on collision: shortest path, then lexicographically
            // smallest — independent of HashMap iteration order.
            let better = match rev.get(idx) {
                None => true,
                Some(existing) => {
                    path.len() < existing.len()
                        || (path.len() == existing.len() && path.as_str() < existing.as_str())
                }
            };
            if better {
                rev.insert(*idx, path.clone());
            }
        }
        Self { program, rev }
    }

    /// The knot/stitch path for a container, if it names a scope.
    pub(crate) fn container_path(&self, idx: u32) -> Option<&str> {
        self.rev.get(&idx).map(String::as_str)
    }

    /// The knot/stitch path a definition id lives in, if resolvable.
    pub(crate) fn def_path(&self, id: DefinitionId) -> Option<&str> {
        let (idx, _) = self.program.resolve_target(id)?;
        self.container_path(idx)
    }

    /// Structured value view for the debugger's locals panel
    /// (`docs/debugger-spec.md` §3, D7/#3185) — see [`DebugValue`]'s own
    /// doc for why this exists alongside `format_value` rather than
    /// replacing it.
    pub(crate) fn debug_value(&self, value: &Value) -> DebugValue {
        match value {
            Value::Int(i) => DebugValue::Int(*i),
            Value::Float(f) => DebugValue::Float(*f),
            Value::Bool(b) => DebugValue::Bool(*b),
            Value::String(s) => DebugValue::Str(s.to_string()),
            Value::Null => DebugValue::Null,
            Value::List(list) => DebugValue::List(
                list.items
                    .iter()
                    .filter_map(|id| self.program.list_item_name(*id))
                    .map(str::to_owned)
                    .collect(),
            ),
            Value::DivertTarget(id) => {
                DebugValue::DivertTarget(self.def_path(*id).map(str::to_owned))
            }
            Value::Record { shape, fields } => {
                let shape_entry = self.program.struct_shape(*shape);
                let name = shape_entry
                    .and_then(|s| self.program.name_checked(s.name))
                    .map(str::to_owned);
                let field_names: Vec<&str> = shape_entry.map_or_else(Vec::new, |s| {
                    s.fields
                        .iter()
                        .map(|&n| self.program.name_checked(n).unwrap_or("?"))
                        .collect()
                });
                let fields = fields
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let field_name = field_names
                            .get(i)
                            .map_or_else(|| format!("_{i}"), |n| (*n).to_owned());
                        (field_name, self.debug_value(v))
                    })
                    .collect();
                DebugValue::Struct { name, fields }
            }
            Value::Handle { kind, id } => DebugValue::Handle {
                kind: self.program.name_checked(*kind).unwrap_or("?").to_owned(),
                id: *id,
            },
            other => DebugValue::Other(self.format_value(other)),
        }
    }

    /// Format a runtime value for display, resolving names where possible.
    pub(crate) fn format_value(&self, value: &Value) -> String {
        match value {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => format!("\"{s}\""),
            Value::Null => "null".to_owned(),
            Value::List(list) => {
                let members: Vec<&str> = list
                    .items
                    .iter()
                    .filter_map(|id| self.program.list_item_name(*id))
                    .collect();
                format!("({})", members.join(", "))
            }
            Value::DivertTarget(id) => match self.def_path(*id) {
                Some(p) => format!("-> {p}"),
                None => "-> ?".to_owned(),
            },
            Value::VariablePointer(id) => match self.program.global_var_name(*id) {
                Some(n) => format!("ref {n}"),
                None => "ref ?".to_owned(),
            },
            Value::TempPointer { slot, frame_depth } => {
                format!("temp[{slot}]@{frame_depth}")
            }
            Value::FragmentRef(idx) => format!("<fragment {idx}>"),
            Value::Array(items) => {
                let parts: Vec<String> = items.iter().map(|v| self.format_value(v)).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Map(map) => {
                let parts: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{}: {}", format_map_key(k), self.format_value(v)))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            // Weighted tables (NS-A7): mirror the construction literal,
            // entries in construction order.
            Value::Weighted(w) => {
                let parts: Vec<String> = w
                    .entries
                    .iter()
                    .map(|(weight, v)| format!("{weight}: {}", self.format_value(v)))
                    .collect();
                format!("Weighted {{ {} }}", parts.join(", "))
            }
            Value::Record { shape, fields } => {
                let parts: Vec<String> = fields.iter().map(|v| self.format_value(v)).collect();
                format!("Record#{}{{{}}}", shape.0, parts.join(", "))
            }
            // Function values (T1c, #700). Debug rendering resolves the target
            // path where possible and shows the bound env; the author-facing
            // `string(f)` display form (spec §5) lands in T1c-3.
            Value::FnRef(target) => match self.def_path(*target) {
                Some(p) => format!("fn {p}"),
                None => "fn ?".to_owned(),
            },
            Value::Closure(c) => {
                let name = self.def_path(c.target).unwrap_or("?");
                let parts: Vec<String> = c
                    .env
                    .iter()
                    .map(|e| {
                        let mode = if e.is_ref { "ref" } else { "val" };
                        format!("{mode} {}", self.format_value(&e.payload))
                    })
                    .collect();
                format!("fn {name}({})", parts.join(", "))
            }
            // Handle values (T1d, `docs/t1d-spec.md` §6). Same display form
            // as the runtime's authoritative `string(h)` (`value_ops::stringify`):
            // `handle <Kind>#<id>`, resolved via the program's name table.
            Value::Handle { kind, id } => {
                let kind_name = self.program.name_checked(*kind).unwrap_or("?");
                format!("handle {kind_name}#{id}")
            }
            // Projection values (T1e, `docs/t1e-spec.md` §4). Same display
            // form as the runtime's authoritative `string(p)`
            // (`value_ops::stringify`).
            // Range values (NS-A5, F7) share the authoritative display too:
            // the written `0..10` / `1..=6` form.
            // Tower values (NS-A8): same display form as the runtime's
            // authoritative `string(v)` (`value_ops::stringify`).
            Value::Projection(_)
            | Value::OptionVal(_)
            | Value::Range { .. }
            | Value::Vec2(_)
            | Value::Vec3(_)
            | Value::Vec4(_)
            | Value::Quat(_)
            | Value::Mat2(_)
            | Value::Mat3(_)
            | Value::Mat4(_) => value_ops::stringify(value, self.program),
        }
    }
}

/// Format a map key for debug display.
fn format_map_key(key: &brink_format::MapKey) -> String {
    match key {
        brink_format::MapKey::Int(n) => n.to_string(),
        brink_format::MapKey::Str(s) => format!("\"{s}\""),
        brink_format::MapKey::Bool(b) => b.to_string(),
    }
}
