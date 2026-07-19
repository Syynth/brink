//! Immutable linked program.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use brink_format::{AliasEntry, CountingFlags, DefinitionId, ListValue, NameId, ShapeId, Value};

use crate::collections::Map as HashMap;

/// A linked, ready-to-execute program.
///
/// Created from [`StoryData`](brink_format::StoryData) via [`link()`](crate::link).
/// Immutable after creation — mutable per-instance state lives in [`Story`](crate::Story).
pub struct Program {
    pub(crate) containers: Vec<LinkedContainer>,
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

impl Program {
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
    #[cfg(feature = "testing")]
    pub fn resolve_address(&self, id: DefinitionId) -> Option<(u32, usize)> {
        self.resolve_target(id)
    }

    /// Get a container by its index.
    pub(crate) fn container(&self, idx: u32) -> &LinkedContainer {
        &self.containers[idx as usize]
    }

    /// Get a container's bytecode by index.
    #[cfg(feature = "testing")]
    pub fn container_bytecode(&self, idx: u32) -> &[u8] {
        &self.containers[idx as usize].bytecode
    }

    /// Number of containers.
    #[cfg(feature = "testing")]
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
    /// returning a fresh `handle<Timer>`) needs the compiled program's
    /// `NameId` for the manifest-declared kind name (`"Timer"`) to build the
    /// token — the wire form carries only the interned id, never the string.
    /// `None` means this compile never interned that name (e.g. no
    /// `handle<Timer>`-typed signature or annotation anywhere in the source
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
    /// ```ignore
    /// if let Some((idx, _)) = program.find_address("intro_scene") {
    ///     let (flow, ctx) = FlowInstance::new_at(program, idx);
    /// }
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
            containers: Vec::new(),
            address_map: HashMap::new(),
            scope_ids: Vec::new(),
            source_checksum: 0,
            globals: Vec::new(),
            global_map: HashMap::new(),
            name_table: Vec::new(),
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
