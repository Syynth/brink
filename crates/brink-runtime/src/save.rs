//! Story `save_state` / `load_state`: produce and reconcile the durable,
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

use brink_format::{DefinitionId, LoadReport, SAVE_FORMAT_VERSION, SaveState, VisitEntry};

use crate::StoryRng;
use crate::debug::NameResolver;
use crate::story::Story;

impl<R: StoryRng> Story<'_, R> {
    /// Capture the default flow's game state as a durable, name-keyed
    /// [`SaveState`]. Does not capture execution position.
    #[must_use]
    pub fn save_state(&self) -> SaveState {
        let ctx = &self.default_context;
        let resolver = NameResolver::new(self.program());

        let globals = ctx
            .globals
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                self.program()
                    .global_slot_name(i)
                    .map(|name| (name.to_owned(), v.clone()))
            })
            .collect();

        let to_entries = |map: &std::collections::HashMap<DefinitionId, u32>| {
            let mut entries: Vec<VisitEntry> = map
                .iter()
                .map(|(&id, &count)| VisitEntry {
                    id,
                    path: resolver.def_path(id).map(str::to_owned),
                    count,
                })
                .collect();
            entries.sort_by_key(|e| e.id.to_raw());
            entries
        };

        SaveState {
            version: SAVE_FORMAT_VERSION,
            globals,
            visits: to_entries(&ctx.visit_counts),
            turns: to_entries(&ctx.turn_counts),
            turn_index: ctx.turn_index,
            rng_seed: ctx.rng_seed,
            previous_random: ctx.previous_random,
        }
    }

    /// Reconcile a [`SaveState`] into the default flow's context, returning a
    /// [`LoadReport`] of anything that couldn't be applied. Globals are matched
    /// by name; visit/turn counts by id. Tolerant of story patches: unknown
    /// globals are reported, scopes the program no longer has retain their
    /// saved counts harmlessly.
    pub fn load_state(&mut self, save: &SaveState) -> LoadReport {
        let mut report = LoadReport::default();

        for (name, value) in &save.globals {
            match self.program().global_index(name) {
                Some(idx) => self.default_context.set_global(idx, value.clone()),
                None => report.unknown_globals.push(name.clone()),
            }
        }

        let ctx = &mut self.default_context;
        ctx.turn_index = save.turn_index;
        ctx.rng_seed = save.rng_seed;
        ctx.previous_random = save.previous_random;
        for e in &save.visits {
            ctx.visit_counts.insert(e.id, e.count);
        }
        for e in &save.turns {
            ctx.turn_counts.insert(e.id, e.count);
        }

        report
    }
}
