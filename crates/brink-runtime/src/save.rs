//! `save_state` / `load_state`: produce and reconcile the durable,
//! name-keyed [`SaveState`] game-state save.
//!
//! [`SaveState`] (defined in `brink-format`) is distinct from the in-memory
//! [`StorySnapshot`](crate::StorySnapshot), which captures full execution
//! position and is locked to one exact program build. `SaveState` captures
//! only *game state* — globals, visit/turn counts, turn index, RNG — keyed by
//! stable identities (variable name; scope `DefinitionId`), so a save survives
//! a story recompile/patch as long as the relevant names/paths are unchanged.
//! Execution position is deliberately not captured; the host re-enters a
//! conversation at a known knot. See `docs/external-binding-foundation.md`.
//!
//! **F6.1b:** the logic lives as free functions over `&Program` +
//! `&impl ContextAccess`, not as `Story` methods, so any holder of a flow's
//! context — `Story`'s own `default_context`, or a `bevy-brink` `ContextView`
//! over a shared `World` plus an entity's `FlowLocal` — can save/load without
//! going through `Story` at all. [`Story::save_state`]/[`Story::load_state`]
//! now delegate to these, unchanged in observable behavior.
//!
//! **Enumeration.** `ContextAccess` has no iteration surface (a `ContextView`
//! can't hand back "every visited id" — it only answers point queries routed
//! by scope), so the candidate id set for visits/turns comes from the
//! `Program`'s own container definitions rather than map iteration. Every
//! container the VM ever visit/turn-counts carries `CountingFlags::VISITS`:
//! `vm.rs`'s `EnterContainer`/goto paths only ever call
//! `increment_visit`/`set_turn_count` when that flag is set on the target
//! container. (The converter *does* set `CountingFlags::TURNS` independently,
//! mirroring inklecate's container flags — but since every VM counting site
//! gates on `VISITS` alone, a TURNS-only container can never accrue a runtime
//! entry.) So containers with `VISITS` set are exactly the superset of ids
//! that could have a visit *or* turn entry. Iterating `Program::containers` (a `Vec`,
//! not a hash map) keeps enumeration order deterministic independent of
//! `Program`'s internal id tables.
//!
//! For each candidate id: `ContextAccess::visit_count` returns `0` for an id
//! the context has never visited (`World::increment_visit` only ever inserts
//! on the first increment, `or_insert(0) += 1`), so a `0` here means "never
//! visited" and is skipped — that reproduces the old code's
//! present-entries-only output, which iterated `World`'s
//! `visit_counts: HashMap` directly and so only ever saw ids that had
//! actually been inserted. `turn_count` returns `Option<u32>`, so absence is
//! directly distinguishable from an explicit `0` without a sentinel value.
//! Output is sorted by id explicitly (`Vec::sort_by_key`) rather than relying
//! on `Program::containers`' container-index order — byte-identical save
//! output is a hard requirement independent of container layout.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use brink_format::{
    ClosureEnvEntry, ClosureValue, CountingFlags, DefinitionId, LoadReport, OrderedMap,
    SAVE_FORMAT_VERSION, SaveState, Value, VisitEntry,
};

use crate::StoryRng;
use crate::debug::NameResolver;
use crate::program::Program;
use crate::state::ContextAccess;
use crate::story::Story;

/// Capture a flow's game state as a durable, name-keyed [`SaveState`]. Does
/// not capture execution position — see the module docs.
///
/// `ctx` can be any [`ContextAccess`] implementor: `World` directly, a
/// `ContextView` routing over `(World, FlowLocal)` (in which case every value
/// captured is the **effective** value for that flow — a `Local` override
/// where present, else `World`'s value on a read-through miss), or an
/// `ObservedContext` wrapping either.
#[must_use]
pub fn save_state<C: ContextAccess + ?Sized>(program: &Program, ctx: &C) -> SaveState {
    let resolver = NameResolver::new(program);

    let globals = (0..program.global_count())
        .filter_map(|idx| {
            program
                .global_slot_name(idx as usize)
                .map(|name| (name.to_owned(), ctx.global(idx).clone()))
        })
        .collect();

    let mut visits = Vec::new();
    let mut turns = Vec::new();
    for container in &program.containers {
        if !container.counting_flags.contains(CountingFlags::VISITS) {
            continue;
        }
        let id = container.id;

        let count = ctx.visit_count(id);
        if count > 0 {
            visits.push(VisitEntry {
                id,
                path: resolver.def_path(id).map(str::to_owned),
                count,
            });
        }

        if let Some(turn) = ctx.turn_count(id) {
            turns.push(VisitEntry {
                id,
                path: resolver.def_path(id).map(str::to_owned),
                count: turn,
            });
        }
    }
    visits.sort_by_key(|e| e.id.to_raw());
    turns.sort_by_key(|e| e.id.to_raw());

    SaveState {
        version: SAVE_FORMAT_VERSION,
        globals,
        visits,
        turns,
        turn_index: ctx.turn_index(),
        rng_seed: ctx.rng_seed(),
        previous_random: ctx.previous_random(),
    }
}

/// Reconcile a [`SaveState`] into a flow's context, returning a
/// [`LoadReport`] of anything that couldn't be applied. Globals are matched
/// by name; visit/turn counts by id. Tolerant of story patches: unknown
/// globals are reported, scopes the program no longer has retain their saved
/// counts harmlessly in the live context. Note one deliberate change from the
/// pre-F6.1b `Story` methods: such stale entries are **not re-emitted by a
/// subsequent [`save_state`]** (which enumerates the *current* program's
/// containers, not the live maps) — ghost counts from older program versions
/// no longer round-trip through saves indefinitely.
///
/// **M-3 rehydration miss-path lookup** (`docs/modules-spec.md` §5): a
/// visit/turn-count id, or a divert-target/fn-token/closure-target id
/// embedded inside a saved global's value, that the current program doesn't
/// recognize is looked up in the compiled `#@was` alias table before being
/// treated as genuinely gone — a knot/stitch/module rename that recorded
/// `#@was` rebinds saved state deterministically instead of orphaning it
/// under the stale id. Still unresolved after that (only checked, and only
/// reported, for a program that carries alias-table entries at all — an
/// ordinary content edit with no `#@was` stays exactly as silent as before
/// M-3) surfaces a teaching message in [`LoadReport::unresolved_renames`].
///
/// Writes go through [`ContextAccess`], so on a `ContextView` they route by
/// scope exactly like any other write: a `World`-scoped unit lands in the
/// shared `World`, a `Local`-scoped unit in the flow's own `FlowLocal`
/// override layer.
pub fn load_state<C: ContextAccess + ?Sized>(
    program: &Program,
    ctx: &mut C,
    save: &SaveState,
) -> LoadReport {
    let mut report = LoadReport::default();
    let renames_matter = program.has_aliases();

    for (name, value) in &save.globals {
        match program.global_index(name) {
            Some(idx) => {
                let value = if renames_matter {
                    rebind_value(program, value, &mut report)
                } else {
                    value.clone()
                };
                ctx.set_global(idx, value);
            }
            None => report.unknown_globals.push(name.clone()),
        }
    }

    ctx.set_turn_index(save.turn_index);
    ctx.set_rng_seed(save.rng_seed);
    ctx.set_previous_random(save.previous_random);
    for e in &save.visits {
        ctx.set_visit_count(rebind_address_key(program, e, &mut report), e.count);
    }
    for e in &save.turns {
        ctx.set_turn_count(rebind_address_key(program, e, &mut report), e.count);
    }

    report
}

// ─── M-3 rehydration miss-path lookup (docs/modules-spec.md §5) ───────────

/// Resolve a visit/turn-count entry's saved id against the current program,
/// falling back to the alias table on a direct miss. Reports a teaching
/// message when still unresolved (only for a program with any alias-table
/// entries, and only when the entry carries an author `path` to name in the
/// message — an anonymous synthetic address has nothing to teach a fix
/// against).
fn rebind_address_key(
    program: &Program,
    entry: &VisitEntry,
    report: &mut LoadReport,
) -> DefinitionId {
    let (id, unresolved) = rebind_address(program, entry.id);
    if unresolved
        && program.has_aliases()
        && let Some(path) = &entry.path
    {
        report
            .unresolved_renames
            .push(teach_was_message("visit count", path));
    }
    id
}

/// Resolve a single address-space id (container/scope/label) against the
/// current program, falling back to the alias table on a direct miss.
/// Returns the id to use and whether it's still unresolved after that (no
/// alias, or an alias whose own target doesn't resolve either) — the
/// compiler never emits a multi-hop alias chain (`old -> old2 -> new`), so
/// one alias lookup is always enough; a still-unresolved alias target means
/// the alias itself is stale (e.g. a further edit deleted the renamed
/// definition), which is the same "genuinely gone" outcome as no alias.
fn rebind_address(program: &Program, id: DefinitionId) -> (DefinitionId, bool) {
    if program.knows_address(id) {
        return (id, false);
    }
    match program.resolve_alias(id) {
        Some(new_id) => (new_id, !program.knows_address(new_id)),
        None => (id, true),
    }
}

/// Resolve a global-variable-pointer id (`Value::VariablePointer`) the same
/// way [`rebind_address`] resolves a container/address id, against the
/// global-slot namespace instead.
fn rebind_global(program: &Program, id: DefinitionId) -> (DefinitionId, bool) {
    if program.knows_global(id) {
        return (id, false);
    }
    match program.resolve_alias(id) {
        Some(new_id) => (new_id, !program.knows_global(new_id)),
        None => (id, true),
    }
}

/// The M-3 teaching fault message (`docs/modules-spec.md` §5): "saved
/// {subject} `{path}` resolves to nothing; if `{suggestion}` was renamed,
/// add `#@was({suggestion})`." The suggestion is the path's outermost
/// segment (module-qualified paths look like `module.knot`; the module is
/// usually the rename culprit for a multi-definition miss) falling back to
/// the whole path for an unqualified name (a bare knot rename).
fn teach_was_message(subject: &str, path: &str) -> String {
    let suggestion = path.split('.').next().unwrap_or(path);
    format!(
        "saved {subject} `{path}` resolves to nothing; if `{suggestion}` was renamed, add `#@was({suggestion})`"
    )
}

/// The M-3 teaching fault message for an id with no saved author path (a
/// divert target / fn token / closure target embedded in a global's value —
/// the wire format carries only the numeric id, never a path string).
fn teach_was_message_for_id(subject: &str, id: DefinitionId) -> String {
    format!(
        "saved {subject} {id} resolves to nothing; if its knot, stitch, or function was renamed, add `#@was(old_name)` to it"
    )
}

/// Rebind an address-space id (divert target / fn token / closure target)
/// found inside a saved `Value`, reporting a teaching message when it's
/// still unresolved after the alias-table lookup. Only called when the
/// program has alias-table entries (`load_state`'s `renames_matter` gate) —
/// the report only fires for a program that actually uses `#@was`.
fn rebind_value_address_id(
    program: &Program,
    subject: &str,
    id: DefinitionId,
    report: &mut LoadReport,
) -> DefinitionId {
    let (new_id, unresolved) = rebind_address(program, id);
    if unresolved {
        report
            .unresolved_renames
            .push(teach_was_message_for_id(subject, id));
    }
    new_id
}

/// Rebind a global-pointer id (`Value::VariablePointer`) found inside a
/// saved `Value`, same discipline as [`rebind_value_address_id`].
fn rebind_value_global_id(
    program: &Program,
    id: DefinitionId,
    report: &mut LoadReport,
) -> DefinitionId {
    let (new_id, unresolved) = rebind_global(program, id);
    if unresolved {
        report
            .unresolved_renames
            .push(teach_was_message_for_id("variable pointer", id));
    }
    new_id
}

/// Recursively rebind M-3 alias-table ids embedded anywhere inside a saved
/// `Value` — divert targets, fn tokens, closure targets and their `ref` env
/// entries, and any of those nested inside an array/map/record. A value
/// with no rename-affected id anywhere in it round-trips unchanged (modulo
/// the `Arc` rebuild collections/records always pay here — load is a
/// one-shot reconciliation, not a hot path, so the simpler always-recurse
/// shape wins over threading a "did anything change" flag through).
fn rebind_value(program: &Program, value: &Value, report: &mut LoadReport) -> Value {
    match value {
        Value::DivertTarget(id) => Value::DivertTarget(rebind_value_address_id(
            program,
            "divert target",
            *id,
            report,
        )),
        Value::FnRef(id) => Value::FnRef(rebind_value_address_id(program, "fn token", *id, report)),
        Value::VariablePointer(id) => {
            Value::VariablePointer(rebind_value_global_id(program, *id, report))
        }
        Value::Closure(c) => {
            let target = rebind_value_address_id(program, "fn token", c.target, report);
            let env = c
                .env
                .iter()
                .map(|e| ClosureEnvEntry {
                    name: e.name,
                    is_ref: e.is_ref,
                    payload: rebind_value(program, &e.payload, report),
                })
                .collect();
            Value::Closure(Arc::new(ClosureValue { target, env }))
        }
        Value::Array(items) => Value::array(
            items
                .iter()
                .map(|v| rebind_value(program, v, report))
                .collect::<Vec<_>>(),
        ),
        Value::Map(m) => {
            let rebound: OrderedMap = m
                .iter()
                .map(|(k, v)| (k.clone(), rebind_value(program, v, report)))
                .collect();
            Value::map(rebound)
        }
        Value::Record { shape, fields } => Value::Record {
            shape: *shape,
            fields: Arc::new(
                fields
                    .iter()
                    .map(|v| rebind_value(program, v, report))
                    .collect(),
            ),
        },
        other => other.clone(),
    }
}

impl<R: StoryRng> Story<R> {
    /// Capture the default flow's game state as a durable, name-keyed
    /// [`SaveState`]. Does not capture execution position. Thin delegating
    /// wrapper over the free [`save_state`] function — see the module docs.
    #[must_use]
    pub fn save_state(&self) -> SaveState {
        save_state(self.program(), &self.default_context)
    }

    /// Reconcile a [`SaveState`] into the default flow's context. Thin
    /// delegating wrapper over the free [`load_state`] function — see the
    /// module docs.
    pub fn load_state(&mut self, save: &SaveState) -> LoadReport {
        let program = self.program_arc();
        load_state(&program, &mut self.default_context, save)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::link;
    use crate::rng::FastRng;

    /// Compile a small ink story with the brink compiler and link it.
    fn compile_for_flow(src: &str) -> (Program, Vec<Vec<brink_format::LineEntry>>) {
        let out = brink_compiler::compile("t.ink", |p| {
            if p == "t.ink" {
                Ok(src.to_string())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no such include",
                ))
            }
        })
        .expect("compile");
        link(&out.data).expect("link")
    }

    /// `DefinitionId`s are content-hash-based (`brink_format::id`), not an
    /// incrementing counter tied to declaration order — so visiting knots in
    /// declaration order already scrambles hash order, and `Program`'s
    /// container `Vec` (declaration order) doesn't coincidentally match id
    /// order either. `save_state`'s explicit `sort_by_key` is what
    /// guarantees `SaveState::visits`/`turns` come out id-sorted regardless
    /// of visit order or container layout — this locks that invariant down.
    #[test]
    fn visits_are_sorted_by_id_regardless_of_visit_order() {
        let (program, tables) = compile_for_flow(
            "-> alpha\n\
             === alpha ===\n\
             Alpha.\n\
             -> DONE\n\
             === beta ===\n\
             Beta.\n\
             -> DONE\n\
             === gamma ===\n\
             Gamma.\n\
             -> DONE\n\
             === reader ===\n\
             {READ_COUNT(-> alpha)} {READ_COUNT(-> beta)} {READ_COUNT(-> gamma)}\n\
             -> DONE\n",
            // `reader` is never entered at runtime — it exists only so the
            // compiler's counting-flags pass (`apply_counting_flags` in
            // brink-ir) sees a `READ_COUNT` reference to each knot and sets
            // `CountingFlags::VISITS` on it. Without a visit-count *read*
            // somewhere in the program, the compiler leaves counting
            // disabled for a knot (an optimization) and the VM never calls
            // `increment_visit`/`set_turn_count` for it at all.
        );
        let program = Arc::new(program);
        let mut story = crate::Story::<FastRng>::new(Arc::clone(&program), tables);

        // Visit alpha (root divert), then gamma, then beta — an order that
        // matches neither declaration order nor (necessarily) id order.
        story.continue_maximally().expect("continue");
        story.choose_path_string("gamma").expect("jump");
        story.continue_maximally().expect("continue");
        story.choose_path_string("beta").expect("jump");
        story.continue_maximally().expect("continue");

        let save = story.save_state();
        assert_eq!(
            save.visits.len(),
            3,
            "alpha/beta/gamma should each have a visit entry: {:?}",
            save.visits
        );

        let ids: Vec<u64> = save.visits.iter().map(|e| e.id.to_raw()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "SaveState::visits must be sorted by id");
    }
}
