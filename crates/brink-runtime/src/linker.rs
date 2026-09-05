//! Links [`StoryData`] into an executable [`Program`].

use alloc::string::String;
use alloc::vec::Vec;

use brink_format::{DefinitionId, NameId, StoryData};

use crate::collections::{Map as HashMap, map_with_capacity};
use crate::error::RuntimeError;
use crate::program::{
    ExternalFnEntry, GlobalSlot, LinkTables, LinkedContainer, LinkedTarget, ListDefEntry,
    ListItemEntry, PathTarget, Program, StructShapeEntry, linked_operand,
};

/// Look up a `NameId` in `StoryData::name_table`, failing cleanly on an
/// out-of-range index instead of panicking. `NameId`s embedded in
/// malformed/adversarial bytecode are not guaranteed to be in range — this
/// is the linker's own validation, the sanctioned way for such a program to
/// stop (never an unchecked index panic).
fn resolve_name(data: &StoryData, name_id: NameId) -> Result<String, RuntimeError> {
    data.name_table
        .get(name_id.0 as usize)
        .cloned()
        .ok_or_else(|| RuntimeError::InvalidNameId(name_id.0))
}

/// Link a [`StoryData`] into an executable [`Program`].
///
/// Builds lookup tables mapping [`DefinitionId`]s to flat array indices.
/// The root container is `containers[0]` by convention — the brink compiler
/// emits the root first.
#[expect(clippy::cast_possible_truncation, clippy::too_many_lines)]
pub fn link(
    data: &StoryData,
) -> Result<(Program, Vec<Vec<brink_format::LineEntry>>), RuntimeError> {
    let mut container_map = map_with_capacity(data.containers.len());

    for (i, cdef) in data.containers.iter().enumerate() {
        let idx = i as u32;
        container_map.insert(cdef.id, idx);
    }

    // Build scope line tables and a map from scope_id → table index.
    let mut scope_table_map: HashMap<DefinitionId, u32> = map_with_capacity(data.line_tables.len());
    let mut line_tables: Vec<Vec<brink_format::LineEntry>> =
        Vec::with_capacity(data.line_tables.len());
    let mut scope_ids: Vec<DefinitionId> = Vec::with_capacity(data.line_tables.len());
    for lt in &data.line_tables {
        let idx = line_tables.len() as u32;
        scope_table_map.insert(lt.scope_id, idx);
        scope_ids.push(lt.scope_id);
        line_tables.push(lt.lines.clone());
    }

    // Build containers with scope_table_idx.
    let mut containers = Vec::with_capacity(data.containers.len());
    for cdef in &data.containers {
        let scope_table_idx = scope_table_map.get(&cdef.scope_id).copied().unwrap_or(0);
        containers.push(LinkedContainer {
            id: cdef.id,
            bytecode: cdef.bytecode.clone(),
            counting_flags: cdef.counting_flags,
            path_hash: cdef.path_hash,
            param_count: cdef.param_count,
            params: cdef.params.clone(),
            scope_table_idx,
            scope_id: cdef.scope_id,
        });
    }

    // Build globals.
    let mut globals = Vec::with_capacity(data.variables.len());
    let mut global_map = map_with_capacity(data.variables.len());
    for (i, gvar) in data.variables.iter().enumerate() {
        let idx = i as u32;
        global_map.insert(gvar.id, idx);
        globals.push(GlobalSlot {
            id: gvar.id,
            name: gvar.name,
            default: gvar.default_value.clone(),
            local: gvar.local,
        });
    }

    // Build unified address map from containers and address defs.
    // Containers get offset 0 (primary addresses).
    let mut address_map = map_with_capacity(data.containers.len() + data.addresses.len());
    for (i, cdef) in data.containers.iter().enumerate() {
        address_map.insert(cdef.id, (i as u32, 0usize));
    }
    // Address defs add intra-container targets (and primary addresses from converter).
    for addr in &data.addresses {
        let container_idx = container_map
            .get(&addr.container_id)
            .copied()
            .ok_or_else(|| RuntimeError::UnresolvedDefinition(addr.container_id))?;
        address_map.insert(addr.id, (container_idx, addr.byte_offset as usize));
    }

    // Root container is always the first entry by convention.
    if data.containers.is_empty() {
        return Err(RuntimeError::NoRootContainer);
    }
    let link = link_static_operands(&containers, &address_map, &global_map);

    let root_idx = 0;

    let name_table = data.name_table.clone();

    // Build list item map.
    let mut list_item_map = map_with_capacity(data.list_items.len());
    for li in &data.list_items {
        list_item_map.insert(
            li.id,
            ListItemEntry {
                name: li.name,
                ordinal: li.ordinal,
                origin: li.origin,
            },
        );
    }

    // Build list defs and list def map.
    let mut list_defs = Vec::with_capacity(data.list_defs.len());
    let mut list_def_map = map_with_capacity(data.list_defs.len());
    for ldef in &data.list_defs {
        let idx = list_defs.len();
        // Collect all items belonging to this list, sorted by ordinal.
        let mut items: Vec<_> = data
            .list_items
            .iter()
            .filter(|li| li.origin == ldef.id)
            .collect();
        items.sort_by_key(|li| li.ordinal);
        let item_ids: Vec<_> = items.iter().map(|li| li.id).collect();

        list_def_map.insert(ldef.id, idx);
        list_defs.push(ListDefEntry {
            name: ldef.name,
            items: item_ids,
        });
    }

    // Clone list literals.
    let list_literals = data.list_literals.clone();

    // Clone the T1b literal pool (`PushLiteral(idx)` targets).
    let literal_pool = data.literal_pool.clone();

    // Build the TM-4 struct shape table, indexed by `ShapeId` (contiguous
    // small-integer ids assigned at codegen time — a plain `Vec` indexed by
    // `shape.0` mirrors `literal_pool`'s `u32`-indexed layout, no `HashMap`
    // involved).
    let mut struct_shapes: Vec<StructShapeEntry> = Vec::with_capacity(data.struct_shapes.len());
    for shape in &data.struct_shapes {
        let idx = shape.id.0 as usize;
        if struct_shapes.len() <= idx {
            struct_shapes.resize_with(idx + 1, || StructShapeEntry {
                name: NameId(0),
                fields: Vec::new(),
            });
        }
        struct_shapes[idx] = StructShapeEntry {
            name: shape.name,
            fields: shape.fields.clone(),
        };
    }

    // Build external function map.
    let mut external_fns = map_with_capacity(data.externals.len());
    for ext in &data.externals {
        external_fns.insert(
            ext.id,
            ExternalFnEntry {
                name: ext.name,
                fallback: ext.fallback,
            },
        );
    }

    // Build the path → address lookup used by `Program::find_address`.
    //
    // When the program carries an explicit `address_paths` table (compiler
    // output), it is the source of truth: each entry's qualified path maps to
    // its target, resolved through `address_map`. This is what enables
    // qualified addressing of scopes (`knot`, `knot.stitch`) and author labels
    // (`knot.label`, `knot.stitch.label`).
    //
    // When the table is empty (legacy `.inkb` or converter output, which does
    // not emit it), fall back to deriving scope paths from container names —
    // the previous behavior, which already qualifies knot/stitch scope names.
    let mut address_by_path: HashMap<String, PathTarget> = HashMap::new();
    if data.address_paths.is_empty() {
        // `BTreeMap` has no `reserve` — no-op under `no_std`.
        #[cfg(feature = "std")]
        address_by_path.reserve(data.containers.len());
        for (i, cdef) in data.containers.iter().enumerate() {
            if let Some(name_id) = cdef.name {
                let name = resolve_name(data, name_id)?;
                address_by_path.insert(
                    name,
                    PathTarget {
                        id: cdef.id,
                        container_idx: i as u32,
                        byte_offset: 0,
                    },
                );
            }
        }
    } else {
        // `BTreeMap` has no `reserve` — no-op under `no_std`.
        #[cfg(feature = "std")]
        address_by_path.reserve(data.address_paths.len());
        for ap in &data.address_paths {
            // Resolve the target through the address map; skip anything
            // unresolvable (defensive — should not happen for valid output).
            if let Some(&(idx, offset)) = address_map.get(&ap.target) {
                let name = resolve_name(data, ap.path)?;
                address_by_path.insert(
                    name,
                    PathTarget {
                        id: ap.target,
                        container_idx: idx,
                        byte_offset: offset,
                    },
                );
            }
        }
    }

    // Compiled `#@local` knot/stitch defaults — the base layer of policy
    // resolution. Sorted by path so a knot expands before its stitches.
    let mut local_scope_defaults: Vec<(String, DefinitionId)> = Vec::new();
    for cdef in data.containers.iter().filter(|c| c.local) {
        if let Some(n) = cdef.name {
            local_scope_defaults.push((resolve_name(data, n)?, cdef.id));
        }
    }
    local_scope_defaults.sort();

    // M-2b (`docs/modules-spec.md` §4): the `#@private` definition set, used
    // only to refuse host semantic access. Empty for the all-public world.
    // Sorted so `Program::is_private` can binary-search (the compiler already
    // emits it sorted; re-sort defensively for hand-built/legacy `StoryData`).
    let mut private_defs: Vec<DefinitionId> = data.private_defs.clone();
    private_defs.sort_by_key(|d| d.to_raw());

    // M-3 (`docs/modules-spec.md` §5): the compiled alias table, sorted by
    // `old` for `Program::resolve_alias`'s binary search. Sorted again here
    // rather than trusted as-is — malformed/adversarial `.inkb` bytes are
    // not guaranteed to preserve the compiler's ordering invariant.
    let mut alias_table = data.alias_table.clone();
    alias_table.sort_unstable();

    let program = Program {
        containers,
        link,
        address_map,
        scope_ids,
        source_checksum: data.source_checksum,
        globals,
        global_map,
        name_table,
        container_paths: crate::program::container_paths_from(&address_by_path),
        address_by_path,
        root_idx,
        list_literals,
        literal_pool,
        list_item_map,
        list_defs,
        list_def_map,
        external_fns,
        local_scope_defaults,
        struct_shapes,
        private_defs,
        alias_table,
        debug_info: data.debug_info.clone(),
    };
    Ok((program, line_tables))
}

/// Resolve every static operand once and write its resolved form into a
/// linked copy of each container's code — a target's ordinal in
/// `LinkTables::targets`, a global's slot index — see `LinkTables` for the
/// layout and the rulings it serves.
///
/// Ordinals are assigned in walk order (container by container, instruction
/// by instruction), so two links of the same data produce the same table.
/// A target `address_map` cannot resolve stays symbolic, and a container
/// whose bytecode stops decoding is left symbolic from that point on: in
/// both cases the VM meets exactly the error it would have met before.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a target ordinal indexes a Vec built here; it cannot exceed u32"
)]
fn link_static_operands(
    containers: &[LinkedContainer],
    address_map: &HashMap<DefinitionId, (u32, usize)>,
    global_map: &HashMap<DefinitionId, u32>,
) -> LinkTables {
    use brink_format::{Opcode, StaticKind};

    let mut targets: Vec<LinkedTarget> = Vec::new();
    let mut ordinals: HashMap<DefinitionId, u32> = HashMap::new();
    let mut code = Vec::with_capacity(containers.len());
    for container in containers {
        let symbolic = &container.bytecode;
        let mut linked = symbolic.clone();
        let mut offset = 0;
        while offset < symbolic.len() {
            let site = Opcode::peek_static(symbolic, offset);
            let Ok(op) = Opcode::decode(symbolic, &mut offset) else {
                break;
            };
            let Some(site) = site else {
                continue;
            };
            let resolved = match (site.kind, op) {
                (
                    StaticKind::Target(_),
                    Opcode::Goto(id)
                    | Opcode::GotoIf(id)
                    | Opcode::EnterContainer(id)
                    | Opcode::Call(id)
                    | Opcode::TunnelCall(id)
                    | Opcode::ThreadCall(id)
                    | Opcode::BeginChoice(_, id),
                ) => address_map.get(&id).map(|&(container_idx, target_offset)| {
                    *ordinals.entry(id).or_insert_with(|| {
                        targets.push(LinkedTarget {
                            container_idx,
                            offset: target_offset,
                            id,
                        });
                        (targets.len() - 1) as u32
                    })
                }),
                // A global's linked operand is its slot index — `globals`
                // is already dense, so no table is needed.
                (
                    StaticKind::Global(_),
                    Opcode::GetGlobal(id) | Opcode::SetGlobal(id) | Opcode::TakeGlobal(id),
                ) => global_map.get(&id).copied(),
                _ => None,
            };
            if let Some(operand) = resolved {
                linked[site.operand..site.end].copy_from_slice(&linked_operand(operand));
            }
        }
        code.push(linked);
    }
    LinkTables { code, targets }
}

#[cfg(test)]
mod tests {
    use super::*;

    use brink_format::Opcode;

    use crate::program::linked_ordinal;

    /// Every kind of static target in one story: a divert to a knot, a
    /// gather label inside a weave, a function call, a tunnel, a thread and
    /// a choice.
    const STORY: &str = r"
VAR x = 0
-> top
=== top ===
~ x = f(1)
-> tunnel ->
<- side
* [A] -> gather_here
* [B]
- (gather_here) Gathered.
{ x > 0: -> top | -> END }
=== function f(n) ===
~ return n + 1
=== tunnel ===
In the tunnel.
->->
=== side ===
Side thread.
-> DONE
";

    fn compiled() -> StoryData {
        brink_compiler::compile("main.ink", |_p| Ok(STORY.to_owned()))
            .unwrap()
            .data
    }

    /// Walk a container's symbolic bytecode, yielding each static-global
    /// site with the id its operand carries.
    fn global_sites(bytecode: &[u8]) -> Vec<(brink_format::StaticSite, DefinitionId)> {
        let mut out = Vec::new();
        let mut off = 0;
        while off < bytecode.len() {
            let site = Opcode::peek_static(bytecode, off);
            let op = Opcode::decode(bytecode, &mut off).expect("symbolic bytecode decodes");
            let Some(site) = site else { continue };
            if !matches!(site.kind, brink_format::StaticKind::Global(_)) {
                continue;
            }
            let (Opcode::GetGlobal(id) | Opcode::SetGlobal(id) | Opcode::TakeGlobal(id)) = op
            else {
                continue;
            };
            assert_eq!(site.end, off);
            out.push((site, id));
        }
        out
    }

    /// Walk a container's symbolic bytecode, yielding each static-target
    /// site with the id its operand carries.
    fn target_sites(bytecode: &[u8]) -> Vec<(brink_format::TargetSite, DefinitionId)> {
        let mut out = Vec::new();
        let mut off = 0;
        while off < bytecode.len() {
            let site = Opcode::peek_target(bytecode, off);
            let op = Opcode::decode(bytecode, &mut off).expect("symbolic bytecode decodes");
            let Some(site) = site else { continue };
            // `peek_target`'s classification is pinned by brink-format's own
            // test; here only the extent agreement matters.
            let (Opcode::Goto(id)
            | Opcode::GotoIf(id)
            | Opcode::EnterContainer(id)
            | Opcode::Call(id)
            | Opcode::TunnelCall(id)
            | Opcode::ThreadCall(id)
            | Opcode::BeginChoice(_, id)) = op
            else {
                continue;
            };
            assert_eq!(
                site.end, off,
                "peek and decode agree on the instruction's extent"
            );
            out.push((site, id));
        }
        out
    }

    /// Each resolvable static target's operand is rewritten to an ordinal
    /// whose table entry is exactly what `address_map` says for the id the
    /// symbolic bytecode still carries; nothing else in the code changes,
    /// and the symbolic bytecode is untouched.
    #[test]
    fn linked_code_holds_ordinals_for_every_resolvable_static_target() {
        let data = compiled();
        let (program, _) = link(&data).expect("links");
        assert_eq!(program.link.code.len(), program.containers.len());

        let mut sites_seen = 0;
        let mut globals_seen = 0;
        let mut kinds = alloc::collections::BTreeSet::new();
        for (i, container) in program.containers.iter().enumerate() {
            let symbolic = &container.bytecode;
            let linked = &program.link.code[i];
            assert_eq!(
                symbolic, &data.containers[i].bytecode,
                "symbolic copy untouched"
            );
            assert_eq!(symbolic.len(), linked.len(), "same length, same offsets");

            let mut rewritten = alloc::vec![false; symbolic.len()];
            for (site, id) in global_sites(symbolic) {
                globals_seen += 1;
                let slot = program.global_map.get(&id).copied();
                let linked_slot = linked_ordinal(&linked[site.operand..site.end]);
                assert_eq!(slot, linked_slot, "global site {site:?} for {id}");
                if linked_slot.is_some() {
                    rewritten[site.operand..site.end].fill(true);
                }
            }
            for (site, id) in target_sites(symbolic) {
                sites_seen += 1;
                kinds.insert(
                    format!("{:?}", site.kind)
                        .split('(')
                        .next()
                        .unwrap()
                        .to_owned(),
                );
                let expected = program.address_map.get(&id).copied();
                let ordinal = linked_ordinal(&linked[site.operand..site.end]);
                assert_eq!(
                    expected.is_some(),
                    ordinal.is_some(),
                    "site {site:?} for {id}: address_map {expected:?}, linked {ordinal:?}"
                );
                if let (Some((cidx, coff)), Some(ord)) = (expected, ordinal) {
                    let t = program.target(ord).expect("ordinal in table");
                    assert_eq!((t.container_idx, t.offset, t.id), (cidx, coff, id));
                    rewritten[site.operand..site.end].fill(true);
                }
            }
            for (k, (a, b)) in symbolic.iter().zip(linked).enumerate() {
                if !rewritten[k] {
                    assert_eq!(
                        a, b,
                        "byte {k} of container {i} outside any operand changed"
                    );
                }
            }
        }
        assert!(
            sites_seen >= 6,
            "the story exercises several targets: {sites_seen}"
        );
        assert!(
            globals_seen >= 2,
            "the story reads and writes a global: {globals_seen}"
        );
        for kind in ["Goto", "Call", "TunnelCall", "ThreadCall", "BeginChoice"] {
            assert!(kinds.contains(kind), "story exercises {kind}: {kinds:?}");
        }
        // Ordinals are dense and deterministic: linking twice gives the same table.
        let (again, _) = link(&data).expect("links");
        assert_eq!(program.link.targets, again.link.targets);
        assert_eq!(program.link.code, again.link.code);
    }

    /// An operand naming an address the program does not have stays
    /// symbolic in the linked code, so the VM meets the same
    /// `UnresolvedDefinition` it did before — and everything after it in the
    /// container is still rewritten.
    #[test]
    fn unresolvable_target_stays_symbolic() {
        let mut data = compiled();
        // Find a Goto site and point it at an id nothing defines.
        let bogus = DefinitionId::new(brink_format::DefinitionTag::Address, 0x00DE_AD00_BEEF);
        let mut patched: Option<(usize, brink_format::TargetSite)> = None;
        'outer: for (i, c) in data.containers.iter().enumerate() {
            for (site, _) in target_sites(&c.bytecode) {
                if site.kind == brink_format::TargetKind::Goto {
                    patched = Some((i, site));
                    break 'outer;
                }
            }
        }
        let (ci, site) = patched.expect("the story has a Goto");
        data.containers[ci].bytecode[site.operand..site.end]
            .copy_from_slice(&bogus.to_raw().to_le_bytes());

        let (program, _) =
            link(&data).expect("an unresolvable divert is a run-time error, not a link error");
        let linked = &program.link.code[ci];
        assert_eq!(linked_ordinal(&linked[site.operand..site.end]), None);
        assert_eq!(
            &linked[site.operand..site.end],
            &bogus.to_raw().to_le_bytes()
        );
        assert!(
            !program.link.targets.iter().any(|t| t.id == bogus),
            "nothing interned for the bogus id"
        );
        assert!(program.resolve(bogus).is_err());
    }

    /// Regression for a fuzzer-discovered panic (`vm_no_panic`, PR #672
    /// workstream C): a `NameId` outside `StoryData::name_table`'s range —
    /// reachable from arbitrary/malformed `.inkb` bytes, not just
    /// well-formed compiler output — indexed the table directly and
    /// panicked (`index out of bounds`). Linking such a program must fail
    /// cleanly instead.
    fn story_with_out_of_range_address_path_name() -> StoryData {
        let mut data = brink_compiler::compile("main.ink", |_p| {
            Ok("=== knot ===\nHello.\n-> END\n".to_owned())
        })
        .unwrap()
        .data;
        assert!(
            !data.address_paths.is_empty(),
            "compiler output should carry an address_paths table"
        );
        data.address_paths[0].path = NameId(u16::MAX);
        data
    }

    #[test]
    fn link_rejects_out_of_range_address_path_name_id() {
        let data = story_with_out_of_range_address_path_name();
        let result = link(&data);
        assert!(
            matches!(result, Err(RuntimeError::InvalidNameId(id)) if id == u16::MAX),
            "out-of-range NameId must not link"
        );
    }
}
