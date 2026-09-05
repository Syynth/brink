//! Immutable linked program.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use brink_format::{
    AliasEntry, CountingFlags, DebugInfoSection, DefinitionId, ListValue, NameId, ShapeId, Value,
};

use crate::collections::Map as HashMap;
use crate::error::RuntimeError;

/// A linked, ready-to-execute program.
///
/// Created from [`StoryData`](brink_format::StoryData) via [`link()`](crate::link).
/// Immutable after creation — mutable per-instance state lives in [`Story`](crate::Story).
pub struct Program {
    pub(crate) containers: Vec<LinkedContainer>,
    /// What the linker derived from the symbolic bytecode: see [`LinkTables`].
    pub(crate) link: LinkTables,
    /// Unified address map: `id → (container_idx, byte_offset)`.
    /// Contains both container IDs (offset 0) and intra-container addresses.
    pub(crate) address_map: HashMap<DefinitionId, (u32, usize)>,
    /// Scope `DefinitionId` for each entry in the line tables (parallel vec).
    /// Structural metadata — does not change with locale.
    pub(crate) scope_ids: Vec<DefinitionId>,
    /// CRC-32 checksum from the source `.inkb`, used for locale validation.
    pub(crate) source_checksum: u32,
    pub(crate) globals: Vec<GlobalSlot>,
    pub(crate) global_map: HashMap<DefinitionId, u32>,
    pub(crate) name_table: Vec<String>,
    /// Map from a knot/stitch path string to its target: the defining
    /// `DefinitionId` plus the resolved `(container_idx, byte_offset)`.
    /// Built at link time from named scope containers; lets consumers
    /// spawn flows at named entry points without needing `DefinitionId`s.
    pub(crate) address_by_path: HashMap<String, PathTarget>,
    /// `container_idx → shortest knot/stitch path` for every container that
    /// is the offset-0 target of an author-facing path — the reverse of
    /// `address_by_path`, built once at link time. Backs
    /// [`Program::container_path`] and, through it, the runtime's
    /// [`current_path`](crate::Story::current_path) query and the
    /// debugger's location names. Deterministic on collision: the shortest
    /// path, then the lexicographically smallest.
    pub(crate) container_paths: HashMap<u32, String>,
    pub(crate) root_idx: u32,
    /// List literal values referenced by `PushList(idx)`.
    pub(crate) list_literals: Vec<ListValue>,
    /// The T1b literal pool: constant collection values referenced by
    /// `PushLiteral(idx)` (`docs/format-v4-rfc.md` §2).
    pub(crate) literal_pool: Vec<Value>,
    /// Per-item metadata keyed by item `DefinitionId`.
    pub(crate) list_item_map: HashMap<DefinitionId, ListItemEntry>,
    /// List definitions indexed by position.
    pub(crate) list_defs: Vec<ListDefEntry>,
    /// Map from list def `DefinitionId` to index in `list_defs`.
    pub(crate) list_def_map: HashMap<DefinitionId, usize>,
    /// External function metadata keyed by the external function's `DefinitionId`.
    pub(crate) external_fns: HashMap<DefinitionId, ExternalFnEntry>,
    /// Compiled flow-private scope defaults for knots/stitches: the
    /// `(path, id)` of every scope container the compiler marked
    /// `#@local`, sorted by path so knots seed before their stitches.
    /// The base layer of `WorldPolicy` resolution
    /// (`docs/directive-annotations-spec.md`).
    pub(crate) local_scope_defaults: Vec<(String, DefinitionId)>,
    /// TM-4 `StructShapes` table (`docs/typed-mode-spec.md` §6), indexed by
    /// `ShapeId` — every `RecordNew`/`RecordGetDyn`/`RecordSetDyn` opcode
    /// looks a shape up here for its field count and field-name → offset
    /// mapping. Empty until a compiler milestone emits `STRUCT`
    /// declarations.
    pub(crate) struct_shapes: Vec<StructShapeEntry>,
    /// M-2b (`docs/modules-spec.md` §4): the set of `#@private` definition
    /// ids. Used only to refuse host **semantic** access (variable get/set,
    /// entry lookup, function eval) — the VM and host **persistence** never
    /// consult it, so private state still executes and still saves/loads.
    /// Empty (and never consulted) for the all-public pre-modules world.
    /// Sorted ascending by raw id (the linker sorts it), so membership is a
    /// `binary_search` — no set type needed for a list that is typically empty
    /// or tiny, and `no_std`-clean.
    pub(crate) private_defs: Vec<DefinitionId>,
    /// M-3 (`docs/modules-spec.md` §5): the compiled `#@was` alias table,
    /// sorted by `old` — [`Program::resolve_alias`] binary-searches it.
    /// Empty for every story that uses no `#@was`.
    pub(crate) alias_table: Vec<AliasEntry>,
    /// D6's `DebugInfo` section (`docs/debugger-spec.md` §2, `.inkb` tag
    /// `0x11`), carried through unchanged from `StoryData::debug_info` —
    /// `None` for a release-exported / non-debug compile (§1.2's ship
    /// policy, never requested) or any story compiled before D6. Two
    /// consumers, both reading it lockstep with the `Containers` table
    /// this `Program` already links (no `DefinitionId` lookup on either
    /// read path, per the section's own design):
    ///
    /// - [`Program::resolve_debug_position`] (D9, #3187) — a running
    ///   `(container_idx, offset)` position resolves to a source range by
    ///   direct index into `containers[container_idx]`.
    /// - [`Program::scope_debug_locals`] (D7, #3185), behind
    ///   [`crate::debug::DebugFrame::locals`] — §3's per-container
    ///   `LocalsTable` names a call frame's live temp slots.
    pub(crate) debug_info: Option<DebugInfoSection>,
}

/// Runtime metadata for one declared struct shape.
pub(crate) struct StructShapeEntry {
    /// The declared `STRUCT` name — the head of the structural display
    /// default (`Point { x: 1, y: 2 }`, NS-A3 / stdlib-spec §9.6).
    pub name: NameId,
    /// Declared field names, in shape order — the same order
    /// [`brink_format::Value::Record`]'s flat field vector follows.
    pub fields: Vec<NameId>,
}

/// A static jump/call target, resolved once by the linker.
///
/// `id` is kept alongside the position because the VM still needs the
/// address identity at run time — visit and turn counts are keyed by it —
/// and the two rulings behind hot-reload (decision log 2026-03-01) make the
/// id the stable identity across relinks while `container_idx` is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LinkedTarget {
    pub container_idx: u32,
    pub offset: usize,
    pub id: DefinitionId,
}

/// The linker's derived layer over the symbolic bytecode.
///
/// `.inkb` stores `DefinitionId`s in every jump and call, with no
/// compile-time indices (decision log 2026-03-01, "`.inkb` stores
/// `ContainerId`s … resolved to fast internal indices at load time"). Until
/// this table existed, "at load time" meant a hash lookup on every
/// `Goto`/`Call`/`BeginChoice` the VM executed — 1.7% of `TheIntercept`'s
/// instructions in `Program::resolve_target`. Now the linker walks each
/// container once, interns every resolvable static target into `targets`,
/// and writes the target's ordinal over the operand bytes in its own copy
/// of the code (`code[i]` is `containers[i].bytecode` with those operands
/// rewritten — same length, same offsets). The VM indexes `targets` by
/// that ordinal; nothing hashes. A global's operand (`GetGlobal`,
/// `SetGlobal`, `TakeGlobal`) is rewritten the same way to its slot index in
/// `globals`, which is already dense, so it needs no table of its own.
///
/// The symbolic `bytecode` stays untouched on each `LinkedContainer` for
/// every other decoder (the debugger, `container_bytecode`, tests):
/// `Opcode::decode` is not defined over the rewritten copy. An operand the
/// linker cannot resolve is left symbolic in `code` too, so an unresolved
/// divert still fails exactly where and how it did before — when reached.
///
/// Rebuilt on every link, so a hot patch that renumbers containers simply
/// produces a new table; nothing here is persisted.
#[derive(Debug, Clone, Default)]
pub(crate) struct LinkTables {
    pub code: Vec<Vec<u8>>,
    pub targets: Vec<LinkedTarget>,
}

/// The operand at a static site of *linked* code: `Some(n)` — a target's
/// ordinal into [`LinkTables::targets`], or a global's slot index — when the
/// linker resolved it, `None` when the bytes still hold a symbolic
/// `DefinitionId`.
///
/// The two are distinguishable by the last byte: an id's top byte is its
/// `DefinitionTag`, never zero (`brink_format::DefinitionTag` starts at
/// `0x01`), while an ordinal is written as a little-endian `u64` whose top
/// four bytes are zero.
pub(crate) fn linked_ordinal(operand: &[u8]) -> Option<u32> {
    let raw = u64::from_le_bytes(operand.try_into().ok()?);
    #[expect(clippy::cast_possible_truncation, reason = "top 32 bits checked zero")]
    (raw >> 32 == 0).then_some(raw as u32)
}

/// The linked form of a target ordinal — the inverse of [`linked_ordinal`].
pub(crate) fn linked_operand(ordinal: u32) -> [u8; 8] {
    u64::from(ordinal).to_le_bytes()
}

pub(crate) struct LinkedContainer {
    pub id: DefinitionId,
    pub bytecode: Vec<u8>,
    pub counting_flags: CountingFlags,
    pub path_hash: i32,
    /// Number of declared parameters (for arity-checking host-directed entry).
    pub param_count: u8,
    /// Per-parameter name/mode metadata, in declared order (T1c, #700).
    /// Empty for containers the converter produced or that declare no params;
    /// used by function-value dispatch to validate a rehydrated closure's
    /// bound env against the current signature.
    pub params: Vec<brink_format::ParamMeta>,
    /// Index into `Program.line_tables` for this container's scope line table.
    pub scope_table_idx: u32,
    /// The lexical scope this container belongs to (`ContainerDef::
    /// scope_id`'s own doc: `scope_id == id` for a scope container itself —
    /// root/knot/stitch — and the enclosing scope's `id` for a child
    /// container — gather, choice target, sequence branch, etc.). D7
    /// (`docs/debugger-spec.md` §3, #3185): the key
    /// [`Program::scope_debug_locals`] groups by, since a call frame's
    /// `container_stack` can legitimately drop back to a single entry
    /// mid-frame (`vm::goto_target`'s "target not already on the stack"
    /// branch clears and replaces it — verified on `origin/main`) while the
    /// frame's declared locals stay live in its `temps` regardless of which
    /// child container the current leaf position sits in.
    pub scope_id: DefinitionId,
}

pub(crate) struct GlobalSlot {
    /// The global's `DefinitionId` — used by save/load and by M-2b
    /// visibility enforcement ([`Program::global_is_private`]).
    pub id: DefinitionId,
    pub name: NameId,
    pub default: Value,
    /// Compiled flow-private (`#@local`) scope default for this global.
    pub local: bool,
}

/// Runtime metadata for a list item.
pub(crate) struct ListItemEntry {
    pub name: NameId,
    pub ordinal: i32,
    pub origin: DefinitionId,
}

/// Runtime metadata for a list definition.
pub(crate) struct ListDefEntry {
    pub name: NameId,
    /// All item `DefinitionId`s belonging to this list, sorted by ordinal.
    pub items: Vec<DefinitionId>,
}

/// Runtime metadata for an external function.
pub(crate) struct ExternalFnEntry {
    pub name: NameId,
    pub fallback: Option<DefinitionId>,
}

/// Resolved target of a qualified path string: the defining `DefinitionId`
/// (used for visit counting, exactly as a divert to the same target would
/// use it) plus the linked `(container_idx, byte_offset)` position.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PathTarget {
    pub id: DefinitionId,
    pub container_idx: u32,
    pub byte_offset: usize,
}

/// Build the `container_idx → path` index [`Program::container_paths`]
/// holds: offset-0 targets only, shortest path (then lexicographically
/// smallest) per container — independent of map iteration order.
pub(crate) fn container_paths_from(
    address_by_path: &HashMap<String, PathTarget>,
) -> HashMap<u32, String> {
    let mut rev: HashMap<u32, String> = HashMap::new();
    for (path, target) in address_by_path {
        if target.byte_offset != 0 {
            continue;
        }
        let better = match rev.get(&target.container_idx) {
            None => true,
            Some(existing) => {
                path.len() < existing.len()
                    || (path.len() == existing.len() && path.as_str() < existing.as_str())
            }
        };
        if better {
            rev.insert(target.container_idx, path.clone());
        }
    }
    rev
}

impl Program {
    /// The knot or `knot.stitch` path a container names, if it names one.
    /// Exact: an anonymous container (a choice body, gather, sequence
    /// branch) is `None` — the save format relies on that to write such a
    /// container's visit entry without a path. For "where is this
    /// container" see [`Program::scope_path`].
    #[must_use]
    pub fn container_path(&self, idx: u32) -> Option<&str> {
        self.container_paths.get(&idx).map(String::as_str)
    }

    /// The knot or `knot.stitch` a container sits in: its own path when it
    /// names one, otherwise its lexical scope's — a choice body, gather, or
    /// sequence branch reports the knot/stitch that holds it (after
    /// `choose`, the frame holds only the chosen branch, and the story is
    /// still in that knot). `None` for the root scope and anything directly
    /// under it. The runtime's own vocabulary for "where am I" — see
    /// [`Story::current_path`](crate::Story::current_path).
    #[must_use]
    pub fn scope_path(&self, idx: u32) -> Option<&str> {
        if let Some(path) = self.container_path(idx) {
            return Some(path);
        }
        let scope_id = self.containers.get(usize::try_from(idx).ok()?)?.scope_id;
        let (scope_idx, _) = self.resolve_target(scope_id)?;
        if scope_idx == idx {
            return None;
        }
        self.container_path(scope_idx)
    }

    /// Resolve any target (container or address) to `(container_idx, byte_offset)`.
    pub(crate) fn resolve_target(&self, id: DefinitionId) -> Option<(u32, usize)> {
        self.address_map.get(&id).copied()
    }

    /// Whether `id` names a live container/address in the current program
    /// (a knot/stitch/label or a synthetic child address) — the "does this
    /// still resolve directly" half of the M-3 rehydration miss-path check
    /// (`docs/modules-spec.md` §5).
    pub(crate) fn knows_address(&self, id: DefinitionId) -> bool {
        self.address_map.contains_key(&id)
    }

    /// Whether `id` names a live global slot in the current program.
    pub(crate) fn knows_global(&self, id: DefinitionId) -> bool {
        self.global_map.contains_key(&id)
    }

    /// Whether `id` names a live list item in the current program — the
    /// list-item half of the M-3 rehydration miss-path check (`docs/modules-spec.md`
    /// §5), mirroring [`knows_address`](Self::knows_address)/[`knows_global`](Self::knows_global)
    /// for the ids embedded in a saved `Value::List` (active items).
    pub(crate) fn knows_list_item(&self, id: DefinitionId) -> bool {
        self.list_item_map.contains_key(&id)
    }

    /// Whether `id` names a live list definition in the current program —
    /// the list-origin half of the M-3 rehydration miss-path check, for the
    /// ids embedded in a saved `Value::List`'s `origins`.
    pub(crate) fn knows_list_def(&self, id: DefinitionId) -> bool {
        self.list_def_map.contains_key(&id)
    }

    /// M-3 rehydration miss-path lookup (`docs/modules-spec.md` §5): given
    /// an `id` the current program doesn't recognize, consult the compiled
    /// `#@was` alias table for its current identity. Callers still need to
    /// check whether the returned id itself resolves — an alias chain is
    /// never followed (the compiler always emits `old -> new` against the
    /// definition's *current* id, never `old -> old2`).
    pub(crate) fn resolve_alias(&self, old: DefinitionId) -> Option<DefinitionId> {
        self.alias_table
            .binary_search_by_key(&old, |e| e.old)
            .ok()
            .map(|idx| self.alias_table[idx].new)
    }

    /// Whether this program carries any `#@was`-derived alias-table
    /// entries at all. Gates `load_state`'s miss-path reporting: an
    /// ordinary content edit with no rename directive stays exactly as
    /// silent as it was before M-3.
    pub(crate) fn has_aliases(&self) -> bool {
        !self.alias_table.is_empty()
    }

    /// Resolve a definition ID to `(container_idx, byte_offset)`.
    ///
    /// Promoted from `#[cfg(feature = "testing")]` to real public API by
    /// W2 (#3295): with [`Self::definition_id_for_path`] it is the
    /// name-based half of source→program addressing ("break on
    /// `tavern.order`"), which the wasm bridge composes as
    /// `resolve_path_address`. A pure lookup over the container table —
    /// nothing here touches the VM hot path, so the `step_once` promotion
    /// warning (`docs/debugger-spec.md` §1.4) does not apply.
    #[must_use]
    pub fn resolve_address(&self, id: DefinitionId) -> Option<(u32, usize)> {
        self.resolve_target(id)
    }

    /// The program→source resolver (D9, issue #3187; wire encoding: D6,
    /// `docs/debugger-spec.md` §2.2). Resolves a runtime execution position
    /// — [`crate::DebugPosition`], as reported by
    /// [`crate::DebugSnapshot::position`]/[`crate::DebugFrame::position`]
    /// (D4, #3182) — to the source range it was compiled from, via this
    /// program's `DebugInfo` section.
    ///
    /// `None` when:
    /// - no `DebugInfo` section is present (a release-exported or
    ///   `--debug-info`-less compile — §1.2 ship policy: this is the
    ///   expected, non-error case for most builds, not a fault);
    /// - `container_idx` is out of range for the section's container table
    ///   (defensive — should not happen for a position this same `Program`
    ///   produced);
    /// - `offset` is before the container's first recorded entry (the
    ///   section's coverage guarantee, §2.2, means this should not happen
    ///   for a real instruction boundary either, but a reader must not
    ///   panic on an adversarial/malformed position).
    ///
    /// The returned range's `file` is `None` for the reserved synthetic
    /// sentinel file (index 0, §2.5) — a compiler-synthesized construct
    /// with no author source to point at — and `Some(path)` (project-root-
    /// relative) otherwise. This is exactly the `path`/`span` pair the
    /// studio's `source` Location space needs (`docs/studio-shell-spec.md`
    /// §6.1) — a caller's `program` resolver wraps this method and returns
    /// `{ kind: "source", file, span: { start: range_start, end:
    /// range_start + range_len } }`.
    ///
    /// Entries within a container are sorted ascending by
    /// `bytecode_offset` and cover the container's full address range with
    /// no gaps (§2.2), so a floor lookup — the last entry whose
    /// `bytecode_offset` is `<= offset` — always names the instruction's
    /// own statement, matching how a running VM's `offset` (the *next*
    /// instruction to execute, always itself a decoded instruction
    /// boundary) lines up against entries recorded at instruction
    /// boundaries during codegen's own walk.
    #[must_use]
    pub fn resolve_debug_position(
        &self,
        position: crate::debug::DebugPosition,
    ) -> Option<crate::debug::DebugSourceLocation> {
        let entry = self.debug_entry_at(position)?;
        let debug_info = self.debug_info.as_ref()?;
        let file = debug_info.files.get(entry.file_idx as usize)?;
        let path = match file.surface {
            brink_format::FileSurface::Synthetic => None,
            brink_format::FileSurface::Ink | brink_format::FileSurface::Native => {
                Some(file.path.clone())
            }
        };
        Some(crate::debug::DebugSourceLocation {
            file: path,
            range_start: entry.range_start,
            range_len: entry.range_len,
        })
    }

    /// The `DebugInfo` entry covering `position` — the floor lookup both
    /// [`Self::resolve_debug_position`] and [`Self::debug_line_key`] share.
    fn debug_entry_at(
        &self,
        position: crate::debug::DebugPosition,
    ) -> Option<&brink_format::DebugEntry> {
        let debug_info = self.debug_info.as_ref()?;
        let table = debug_info.containers.get(position.container_idx as usize)?;
        let target = u32::try_from(position.offset).ok()?;
        let idx = match table
            .entries
            .binary_search_by_key(&target, |e| e.bytecode_offset)
        {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        table.entries.get(idx)
    }

    /// A cheap identity for "which source line is this position on":
    /// `(file_idx, line_idx)`, both section-local and 0-based (#3264).
    ///
    /// Deliberately not `(String, u32)`: line stepping compares this once
    /// per VM instruction, and cloning a path per instruction to answer
    /// "same line?" would make the verb's cost scale with path length for
    /// no benefit. The indices are only ever compared to each other, never
    /// shown, so they never need resolving to text.
    ///
    /// `None` when the artifact carries no `DebugInfo`, the position has no
    /// covering entry, or that file carries no line index (compiled without
    /// source text — see `DebugFileEntry::line_starts`).
    #[cfg(feature = "debug-hooks")]
    pub(crate) fn debug_line_key(
        &self,
        position: crate::debug::DebugPosition,
    ) -> Option<(u32, u32)> {
        let entry = self.debug_entry_at(position)?;
        let file = self
            .debug_info
            .as_ref()?
            .files
            .get(entry.file_idx as usize)?;
        let line = Self::line_index_in(file, entry.range_start)?;
        Some((entry.file_idx, line))
    }
}

/// [`Program::resolve_debug_line`]'s answer: where a bytecode position
/// sits in author-facing source, at both granularities the debugger
/// serves — the line (the author tier's band/chip) and the covering
/// entry's byte range (the finer tiers: expression rows, instruction
/// stepping, step-out's mid-line call-site landing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedDebugLine<'a> {
    pub file: &'a str,
    /// 0-based.
    pub line: u32,
    /// Byte offset in `file`, as the compiler consumed it.
    pub range_start: u32,
    pub range_len: u32,
}

impl Program {
    /// The `file:line` — plus the covering entry's exact byte range — of a
    /// bytecode position (W6/#3299). The line places the execution
    /// highlight's band and the paused chip; the RANGE rides along so
    /// finer-than-line consumers need no new seam: expression-level
    /// entries (D1's unflagged rows, once #3183's `Expr` provenance
    /// lands), instruction stepping in the editor, and the one
    /// mid-line case that exists TODAY — a step-out lands at the call
    /// site, not a line start (`docs/debugger-spec.md` §4's `finish`
    /// semantics).
    ///
    /// 0-based line (UIs showing 1-based convert at their edge). `None`
    /// when the position doesn't resolve, resolves to the synthetic
    /// sentinel, or that file carries no line index.
    #[must_use]
    pub fn resolve_debug_line(
        &self,
        position: crate::debug::DebugPosition,
    ) -> Option<ResolvedDebugLine<'_>> {
        let entry = self.debug_entry_at(position)?;
        let file = self
            .debug_info
            .as_ref()?
            .files
            .get(entry.file_idx as usize)?;
        if !matches!(
            file.surface,
            brink_format::FileSurface::Ink | brink_format::FileSurface::Native
        ) {
            return None;
        }
        let line = Self::line_index_in(file, entry.range_start)?;
        Some(ResolvedDebugLine {
            file: file.path.as_str(),
            line,
            range_start: entry.range_start,
            range_len: entry.range_len,
        })
    }

    /// 0-based line containing `byte` within `file`'s line index, or `None`
    /// when that file carries no index or the offset precedes its first
    /// line start (which a well-formed index makes impossible, since it
    /// always begins at 0).
    fn line_index_in(file: &brink_format::DebugFileEntry, byte: u32) -> Option<u32> {
        if file.line_starts.is_empty() {
            return None;
        }
        // `partition_point` gives the count of starts at or before `byte`;
        // the line is one less. Never underflows for a well-formed index,
        // whose first start is 0 — but a malformed artifact must degrade to
        // `None` rather than wrap.
        let count = file.line_starts.partition_point(|&s| s <= byte);
        u32::try_from(count.checked_sub(1)?).ok()
    }

    /// 0-based line containing `byte` in `file` (#3264) — the public form
    /// of the lookup [`Self::debug_line_key`] uses internally. `None` when
    /// the file is unknown or carries no line index.
    #[must_use]
    pub fn line_at(&self, file: &str, byte: u32) -> Option<u32> {
        let debug_info = self.debug_info.as_ref()?;
        let entry = debug_info.files.iter().find(|f| {
            matches!(
                f.surface,
                brink_format::FileSurface::Ink | brink_format::FileSurface::Native
            ) && f.path == file
        })?;
        Self::line_index_in(entry, byte)
    }

    /// The inverse of [`Self::resolve_debug_position`] (D9/#3187): the
    /// program address to break on for a span of **source** text — issue
    /// #3246, the half a breakpoint gutter needs. `BreakpointSet` is keyed
    /// by `(container_idx, offset)`; an editor speaks in source. This maps
    /// the latter to the former.
    ///
    /// # Why a byte range and not a line number
    ///
    /// The `DebugInfo` section records **byte ranges**, and a `Program`
    /// holds neither source text nor a line table — so it physically
    /// cannot turn "line 7" into bytes. That conversion belongs where the
    /// source already lives (the editor, the CLI's own file read), which
    /// also keeps the UTF-8/UTF-16 question out of the runtime entirely.
    /// The caller passes the half-open byte range `[start, end)` it
    /// considers "the line" (or a selection, or any span), and this answers
    /// where to break within it.
    ///
    /// # Which candidate wins
    ///
    /// Every entry in every container whose file is `file` and whose
    /// `range_start` lies in `[start, end)` is a candidate. The winner is
    /// the minimum by `(range_start, container_idx, bytecode_offset)`:
    ///
    /// - **`range_start` first** — the textually earliest construct in the
    ///   span, which is what "break on this line" means to a person. Note
    ///   this is deliberately *not* "lowest `container_idx`": containers
    ///   are independent bytecode streams with no execution order between
    ///   them, so ordering by container index would be arbitrary dressed
    ///   up as a rule.
    /// - **then `container_idx`, then `bytecode_offset`** — pure
    ///   tie-breaking, so a given span always yields the same address
    ///   rather than whichever entry iteration happened to reach first
    ///   (`CLAUDE.md`: determinism matters).
    ///
    /// # `None` is a real answer, not a failure
    ///
    /// Returns `None` when the span contains no executable code at all — a
    /// comment, a blank line, a line whose code folded away — and when the
    /// artifact carries no `DebugInfo` or names no such file. Callers
    /// **must** surface that: a gutter has to refuse to arm visibly,
    /// because a breakpoint that silently never hits is worse than no
    /// breakpoint.
    ///
    /// Entries whose file is the reserved synthetic sentinel (§2.5) never
    /// match, since no author-facing path names it.
    #[must_use]
    pub fn resolve_source_range(
        &self,
        file: &str,
        start: u32,
        end: u32,
    ) -> Option<crate::debug::DebugPosition> {
        let debug_info = self.debug_info.as_ref()?;

        // Path -> file table index. Synthetic entries carry no
        // author-facing path and must never match one.
        let file_idx = u32::try_from(debug_info.files.iter().position(|f| {
            matches!(
                f.surface,
                brink_format::FileSurface::Ink | brink_format::FileSurface::Native
            ) && f.path == file
        })?)
        .ok()?;

        let mut best: Option<(u32, u32, u32)> = None;
        for (container_idx, table) in debug_info.containers.iter().enumerate() {
            let Ok(container_idx) = u32::try_from(container_idx) else {
                continue;
            };
            for entry in &table.entries {
                if entry.file_idx != file_idx
                    || entry.range_start < start
                    || entry.range_start >= end
                {
                    continue;
                }
                let candidate = (entry.range_start, container_idx, entry.bytecode_offset);
                if best.is_none_or(|current| candidate < current) {
                    best = Some(candidate);
                }
            }
        }

        let (_, container_idx, bytecode_offset) = best?;
        Some(crate::debug::DebugPosition {
            container_idx,
            offset: bytecode_offset as usize,
        })
    }

    /// Whether this program carries a `DebugInfo` section at all (#3248).
    ///
    /// Every other debug accessor returns `None` for two very different
    /// reasons — "this artifact was compiled without `--debug-info`" and
    /// "that particular position/line has nothing on it" — and a debugger
    /// front-end must tell a user which. Reporting "that line has no
    /// executable code" for a story compiled without the flag sends the
    /// author hunting a bug in their source when the fix is a compiler
    /// flag. This is the cheap discriminator that keeps that message
    /// honest; it says nothing about whether any *particular* lookup will
    /// succeed.
    #[must_use]
    pub fn has_debug_info(&self) -> bool {
        self.debug_info.is_some()
    }

    /// The program address to break on for a **line** of source, with no
    /// source text required (#3261) — the `DebugInfo` file table carries a
    /// per-file line index, so the engine can answer `file:line` directly.
    ///
    /// `line` is **0-based**. Every UI that shows 1-based line numbers
    /// converts at its own edge; keeping the engine 0-based means the
    /// fencepost lives in exactly one place per consumer instead of being
    /// re-decided here.
    ///
    /// This is the shape a remote debugger frontend needs — DAP's
    /// `setBreakpoints` is file + line, and an adapter may hold no source
    /// at all. It is a thin wrapper over
    /// [`Self::resolve_source_range`]: the line index turns the line into
    /// its half-open byte span, and the same "textually earliest construct
    /// wins" rule picks the address.
    ///
    /// `None` when the file is unknown, carries no line index (compiled
    /// before this data existed, or with no source text supplied), the line
    /// is past the end of the file, or the line holds no executable code —
    /// a comment, a blank, a line whose code folded away. Callers must
    /// surface that: a gutter has to refuse to arm visibly, because a
    /// breakpoint that silently never hits is worse than no breakpoint.
    #[must_use]
    pub fn resolve_source_line(
        &self,
        file: &str,
        line: u32,
    ) -> Option<crate::debug::DebugPosition> {
        let (start, end) = self.line_span(file, line)?;
        self.resolve_source_range(file, start, end)
    }

    /// The half-open byte span `[start, end)` of a 0-based `line` in `file`,
    /// from the `DebugInfo` file table's line index (#3261). `None` when the
    /// file is unknown, has no line index, or the line is past its end.
    ///
    /// The last line runs to the end of the file, which the index does not
    /// record — so it is represented as `u32::MAX`, an end bound no real
    /// `range_start` can reach. That is deliberate rather than clamping to
    /// a length the section does not carry.
    #[must_use]
    pub fn line_span(&self, file: &str, line: u32) -> Option<(u32, u32)> {
        let debug_info = self.debug_info.as_ref()?;
        let entry = debug_info.files.iter().find(|f| {
            matches!(
                f.surface,
                brink_format::FileSurface::Ink | brink_format::FileSurface::Native
            ) && f.path == file
        })?;
        let idx = line as usize;
        let start = *entry.line_starts.get(idx)?;
        let end = entry.line_starts.get(idx + 1).copied().unwrap_or(u32::MAX);
        Some((start, end))
    }

    /// Whether `text` is byte-identical to the source `file` was compiled
    /// from, by the `DebugInfo` file table's `source_hash` (#3261).
    ///
    /// The problem this exists for: both debug resolvers happily answer
    /// questions about source they were never built from. Author types, the
    /// recompile is still debounced, the gutter asks about the *current*
    /// buffer against the *previous* program — and gets a confidently wrong
    /// address rather than an error. That applies to byte ranges every bit
    /// as much as to line numbers; offsets shift on every inserted
    /// character.
    ///
    /// Per-file on purpose: one dirty file degrades debugging in that file
    /// alone, where a whole-program checksum degrades everything.
    ///
    /// `None` — "cannot tell" — when the artifact carries no `DebugInfo`,
    /// names no such file, or recorded no hash (compiled without source
    /// text). Deliberately tri-state rather than defaulting to `false`:
    /// "unknown" and "stale" call for different handling, and collapsing
    /// them would make every hash-less artifact look permanently stale.
    ///
    /// A change **detector**, not a proof — see [`brink_format::content_hash`].
    #[must_use]
    pub fn source_matches(&self, file: &str, text: &str) -> Option<bool> {
        let debug_info = self.debug_info.as_ref()?;
        let entry = debug_info.files.iter().find(|f| {
            matches!(
                f.surface,
                brink_format::FileSurface::Ink | brink_format::FileSurface::Native
            ) && f.path == file
        })?;
        if entry.source_hash == 0 {
            return None;
        }
        Some(entry.source_hash == brink_format::content_hash(text))
    }

    /// Get a container by its index.
    pub(crate) fn container(&self, idx: u32) -> &LinkedContainer {
        &self.containers[idx as usize]
    }

    /// The code the VM executes for container `idx`: the linker's rewritten
    /// copy when it produced one (`LinkTables::code`), else the symbolic
    /// bytecode — same bytes at every offset except static-target operands.
    #[inline]
    pub(crate) fn code(&self, idx: u32) -> &[u8] {
        self.link.code.get(idx as usize).map_or_else(
            || self.containers[idx as usize].bytecode.as_slice(),
            Vec::as_slice,
        )
    }

    /// Static target `ordinal` of the linked table.
    #[inline]
    pub(crate) fn target(&self, ordinal: u32) -> Option<&LinkedTarget> {
        self.link.targets.get(ordinal as usize)
    }

    /// Resolve a symbolic address id to a [`LinkedTarget`] — the slow path
    /// the VM takes for operands the linker left symbolic and for targets
    /// that arrive as values (`Value::DivertTarget`).
    pub(crate) fn resolve(&self, id: DefinitionId) -> Result<LinkedTarget, RuntimeError> {
        let (container_idx, offset) = self
            .resolve_target(id)
            .ok_or(RuntimeError::UnresolvedDefinition(id))?;
        Ok(LinkedTarget {
            container_idx,
            offset,
            id,
        })
    }

    /// Get a container's bytecode by index.
    #[cfg(feature = "testing")]
    pub fn container_bytecode(&self, idx: u32) -> &[u8] {
        &self.containers[idx as usize].bytecode
    }

    /// Number of containers. Promoted from `#[cfg(feature = "testing")]`
    /// to real public API for the structural-transcript re-render road
    /// (RULED 2026-08-30): a transcript saved against an older compile can
    /// carry container indices this program no longer has, and the caller
    /// must be able to bounds-filter them before `scope_table_idx` would
    /// panic.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "container count fits in u32"
    )]
    pub fn container_count(&self) -> u32 {
        self.containers.len() as u32
    }

    /// CRC-32 checksum from the source `.inkb`, used for transcript validation.
    pub fn source_checksum(&self) -> u32 {
        self.source_checksum
    }

    /// Get the scope line table index for a container.
    pub(crate) fn scope_table_idx(&self, container_idx: u32) -> u32 {
        self.containers[container_idx as usize].scope_table_idx
    }

    /// Look up a name by id.
    pub(crate) fn name(&self, id: NameId) -> &str {
        &self.name_table[id.0 as usize]
    }

    /// Look up a name by id, returning `None` if the id is out of range. Used
    /// by function-value rehydration (T1c, #700): a closure loaded from a save
    /// produced against a *different* compile can carry a `NameId` that no
    /// longer indexes this program's table — treated as a mismatch (fault),
    /// never a panic.
    ///
    /// Public (T1d, `docs/t1d-spec.md` §6): the same "index by id, `None` if
    /// out of range" contract a host needs to resolve a [`brink_format::Value::Handle`]'s
    /// `kind` to its manifest-declared name — e.g. for dev-tooling display or
    /// a host-side capability check. `bevy-brink` re-exports `Program`
    /// (decision 2026-07-10), so this is reachable from engine code without a
    /// direct `brink-runtime` dependency.
    pub fn name_checked(&self, id: NameId) -> Option<&str> {
        self.name_table.get(id.0 as usize).map(String::as_str)
    }

    /// Reverse of [`name_checked`](Self::name_checked): look up the
    /// [`NameId`] a string interns to in this program's name table, if any.
    ///
    /// Public (T1d-3, `docs/t1d-spec.md` §4): a host minting a
    /// [`brink_format::Value::Handle`] from a binding (e.g. `spawn_timer()`
    /// returning a fresh `Handle<Timer>`) needs the compiled program's
    /// `NameId` for the manifest-declared kind name (`"Timer"`) to build the
    /// token — the wire form carries only the interned id, never the string.
    /// `None` means this compile never interned that name (e.g. no
    /// `Handle<Timer>`-typed signature or annotation anywhere in the source
    /// graph), so no token of that kind can be minted against this program.
    /// Linear scan, same cost class as [`global_index`](Self::global_index).
    #[must_use]
    pub fn name_id(&self, name: &str) -> Option<NameId> {
        self.name_table
            .iter()
            .position(|n| n == name)
            .and_then(|i| u16::try_from(i).ok())
            .map(NameId)
    }

    /// Access a container's per-parameter name/mode metadata (T1c, #700).
    pub(crate) fn container_params(&self, idx: u32) -> &[brink_format::ParamMeta] {
        &self.containers[idx as usize].params
    }

    /// Look up a global slot index.
    pub(crate) fn resolve_global(&self, id: DefinitionId) -> Option<u32> {
        self.global_map.get(&id).copied()
    }

    /// Get the root container index.
    pub(crate) fn root_idx(&self) -> u32 {
        self.root_idx
    }

    /// Resolve a qualified ink path to its `(container_idx, byte_offset)`.
    ///
    /// Supports knot names (`intro`), qualified stitches (`knot.stitch`), and,
    /// for programs compiled by `brink-compiler`, author labels
    /// (`knot.label`, `knot.stitch.label`). Programs without the compiler's
    /// `address_paths` table (legacy `.inkb` or converter output) resolve
    /// knot/stitch scope paths only. Use this to spawn flows at named entry
    /// points:
    ///
    /// ```no_run
    /// # fn example(program: &brink_runtime::Program) {
    /// use brink_runtime::FlowInstance;
    ///
    /// if let Some((idx, _)) = program.find_address("intro_scene") {
    ///     let (flow, ctx) = FlowInstance::new_at(program, idx);
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn find_address(&self, path: &str) -> Option<(u32, usize)> {
        self.address_by_path
            .get(path)
            .map(|t| (t.container_idx, t.byte_offset))
    }

    /// Resolve a qualified ink path to the `DefinitionId` of its target.
    /// Same path grammar as [`find_address`](Self::find_address). Used by
    /// `choose_path_string`, which needs the id so the jump goes through the
    /// same divert machinery (and visit counting) as `-> path` would.
    pub(crate) fn find_path_target(&self, path: &str) -> Option<DefinitionId> {
        self.address_by_path.get(path).map(|t| t.id)
    }

    /// Public wrapper on [`find_path_target`](Self::find_path_target): resolve
    /// a qualified ink path (same grammar as [`find_address`](Self::find_address))
    /// to the `DefinitionId` of its target. Used by hosts that need the id
    /// itself — e.g. `bevy-brink`'s wake-condition purity check (issue #995),
    /// which looks the id up in the story's `EffectRows` table to inspect a
    /// `FlowSleep` condition's effect row before admitting it into the wake
    /// contract.
    #[must_use]
    pub fn definition_id_for_path(&self, path: &str) -> Option<DefinitionId> {
        self.find_path_target(path)
    }

    /// Declared parameter count of the container a `path` targets, for
    /// arity-checking a host-directed parameterized entry. `None` if the path
    /// is unknown. (Always `0` for converter-built programs, which don't
    /// record param counts.)
    pub(crate) fn path_param_count(&self, path: &str) -> Option<u8> {
        self.address_by_path
            .get(path)
            .map(|t| self.containers[t.container_idx as usize].param_count)
    }

    // ── Visibility (`#@private` — M-2b, docs/modules-spec.md §4) ────────────

    /// Whether the compiler marked any definition `#@private`. `false` for the
    /// entire pre-modules / all-public world — the fast path where visibility
    /// enforcement is a single boolean check that skips every lookup below.
    pub(crate) fn has_private_defs(&self) -> bool {
        !self.private_defs.is_empty()
    }

    /// Whether the definition `id` was declared `#@private`.
    pub(crate) fn is_private(&self, id: DefinitionId) -> bool {
        self.private_defs
            .binary_search_by_key(&id.to_raw(), |d| d.to_raw())
            .is_ok()
    }

    /// Whether the global at slot `idx` is `#@private`.
    pub(crate) fn global_is_private(&self, idx: u32) -> bool {
        self.globals
            .get(idx as usize)
            .is_some_and(|slot| self.is_private(slot.id))
    }

    /// Whether the named entry point (knot/stitch/function path) is
    /// `#@private`. Unknown paths are treated as not-private — resolution
    /// failure is reported by the caller's own "not found" path, not here.
    pub(crate) fn path_is_private(&self, path: &str) -> bool {
        self.find_path_target(path)
            .is_some_and(|id| self.is_private(id))
    }

    /// Whether the container at `idx` is `#@private`. Used by
    /// [`FlowInstance::begin_function_eval`](crate::FlowInstance::begin_function_eval)/
    /// [`begin_function_value_eval`](crate::FlowInstance::begin_function_value_eval),
    /// which receive an already-resolved `container_idx` rather than a name
    /// (the caller resolves it, typically via [`find_address`](Self::find_address),
    /// before entering the VM boundary). Out-of-range indices are not
    /// private — an invalid index is the caller's bug, reported elsewhere.
    pub(crate) fn container_is_private(&self, idx: u32) -> bool {
        self.containers
            .get(idx as usize)
            .is_some_and(|c| self.is_private(c.id))
    }

    /// Build the initial globals vector from slot defaults.
    pub fn global_defaults(&self) -> Vec<Value> {
        self.globals.iter().map(|s| s.default.clone()).collect()
    }

    /// Find the global variable slot index for a variable name, if declared.
    /// Used by host-facing variable get/set (`Story::variable`/`set_variable`).
    #[expect(clippy::cast_possible_truncation, reason = "global count fits in u32")]
    pub fn global_index(&self, name: &str) -> Option<u32> {
        self.globals
            .iter()
            .position(|slot| self.name(slot.name) == name)
            .map(|i| i as u32)
    }

    /// Get a list literal by index.
    pub(crate) fn list_literal(&self, idx: u16) -> &ListValue {
        &self.list_literals[idx as usize]
    }

    /// Get a T1b literal pool entry by index. `None` on an out-of-range
    /// index (malformed bytecode) rather than panicking — the VM turns
    /// this into a `RuntimeError`, never a crash.
    pub(crate) fn literal_pool_entry(&self, idx: u32) -> Option<&Value> {
        self.literal_pool.get(idx as usize)
    }

    /// Look up a `STRUCT` shape's runtime metadata by `ShapeId`. `None` on an
    /// out-of-range id (malformed bytecode) rather than panicking — mirrors
    /// [`literal_pool_entry`](Self::literal_pool_entry).
    pub(crate) fn struct_shape(&self, shape: ShapeId) -> Option<&StructShapeEntry> {
        self.struct_shapes.get(shape.0 as usize)
    }

    /// Look up a list item's metadata.
    pub(crate) fn list_item(&self, id: DefinitionId) -> Option<&ListItemEntry> {
        self.list_item_map.get(&id)
    }

    /// Get a list definition by its `DefinitionId`.
    pub(crate) fn list_def(&self, id: DefinitionId) -> Option<&ListDefEntry> {
        self.list_def_map.get(&id).map(|&idx| &self.list_defs[idx])
    }

    /// Find a list definition by its string name.
    pub(crate) fn list_def_by_name(&self, name: &str) -> Option<&ListDefEntry> {
        self.list_defs
            .iter()
            .find(|def| self.name(def.name) == name)
    }

    /// Look up an external function by its `DefinitionId`.
    pub(crate) fn external_fn(&self, id: DefinitionId) -> Option<&ExternalFnEntry> {
        self.external_fns.get(&id)
    }

    // ── Public variable introspection (host-facing) ─────────────────────────
    // `global_index` (above), `global_name`, and `global_count` form the
    // host-facing variable-introspection set used by `Story::variable`/
    // `set_variable` and consumers like the RMMZ var↔switch mapping. They were
    // previously `testing`-gated; promoted to public per the State View plan.

    /// Resolve a global cell's `DefinitionId` to its slot index — the
    /// numbering [`ContextAccess::set_global`](crate::ContextAccess) and
    /// [`Self::global_index`] use.
    ///
    /// Public because effect rows (`brink_format::DirectEffects::reads` /
    /// `writes`) name global cells by `DefinitionId` while the runtime's
    /// world writes are keyed by slot: a host consuming rows for scheduling
    /// (bevy-brink's row-directed wake dirtying, issue #1146) needs exactly
    /// this bridge. `None` for an id this program declares no global for
    /// (a stale row, a `VAR` removed by a story patch).
    pub fn global_slot(&self, id: DefinitionId) -> Option<u32> {
        self.resolve_global(id)
    }

    /// Resolve a global slot index to its variable name.
    pub fn global_name(&self, idx: u32) -> Option<&str> {
        self.globals
            .get(idx as usize)
            .map(|slot| self.name(slot.name))
    }

    // ── Compiled scope defaults (`#@local` — directive-annotations spec) ────

    /// Whether the compiler marked anything flow-private. When `false`
    /// (all existing unannotated ink), policy resolution keeps its
    /// all-`World` fast path.
    ///
    /// Public so the bevy host (`bevy-brink`'s batch driver) can guard
    /// against batching a `#@local`-annotated story: batch mode routes only
    /// the shared `World`, never a flow's private `FlowLocal`, so a story
    /// carrying compiled flow-private defaults must stay on the serial API
    /// (`docs/effects-spec.md` §12; bevy-brink #925).
    pub fn has_local_defaults(&self) -> bool {
        !self.local_scope_defaults.is_empty() || self.globals.iter().any(|g| g.local)
    }

    /// Compiled flow-private default for a global slot.
    pub(crate) fn global_is_local(&self, idx: u32) -> bool {
        self.globals.get(idx as usize).is_some_and(|g| g.local)
    }

    /// Compiled flow-private knot/stitch defaults, sorted by path.
    pub(crate) fn local_scope_defaults(&self) -> &[(String, DefinitionId)] {
        &self.local_scope_defaults
    }

    /// Number of global variable slots.
    #[expect(clippy::cast_possible_truncation, reason = "global count fits in u32")]
    pub fn global_count(&self) -> u32 {
        self.globals.len() as u32
    }

    // ── Debug introspection name lookups (used by `debug_snapshot`) ──────────

    /// Variable name for a global slot index.
    pub(crate) fn global_slot_name(&self, idx: usize) -> Option<&str> {
        self.globals.get(idx).map(|slot| self.name(slot.name))
    }

    /// D7 (`docs/debugger-spec.md` §3, #3185): every `LocalsTable` row
    /// declared anywhere in the same lexical **scope** (`ContainerDef::
    /// scope_id`) as the container at `leaf_container_idx` — not just that
    /// one container's own table.
    ///
    /// This is deliberately scope-wide, not container-local: a call frame's
    /// `container_stack` can legitimately shrink back to a single entry
    /// mid-frame (`vm::goto_target`'s "target not already on the stack"
    /// branch `clear()`s and replaces it wholesale — e.g. entering a
    /// `{? … }` choice-target body from its enclosing knot is exactly this
    /// case), which would silently drop an enclosing container's locals
    /// (its own parameters/`~ temp`s) from view the moment the leaf
    /// position moves into a *sibling* child container — even though the
    /// call frame's `temps` are completely unaffected (they are declared
    /// once per **scope root**, `docs/debugger-spec.md` §3: "VM temp slots
    /// ... are allocated per active call frame, not lexically nested").
    /// Grouping by `scope_id` instead of by whatever happens to be on
    /// `container_stack` right now is what keeps a parameter/`~ temp`
    /// visible for the frame's entire lifetime, matching the runtime's own
    /// slot-allocation model rather than the transient shape of one
    /// in-frame navigation stack.
    ///
    /// Empty when this artifact carries no `DebugInfo` at all (a
    /// release-exported story, or one compiled before D6), when
    /// `leaf_container_idx` is out of range (malformed/adversarial
    /// `.inkb`, not a panic case), or when the scope genuinely declares no
    /// locals.
    pub(crate) fn scope_debug_locals(
        &self,
        leaf_container_idx: u32,
    ) -> Vec<&brink_format::DebugLocalEntry> {
        let Some(debug_info) = self.debug_info.as_ref() else {
            return Vec::new();
        };
        let Some(scope_id) = self
            .containers
            .get(leaf_container_idx as usize)
            .map(|c| c.scope_id)
        else {
            return Vec::new();
        };
        self.containers
            .iter()
            .zip(debug_info.containers.iter())
            .filter(|(c, _)| c.scope_id == scope_id)
            .flat_map(|(_, table)| table.locals.iter())
            .collect()
    }

    /// Compiled `DefinitionId` for a global slot index — the identity
    /// `save_state` round-trips into `SaveState::global_ids` so the M-3
    /// rehydration miss path (`docs/modules-spec.md` §5) can recover a
    /// renamed VAR/CONST/LIST global's *save-time* id (declared-module
    /// identity is `(module, name)`-hashed, so the bare name alone can't
    /// reconstruct it) and look it up in the compiled alias table.
    pub(crate) fn global_id(&self, idx: usize) -> Option<DefinitionId> {
        self.globals.get(idx).map(|slot| slot.id)
    }

    /// Variable name for a global's defining `DefinitionId` (e.g. a
    /// `VariablePointer` target, or a T1e projection's root cell). `pub`
    /// (not `pub(crate)`) since `brink-web`'s program-model/speculation
    /// disassembly needs it to render a projection's root name at the wasm
    /// boundary, the same way `divert_target_path` already resolves a
    /// divert's `DefinitionId` for that consumer.
    pub fn global_var_name(&self, id: DefinitionId) -> Option<&str> {
        let slot = self.resolve_global(id)?;
        self.global_slot_name(slot as usize)
    }

    /// Display name for a list item by its `DefinitionId`.
    pub(crate) fn list_item_name(&self, id: DefinitionId) -> Option<&str> {
        self.list_item(id).map(|item| self.name(item.name))
    }

    // ── Host-facing structured value display (F4.3 web binding) ─────────────
    // `list_members`/`divert_target_path` give a host (e.g. brink-web's wasm
    // marshaling) the same name resolution `value_ops::stringify_list` and
    // `debug::NameResolver` already do internally, but structured rather than
    // pre-joined into a display string — a host may want to render a list's
    // members or a divert's destination as distinct fields rather than text.
    // On-demand only (not on any hot path), like `debug::NameResolver`.

    /// Resolve the active members of a list value for host-facing display:
    /// each member's origin list name, unqualified item name, and ordinal.
    /// Sorted the same way in-story list stringification orders them
    /// (ordinal, then origin name) so the two presentations agree.
    #[must_use]
    pub fn list_members(&self, list: &ListValue) -> Vec<ListMember> {
        let mut entries: Vec<ListMember> = list
            .items
            .iter()
            .filter_map(|&id| {
                self.list_item(id).map(|entry| {
                    let origin = self
                        .list_def(entry.origin)
                        .map_or_else(String::new, |def| self.name(def.name).to_owned());
                    let full_name = self.name(entry.name);
                    let name = full_name
                        .split_once('.')
                        .map_or_else(|| full_name.to_owned(), |(_, item)| item.to_owned());
                    ListMember {
                        origin,
                        name,
                        ordinal: entry.ordinal,
                    }
                })
            })
            .collect();
        entries.sort_by(|a, b| {
            a.ordinal
                .cmp(&b.ordinal)
                .then_with(|| a.origin.cmp(&b.origin))
        });
        entries
    }

    /// The qualified knot/stitch path a `DefinitionId` names, if it resolves
    /// to a named scope entry (offset-0 in `address_by_path`) — the
    /// destination of a `Value::DivertTarget` for host-facing display.
    /// Deterministic on collision: shortest path, then lexicographically
    /// smallest, independent of the map's iteration order (mirrors
    /// `debug::NameResolver`'s reverse lookup).
    #[must_use]
    pub fn divert_target_path(&self, id: DefinitionId) -> Option<String> {
        let (container_idx, _) = self.resolve_target(id)?;
        let mut best: Option<&str> = None;
        for (path, target) in &self.address_by_path {
            if target.byte_offset != 0 || target.container_idx != container_idx {
                continue;
            }
            best = Some(match best {
                None => path.as_str(),
                Some(existing) => {
                    if path.len() < existing.len()
                        || (path.len() == existing.len() && path.as_str() < existing)
                    {
                        path.as_str()
                    } else {
                        existing
                    }
                }
            });
        }
        best.map(ToOwned::to_owned)
    }
}

/// One active member of a list value, resolved for host-facing display. See
/// [`Program::list_members`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListMember {
    /// The origin list's declared name (e.g. `"Weekday"`).
    pub origin: String,
    /// The item's unqualified display name (e.g. `"Monday"`).
    pub name: String,
    /// The item's ordinal within its origin list.
    pub ordinal: i32,
}

#[cfg(test)]
mod find_address_tests {
    use super::*;

    fn make_program_with_named_containers(names: &[&str]) -> Program {
        // Build a minimal Program where each name maps to a unique
        // container_idx. Used to exercise find_address without going
        // through the full link path.
        let mut address_by_path = HashMap::new();
        for (i, name) in names.iter().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "test fixture")]
            address_by_path.insert(
                (*name).to_string(),
                PathTarget {
                    id: DefinitionId::new(brink_format::DefinitionTag::Address, i as u64),
                    container_idx: i as u32,
                    byte_offset: 0,
                },
            );
        }
        Program {
            link: crate::program::LinkTables::default(),
            containers: Vec::new(),
            address_map: HashMap::new(),
            scope_ids: Vec::new(),
            source_checksum: 0,
            globals: Vec::new(),
            global_map: HashMap::new(),
            name_table: Vec::new(),
            container_paths: container_paths_from(&address_by_path),
            address_by_path,
            root_idx: 0,
            list_literals: Vec::new(),
            literal_pool: Vec::new(),
            list_item_map: HashMap::new(),
            list_defs: Vec::new(),
            list_def_map: HashMap::new(),
            external_fns: HashMap::new(),
            local_scope_defaults: Vec::new(),
            struct_shapes: Vec::new(),
            private_defs: Vec::new(),
            alias_table: Vec::new(),
            debug_info: None,
        }
    }

    #[test]
    fn finds_known_knot() {
        let program = make_program_with_named_containers(&["intro", "outro"]);
        assert_eq!(program.find_address("intro"), Some((0, 0)));
        assert_eq!(program.find_address("outro"), Some((1, 0)));
    }

    #[test]
    fn returns_none_for_unknown_knot() {
        let program = make_program_with_named_containers(&["intro"]);
        assert_eq!(program.find_address("nope"), None);
    }

    #[test]
    fn empty_program_returns_none() {
        let program = make_program_with_named_containers(&[]);
        assert_eq!(program.find_address("anything"), None);
    }
}
