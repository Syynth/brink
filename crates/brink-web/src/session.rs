use std::cell::{Cell, RefCell};
use std::sync::Arc;

use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult, FastRng};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::external_binding::{BusyGuard, reentrant_error};
use crate::program_model;
use crate::value_marshal::{LineJs, debug_snapshot_to_js, js_to_value, line_to_js, value_to_js};

// ── WebSession (Story Session wasm bindings, #370/#387) ──────────────
//
// `WebSession` wraps `brink_runtime::StorySession` — the Rust-canonical
// journal + replay layer (see `docs/story-session-spec.md`). Additive: this
// section introduces new wasm-exported types alongside `StoryRunner` above;
// it does not touch or reorganize any existing binding. Same Box-pinning
// self-referential pattern as `StoryRunner::new` (the `Program` is
// heap-pinned and outlives the `StorySession`, which borrows it for
// `'static`; wasm is single-threaded and the struct's field-drop order keeps
// the session dropped before the program).
//
// Wire-format fix (the issue's headline item): `StepOutcome` is split OUT of
// the `Line` JSON union into its own `{ type: "line", line: Line } | { type:
// "awaiting_external", deferred: bool, name?: string }` shape — unlike
// `StoryRunner::advance_one`'s `LineJs`, which smuggles `awaiting_external`
// into the `Line` type tag. The two park states stay distinct: a
// promise-in-flight (JsHandler-style async bindings the session awaits
// internally — never surfaced as `awaiting_external` here, since
// `WebSession` does not register JS bindings; see below) vs. a *deferred*
// out-of-band pause the host must resolve via `resolveExternal`. Because
// `WebSession` has no JS-binding registry of its own (unlike `StoryRunner`),
// every external the wrapped VM can't resolve inline is a deferred pause —
// `deferred` is always `true` on `awaiting_external` from this type. The
// `deferred: string[]` constructor option (below) additionally forces named
// externals to *always* park, even when a fallback body exists, so a host
// can route specific calls out-of-band unconditionally.
#[wasm_bindgen]
pub struct WebSession {
    // Shared ownership: `StorySession` holds an `Arc::clone`, so there is no
    // self-referential borrow to work around (mirrors `StoryRunner`).
    program: Arc<brink_runtime::Program>,
    base_line_tables: Vec<Vec<brink_format::LineEntry>>,
    /// The decoded `StoryData` this session is running — kept (mirroring
    /// `StoryRunner::data`) so the Program Explorer facts (`program_model`,
    /// `program_inkt`) can be derived without re-decoding `story_bytes`.
    /// Compile-bound, not session-bound: swapped on `reload`, untouched by
    /// `restart` (same program, fresh session).
    data: brink_format::StoryData,
    session: RefCell<Option<brink_runtime::StorySession<FastRng>>>,
    /// Explicit RNG seed, if the host set one. Re-applied on `restart`/
    /// `reload` so re-runs stay deterministic.
    seed: Cell<Option<i32>>,
    /// External names that always park as `awaiting_external` regardless of
    /// whether the story defines a fallback body — the `deferred: string[]`
    /// constructor option. Checked before falling through to the ink body.
    always_deferred: std::collections::BTreeSet<String>,
    /// Reentrancy guard, mirroring `StoryRunner::busy`.
    busy: Cell<bool>,
    /// The M-2b dev-tooling visibility override (play-from-here) — mirrors
    /// `StoryRunner::dev_visibility_override`. When `true`, host semantic
    /// access to `#@private` definitions is allowed (enforcement off).
    /// Re-applied across `restart`/`reload`. Default `false`.
    dev_visibility_override: Cell<bool>,
    /// The `ReplayOutcome` JSON produced by the most recent `restore`/`reload`
    /// on this session, if any — wasm constructors/`&mut self` methods can
    /// only return one value, so `restore`'s outcome is stashed here for
    /// `lastReplayOutcome` to read right after construction/reload.
    last_replay_outcome: RefCell<Option<String>>,
    /// Debug-control breakpoints (D8, #3186) armed on this session — checked
    /// by `debugRun`/`debugStep` (#3232). Caller-owned per
    /// `BreakpointSet`'s own doc, kept here so a JS host doesn't need to
    /// round-trip the whole set across the wasm boundary on every call.
    /// Position-keyed against `self.program`; untouched by `restart` (same
    /// program) — a `reload` onto a *different* compile can make a stale
    /// breakpoint's position meaningless, same caveat
    /// `resolve_debug_position`'s doc already carries for program identity.
    breakpoints: RefCell<brink_runtime::BreakpointSet>,
    /// Break-on-write data breakpoints (W18/#3311): the watched globals'
    /// AUTHOR NAMES. Indices are re-resolved against the CURRENT program
    /// on every watching advance — names survive a hot reload where a
    /// stored slot index would silently watch the wrong global.
    watched_globals: RefCell<Vec<String>>,
}

#[wasm_bindgen]
impl WebSession {
    /// Create a new session from compiled story bytes. `seed`, if given, seeds
    /// the RNG immediately and is re-applied across `restart`/`reload`.
    /// `deferred` names externals that must always park as
    /// `awaiting_external` (out-of-band), even when the story defines a
    /// fallback body for them — pass `undefined`/omit for none.
    #[wasm_bindgen(constructor)]
    pub fn new(
        story_bytes: &[u8],
        seed: Option<i32>,
        deferred: Option<Vec<String>>,
    ) -> Result<WebSession, JsError> {
        let data = brink_format::read_inkb(story_bytes)
            .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let (prog, line_tables) =
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?;
        let program = Arc::new(prog);

        let mut story =
            brink_runtime::Story::<FastRng>::new(Arc::clone(&program), line_tables.clone());
        if let Some(s) = seed {
            story.set_rng_seed(s);
        }
        let session = brink_runtime::StorySession::new(story, seed.map(seed_to_journal_u64));

        Ok(WebSession {
            program,
            base_line_tables: line_tables,
            data,
            session: RefCell::new(Some(session)),
            seed: Cell::new(seed),
            always_deferred: deferred.unwrap_or_default().into_iter().collect(),
            busy: Cell::new(false),
            last_replay_outcome: RefCell::new(None),
            dev_visibility_override: Cell::new(false),
            breakpoints: RefCell::new(brink_runtime::BreakpointSet::new()),
            watched_globals: RefCell::new(Vec::new()),
        })
    }

    // ── Program inspection (Program Explorer / State View) ──────────

    /// A typed, name-resolved runtime snapshot (current location, globals,
    /// call stack, visit counts, pending choices, RNG state) for the studio's
    /// State View. Mirrors `StoryRunner::debug_snapshot` — live position, not
    /// compile-bound, so it reflects wherever the session currently is (unlike
    /// `program_model`/`program_inkt` below). Reached through the documented
    /// `StorySession::story()` escape hatch (read-only; bypasses the journal
    /// since nothing is mutated).
    pub fn debug_snapshot(&self) -> Result<String, JsError> {
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let js = debug_snapshot_to_js(session.story().debug_snapshot());
        serde_json::to_string(&js).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The program→source resolver (D9, issue #3187) — the studio Location
    /// protocol's `program` resolver (`docs/studio-shell-spec.md` §6.1)
    /// resolves through this. Resolves a `(container_idx, offset)` bytecode
    /// position — exactly what `debug_snapshot()`'s `position`/call-stack
    /// frame `position` fields report (D4, #3182) — to the source range it
    /// was compiled from, via the loaded program's `DebugInfo` section (D6,
    /// #3184). Mirrors `StoryRunner::resolve_debug_position` — this is the
    /// `WebSession` (journal/replay-session) counterpart, since
    /// `LocalSessionProvider` (`packages/studio-store`) drives the studio's
    /// live session through `StorySessionHandle`/`WebSession`, not
    /// `StoryRunner`/`StoryRunnerHandle`.
    ///
    /// Returns JSON `null`, not an error, when the program carries no
    /// `DebugInfo` section (a compile without `--debug-info`) or the
    /// position doesn't resolve. Otherwise `{ "file": string | null,
    /// "range_start": number, "range_len": number }`.
    ///
    /// Callers MUST gate this on program identity before trusting the
    /// result for anything source-position-sensitive — see
    /// `StoryRunner::resolve_debug_position`'s doc for the full argument;
    /// `docs/live-inspector-spec.md` §5's `sessionDegraded` predicate is
    /// the gate.
    pub fn resolve_debug_position(
        &self,
        container_idx: u32,
        offset: u32,
    ) -> Result<String, JsError> {
        let position = brink_runtime::DebugPosition {
            container_idx,
            offset: offset as usize,
        };
        let resolved = self
            .program
            .resolve_debug_position(position)
            .map(crate::value_marshal::debug_source_location_to_js);
        serde_json::to_string(&resolved).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The inverse of [`Self::resolve_debug_position`] (#3246): the program
    /// address to break on for a span of source text, or `null` when the
    /// span holds no executable code (a comment, a blank line, a line whose
    /// code folded away) — or when this artifact carries no `DebugInfo`.
    ///
    /// `start`/`end` are a half-open **byte** range in `file`. For the
    /// `file:line` shape a gutter naturally has, use
    /// [`Self::resolve_source_line`] — the `DebugInfo` file table carries a
    /// per-file line index (#3261), so no source text is needed.
    ///
    /// `null` is a real answer a gutter must render, not an error to
    /// swallow: refusing to arm visibly beats arming a breakpoint that can
    /// never hit.
    pub fn resolve_source_range(
        &self,
        file: &str,
        start: u32,
        end: u32,
    ) -> Result<String, JsError> {
        let resolved = self
            .program
            .resolve_source_range(file, start, end)
            .map(|p| {
                serde_json::json!({
                    "container_idx": p.container_idx,
                    "offset": p.offset,
                })
            });
        serde_json::to_string(&resolved).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The `file:line` of a bytecode position (W6/#3299) — the
    /// program→source resolver reduced to the shape the execution
    /// highlight and the paused chip consume:
    /// `{ "file": string, "line": number }` (0-based line) or `null`
    /// (unresolvable, synthetic, or no line index). Same degraded-gating
    /// contract as `resolve_debug_position` — the caller compares program
    /// identity first.
    #[wasm_bindgen(js_name = resolveDebugLine)]
    pub fn resolve_debug_line(&self, container_idx: u32, offset: u32) -> Result<String, JsError> {
        let position = brink_runtime::DebugPosition {
            container_idx,
            offset: offset as usize,
        };
        let resolved = self.program.resolve_debug_line(position).map(|l| {
            serde_json::json!({
                "file": l.file,
                "line": l.line,
                "range_start": l.range_start,
                "range_len": l.range_len,
            })
        });
        serde_json::to_string(&resolved).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The program address to break on for a **line** of source (W2/#3295,
    /// runtime half #3261): `{ "container_idx": number, "offset": number }`
    /// or `null`. `line` is **0-based** — a UI showing 1-based numbers
    /// converts at its own edge (the fencepost lives in one place per
    /// consumer, exactly the runtime's contract).
    ///
    /// `null` when the artifact carries no `DebugInfo`, the file is
    /// unknown or has no line index, or the line holds no executable code
    /// (a comment, a blank, a line whose code folded away). Same
    /// refuse-to-arm-visibly contract as [`Self::resolve_source_range`];
    /// use [`Self::has_debug_info`] to tell "no section" from "nothing on
    /// that line" when wording the refusal.
    pub fn resolve_source_line(&self, file: &str, line: u32) -> Result<String, JsError> {
        let resolved = self.program.resolve_source_line(file, line).map(|p| {
            serde_json::json!({
                "container_idx": p.container_idx,
                "offset": p.offset,
            })
        });
        serde_json::to_string(&resolved).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Whether the loaded program carries a `DebugInfo` section at all
    /// (W2/#3295; runtime #3248). The honest-refusal discriminator: every
    /// resolver here returns `null` both for "compiled without debug info"
    /// and for "nothing at that position", and a frontend must tell the
    /// author which — "that line has no executable code" is a wild-goose
    /// chase when the truth is a compiler flag (or the App-settings
    /// opt-out, W1/#3294).
    #[must_use]
    pub fn has_debug_info(&self) -> bool {
        self.program.has_debug_info()
    }

    /// Whether `text` is byte-identical to the source `file` was compiled
    /// from (W2/#3295; runtime #3261) — the **per-file** staleness gate:
    /// `true`/`false`, or `null` for "cannot tell" (no `DebugInfo`, file
    /// unknown, or the compile recorded no source hash). Per-file
    /// deliberately: one dirty file degrades debugging in that file alone,
    /// where the whole-program `sessionDegraded` checksum degrades
    /// everything. `null` must not be collapsed into "stale" — a hash-less
    /// artifact would then look permanently dirty.
    pub fn source_matches(&self, file: &str, text: &str) -> Result<String, JsError> {
        serde_json::to_string(&self.program.source_matches(file, text))
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The program address of a named knot/stitch/function path (W2/#3295)
    /// — `Program::definition_id_for_path` + `resolve_address` composed,
    /// the two halves #3187 named as "both already public and can bridge
    /// them — nothing does". `{ "container_idx": number, "offset": number }`
    /// or `null` for an unknown path. This is name-based addressing ("break
    /// on `tavern.order`", the launcher's play-from typeahead) — no
    /// `DebugInfo` section required, since it reads the container table.
    pub fn resolve_path_address(&self, path: &str) -> Result<String, JsError> {
        let resolved = self
            .program
            .definition_id_for_path(path)
            .and_then(|id| self.program.resolve_address(id))
            .map(|(container_idx, offset)| {
                serde_json::json!({
                    "container_idx": container_idx,
                    "offset": offset,
                })
            });
        serde_json::to_string(&resolved).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    // ── Debug control (D8, #3186 — the control-half wasm bridge, #3232) ──
    //
    // `debugRun`/`debugStep`/`debugBreakpoint*` bind `Story::debug_run`/
    // `debug_step`/`BreakpointSet` onto the session — the studio's actual
    // drive path (`LocalSessionProvider` runs `WebSession`, not
    // `StoryRunner`; see `StoryRunner`'s copy of these methods for parity).
    // Reached through `StorySession::story_mut()`, the documented
    // journal-bypass escape hatch `setDevVisibilityOverride` above already
    // uses — debug stepping is not a turn the player took, so it must not be
    // journaled (a resumed session must not replay debugger single-steps).

    /// Add an enabled breakpoint at `(container_idx, offset)`, returning its
    /// id — pass it to `debugBreakpointRemove`/`debugBreakpointSetEnabled`.
    /// An empty/omitted `name` is replaced with a `container:offset` label
    /// (`BreakpointSet::insert`'s own doc).
    #[wasm_bindgen(js_name = debugBreakpointAdd)]
    pub fn debug_breakpoint_add(
        &self,
        container_idx: u32,
        offset: u32,
        name: Option<String>,
    ) -> u32 {
        self.breakpoints.borrow_mut().insert(
            container_idx,
            offset as usize,
            name.unwrap_or_default(),
        )
    }

    /// Remove a breakpoint by id. Returns `false` if no breakpoint with that
    /// id exists (already removed, or never added).
    #[wasm_bindgen(js_name = debugBreakpointRemove)]
    pub fn debug_breakpoint_remove(&self, id: u32) -> bool {
        self.breakpoints.borrow_mut().remove(id)
    }

    /// Enable/disable a breakpoint without removing it. Returns `false` if
    /// no breakpoint with that id exists.
    #[wasm_bindgen(js_name = debugBreakpointSetEnabled)]
    pub fn debug_breakpoint_set_enabled(&self, id: u32, enabled: bool) -> bool {
        self.breakpoints.borrow_mut().set_enabled(id, enabled)
    }

    /// Every breakpoint currently armed on this session, in insertion order
    /// (deterministic — `BreakpointSet` is a `Vec`, not a hash map). Returns
    /// JSON `Breakpoint[]`.
    #[wasm_bindgen(js_name = debugBreakpoints)]
    pub fn debug_breakpoints(&self) -> Result<String, JsError> {
        let list: Vec<_> = self
            .breakpoints
            .borrow()
            .iter()
            .map(crate::value_marshal::breakpoint_to_js)
            .collect();
        serde_json::to_string(&list).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Arm a break-on-write data breakpoint on a global (W18/#3311,
    /// RULED). Stored by AUTHOR NAME — the slot index is re-resolved
    /// against the current program on every watching advance, so the arm
    /// survives hot reloads. `false` = no such global in the current
    /// program (nothing armed), or already watched.
    #[wasm_bindgen(js_name = debugWatchpointAdd)]
    pub fn debug_watchpoint_add(&self, name: &str) -> Result<bool, JsError> {
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        if session.story().program().global_index(name).is_none() {
            return Ok(false);
        }
        let mut watched = self.watched_globals.borrow_mut();
        if watched.iter().any(|w| w == name) {
            return Ok(false);
        }
        watched.push(name.to_owned());
        Ok(true)
    }

    /// Disarm a data breakpoint by name. `false` = wasn't armed.
    #[wasm_bindgen(js_name = debugWatchpointRemove)]
    pub fn debug_watchpoint_remove(&self, name: &str) -> bool {
        let mut watched = self.watched_globals.borrow_mut();
        let before = watched.len();
        watched.retain(|w| w != name);
        watched.len() != before
    }

    /// The armed data breakpoints' names, in arm order (JSON `string[]`).
    #[wasm_bindgen(js_name = debugWatchpoints)]
    pub fn debug_watchpoints(&self) -> Result<String, JsError> {
        serde_json::to_string(&*self.watched_globals.borrow())
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Build the per-advance observer from the armed names, resolved
    /// against the CURRENT program (see `watched_globals`' doc). `None`
    /// when nothing is armed — the advance runs unobserved, as before.
    fn watch_observer(
        session: &brink_runtime::StorySession<brink_runtime::FastRng>,
        watched: &[String],
    ) -> Option<brink_runtime::WatchpointObserver> {
        if watched.is_empty() {
            return None;
        }
        let program = session.story().program();
        let indices: Vec<u32> = watched
            .iter()
            .filter_map(|name| program.global_index(name))
            .collect();
        if indices.is_empty() {
            return None;
        }
        Some(brink_runtime::WatchpointObserver::new(indices))
    }

    /// Resolve a watchpoint stop's slot index back to its author name in
    /// the marshaled outcome (the wire carries both, W18).
    fn name_watchpoint_stop(
        js: &mut crate::value_marshal::DebugRunOutcomeJs,
        watched: &[String],
        session: &brink_runtime::StorySession<brink_runtime::FastRng>,
    ) {
        if let crate::value_marshal::DebugStopReasonJs::Watchpoint { global_idx, name } =
            &mut js.reason
        {
            let program = session.story().program();
            *name = watched
                .iter()
                .find(|w| program.global_index(w) == Some(*global_idx))
                .cloned();
        }
    }

    /// Run the default flow forward until an armed breakpoint, a choice
    /// point, or a terminal outcome (`Story::debug_run`'s own doc has the
    /// full stop-condition/resume contract). `budget_ceiling` defaults to
    /// `DEFAULT_DEBUG_BUDGET` when omitted — the debug-only step ceiling,
    /// entirely separate from production's step limit. Returns JSON
    /// `DebugRunOutcome`.
    #[wasm_bindgen(js_name = debugRun)]
    pub fn debug_run(&self, budget_ceiling: Option<u32>) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("debug_run"))?;
        let ceiling = budget_ceiling.map_or(brink_runtime::DEFAULT_DEBUG_BUDGET, u64::from);
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let breakpoints = self.breakpoints.borrow();
        let watched = self.watched_globals.borrow().clone();
        // Drain the shared delivery cursor around the advance (W5/#3298):
        // lines the production lookahead already completed surface here
        // exactly once, and lines this advance completes follow them.
        let mut drained = session.story_mut().debug_drain_buffered_lines();
        // W18: armed data breakpoints route the run through the watching
        // variant — a watched write stops it with the writer's position.
        let outcome = match Self::watch_observer(session, &watched) {
            Some(mut w) => session
                .story_mut()
                .debug_run_watching(&breakpoints, &mut w, ceiling),
            None => session.story_mut().debug_run(&breakpoints, ceiling),
        }
        .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        drained.extend(session.story_mut().debug_drain_buffered_lines());
        let lines = crate::value_marshal::drained_lines_to_js(drained);
        let mut js = crate::value_marshal::debug_run_outcome_to_js(outcome, lines);
        Self::name_watchpoint_stop(&mut js, &watched, session);
        serde_json::to_string(&js).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Run the default flow forward until the next **content line** is
    /// delivered (2026-08-30 Continue ruling — the granularity ladder's
    /// top tier), or a breakpoint/choice point/terminal/deferred external
    /// stops it early. The stop lands past the line's glue/commit
    /// boundary, so the crossed line is IN this outcome's `lines` — no
    /// one-advance delivery lag (#3321). Needs no debug line info (the
    /// stop condition is output-buffer state). Same budget default and
    /// journal-bypass contract as `debugRun`. Returns JSON
    /// `DebugRunOutcome`.
    #[wasm_bindgen(js_name = debugRunToLine)]
    pub fn debug_run_to_line(&self, budget_ceiling: Option<u32>) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("debug_run_to_line"))?;
        let ceiling = budget_ceiling.map_or(brink_runtime::DEFAULT_DEBUG_BUDGET, u64::from);
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let breakpoints = self.breakpoints.borrow();
        let watched = self.watched_globals.borrow().clone();
        // Drain the shared delivery cursor around the advance (W5/#3298):
        // a line the production lookahead already completed satisfies the
        // "one content line delivered" contract by itself — the runtime
        // verb only runs when the cursor starts empty.
        let mut drained = session.story_mut().debug_drain_buffered_lines();
        if drained.is_empty() {
            // W18: the Continue tier honors data breakpoints — a watched
            // write stops before the next content line commits.
            let outcome = match Self::watch_observer(session, &watched) {
                Some(mut w) => {
                    session
                        .story_mut()
                        .debug_run_to_line_watching(&breakpoints, &mut w, ceiling)
                }
                None => session.story_mut().debug_run_to_line(&breakpoints, ceiling),
            }
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
            drained.extend(session.story_mut().debug_drain_buffered_lines());
            let lines = crate::value_marshal::drained_lines_to_js(drained);
            let mut js = crate::value_marshal::debug_run_outcome_to_js(outcome, lines);
            Self::name_watchpoint_stop(&mut js, &watched, session);
            return serde_json::to_string(&js)
                .map_err(|e| JsError::new(&format!("json error: {e}")));
        }
        let position = session.story_mut().debug_position();
        let depth = session.story_mut().debug_call_stack_depth();
        let lines = crate::value_marshal::drained_lines_to_js(drained);
        serde_json::to_string(&crate::value_marshal::debug_run_outcome_to_js(
            brink_runtime::DebugRunOutcome {
                reason: brink_runtime::DebugStopReason::Step,
                position,
                depth,
            },
            lines,
        ))
        .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Step the default flow by one unit — `mode` is `"into"` | `"over"` |
    /// `"out"` (`StepMode`'s own doc has the exact depth-delta semantics per
    /// variant). Same budget default and journal-bypass contract as
    /// `debugRun`. Returns JSON `DebugRunOutcome`.
    #[wasm_bindgen(js_name = debugStep)]
    pub fn debug_step(&self, mode: &str, budget_ceiling: Option<u32>) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("debug_step"))?;
        let step_mode = crate::value_marshal::parse_step_mode(mode)?;
        let ceiling = budget_ceiling.map_or(brink_runtime::DEFAULT_DEBUG_BUDGET, u64::from);
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let breakpoints = self.breakpoints.borrow();
        // Drain the shared delivery cursor around the advance (W5/#3298):
        // lines the production lookahead already completed surface here
        // exactly once, and lines this advance completes follow them.
        let mut drained = session.story_mut().debug_drain_buffered_lines();
        let outcome = session
            .story_mut()
            .debug_step(step_mode, &breakpoints, ceiling)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        drained.extend(session.story_mut().debug_drain_buffered_lines());
        let lines = crate::value_marshal::drained_lines_to_js(drained);
        serde_json::to_string(&crate::value_marshal::debug_run_outcome_to_js(
            outcome, lines,
        ))
        .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Step the default flow to the next **source line** (#3264, W5/#3298)
    /// — the granularity every GDB-style debugger means by `step`/`next`,
    /// and the unified Player advance when breakpoints are armed: one
    /// visible line, bounded by any armed breakpoint, whichever comes
    /// first. `mode` and budget as `debugStep`; same journal-bypass
    /// contract. Returns JSON `DebugRunOutcome` (with the emitted-lines
    /// delta), reason `noLineInfo` when the artifact carries no line index.
    #[wasm_bindgen(js_name = debugStepLine)]
    pub fn debug_step_line(
        &self,
        mode: &str,
        budget_ceiling: Option<u32>,
    ) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("debug_step_line"))?;
        let step_mode = crate::value_marshal::parse_step_mode(mode)?;
        let ceiling = budget_ceiling.map_or(brink_runtime::DEFAULT_DEBUG_BUDGET, u64::from);
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let breakpoints = self.breakpoints.borrow();
        // Drain the shared delivery cursor around the advance (W5/#3298):
        // lines the production lookahead already completed surface here
        // exactly once, and lines this advance completes follow them.
        let mut drained = session.story_mut().debug_drain_buffered_lines();
        let outcome = session
            .story_mut()
            .debug_step_line(step_mode, &breakpoints, ceiling)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        drained.extend(session.story_mut().debug_drain_buffered_lines());
        let lines = crate::value_marshal::drained_lines_to_js(drained);
        serde_json::to_string(&crate::value_marshal::debug_run_outcome_to_js(
            outcome, lines,
        ))
        .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The compiled program rendered as `.inkt` text for the Program Explorer:
    /// checksum, name table, globals, lists, externals, address paths, and
    /// containers with bytecode disassembly. Static for the loaded program —
    /// mirrors `StoryRunner::program_inkt`.
    pub fn program_inkt(&self) -> Result<String, JsError> {
        let mut out = String::new();
        brink_format::write_inkt(&self.data, &mut out)
            .map_err(|e| JsError::new(&format!("inkt error: {e}")))?;
        Ok(out)
    }

    /// Structured model of the compiled program for the Program Explorer:
    /// globals / lists / externals tables plus a knot/stitch tree with
    /// per-knot, name-resolved bytecode disassembly. Static for the loaded
    /// program — mirrors `StoryRunner::program_model`. Returns JSON.
    pub fn program_model(&self) -> Result<String, JsError> {
        let model = program_model::build(&self.data);
        serde_json::to_string(&model).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    // ── Shared flows (#200) ───────────────────────────────────────
    // Mirrors `StoryRunner`'s shared-flow surface exactly, delegating through
    // the documented `story()`/`story_mut()` escape hatch: a flow spawned here
    // SHARES this session's `Story` (globals / visit counts / rng), so it
    // stays coherent with whatever the session itself is driving. Flow
    // stepping bypasses the journal by design — the journal spec explicitly
    // reserves this as "shared flows keep working; their externals never
    // journal".

    /// Spawn a shared-context flow, started at the program root (or `path`).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen passes an optional JS string as an owned value across the boundary"
    )]
    pub fn spawn_flow(&self, name: &str, path: Option<String>) -> Result<(), JsError> {
        let _guard = BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("spawn_flow"))?;
        let container_idx = match path.as_deref() {
            None => None,
            Some(p) => Some(
                self.program
                    .find_address(p)
                    .map(|(idx, _)| idx)
                    .ok_or_else(|| JsError::new(&format!("unknown path: {p}")))?,
            ),
        };
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        session
            .story_mut()
            .spawn_flow_shared(name, container_idx)
            .map_err(|e| JsError::new(&format!("spawn_flow error: {e}")))
    }

    /// Advance a shared flow by one line. Returns one `Line` JSON. Externals
    /// resolve via the ink fallback body only (no JS-binding registry, same
    /// as the default flow) and are never journaled.
    pub fn continue_flow(&self, name: &str) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("continue_flow"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let line = session
            .story_mut()
            .continue_flow_single_with(name, &brink_runtime::FallbackHandler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        serde_json::to_string(&line_to_js(line))
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Run a shared flow to its next terminal line. Returns JSON array of
    /// `Line` objects, capped at `FlowInstance::LINE_LIMIT` (10,000) lines —
    /// the same bound `continue_story`/`continue_single` enforce for the
    /// primary flow, so an infinite-emitting flow errors instead of hanging
    /// or exhausting memory on the host. Externals resolve via the ink fallback body only
    /// and are never journaled (same as `continue_flow`).
    pub fn continue_flow_maximally(&self, name: &str) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy)
            .ok_or_else(|| reentrant_error("continue_flow_maximally"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let lines = session
            .story_mut()
            .continue_flow_maximally_shared_with(name, &brink_runtime::FallbackHandler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        let resp: Vec<LineJs> = lines.into_iter().map(line_to_js).collect();
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Select a choice in a shared flow.
    pub fn choose_flow(&self, name: &str, index: usize) -> Result<(), JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("choose_flow"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        session
            .story_mut()
            .choose_flow_shared(name, index)
            .map_err(|e| JsError::new(&format!("choose_flow error: {e}")))
    }

    /// Destroy a shared flow.
    pub fn destroy_flow(&self, name: &str) -> Result<(), JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("destroy_flow"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        session
            .story_mut()
            .destroy_flow(name)
            .map_err(|e| JsError::new(&format!("destroy_flow error: {e}")))
    }

    /// JSON array of active flow names (sorted, deterministic).
    pub fn flow_names(&self) -> Result<String, JsError> {
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        serde_json::to_string(&session.story().flow_names())
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Per-flow debug snapshot (State View) for a named flow.
    pub fn flow_debug_snapshot(&self, name: &str) -> Result<String, JsError> {
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let snap = session
            .story()
            .debug_snapshot_flow(name)
            .map_err(|e| JsError::new(&format!("flow error: {e}")))?;
        serde_json::to_string(&debug_snapshot_to_js(snap))
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Re-evaluate parked flows' wake conditions and return a JSON array of
    /// the flow ids that woke (`docs/flow-suspension-spec.md` §10.2).
    /// Waking never auto-continues — drive a woken flow with `continueFlow`.
    ///
    /// **Returns `[]` until parks exist (FS-3r).** No flow can park in
    /// today's runtime (the E052 fence keeps `await` from lowering, so
    /// `Line::Suspended` is unreachable). Exported now so hosts wire the
    /// wake loop against a stable shape.
    pub fn wake_check(&self) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("wake_check"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        serde_json::to_string(&session.story_mut().wake_check())
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    // ── Stepping ──────────────────────────────────────────────────

    /// Advance one step. Returns `StepOutcomeJs` JSON: `{ "type": "line",
    /// "line": Line }` or `{ "type": "awaiting_external", "deferred": true,
    /// "name": "..." }`. A deferred external must be resolved via
    /// `resolveExternal` before stepping again.
    pub fn advance(&self) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("advance"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let handler = AlwaysDeferHandler {
            always_deferred: &self.always_deferred,
        };
        let outcome = session
            .advance_with(&handler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        let resp = step_outcome_to_js(outcome, session);
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Advance until one line of content or a yield point. Returns `Line` JSON
    /// (never `awaiting_external` — externals resolve inline or via the ink
    /// fallback body; use `advance`/`resolveExternal` for deferred bindings).
    pub fn continue_single(&self) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("continue_single"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let line = session
            .continue_single()
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        serde_json::to_string(&line_to_js(line))
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Advance to the next pause. Returns a JSON array of `Line`; the last
    /// element is always terminal (`done` / `choices` / `end`).
    pub fn continue_to_pause(&self) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("continue_to_pause"))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let lines = session
            .continue_to_pause()
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        let resp: Vec<LineJs> = lines.into_iter().map(line_to_js).collect();
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Select a choice by index, journaling the `Choice` event.
    pub fn choose(&self, index: usize) -> Result<(), JsError> {
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        session
            .choose(index)
            .map_err(|e| JsError::new(&format!("choose error: {e}")))
    }

    /// Resolve the external the session is parked on (deferred out-of-band
    /// pause from `advance`). No-op if not awaiting.
    pub fn resolve_external(&self, value: &JsValue) {
        if self.busy.get() {
            web_sys::console::warn_1(&JsValue::from_str(
                "brink: reentrant 'resolve_external' ignored",
            ));
            return;
        }
        let v = js_to_value(value);
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.resolve_external(v);
        }
    }

    /// Whether the session is parked on a deferred external.
    #[must_use]
    pub fn has_pending_external(&self) -> bool {
        self.session
            .borrow()
            .as_ref()
            .is_some_and(brink_runtime::StorySession::has_pending_external)
    }

    /// Whether the last execution cycle of the **default** flow ended with
    /// a safe exit (an explicit `-> DONE`), as opposed to the flow running
    /// out of content. Both deliver a `done`-type `Line`; read this right
    /// after one to tell them apart — `false` means the next
    /// `continueSingle`/`advance` call will error instead of returning
    /// more text. `false` if no session is initialized.
    ///
    /// This reflects only the default flow — it does **not** track flows
    /// spawned/continued via `spawnFlow`/`continueFlow`/
    /// `continueFlowMaximally`. See
    /// [`brink_runtime::Story::did_safe_exit`] (issue #1573).
    #[must_use]
    pub fn did_safe_exit(&self) -> bool {
        self.session
            .borrow()
            .as_ref()
            .is_some_and(|s| s.story().did_safe_exit())
    }

    // ── Turn-boundary mutations (journaled) ──────────────────────

    /// Set a global variable. Turn-boundary only: errors mid-turn (drain the
    /// current turn to `done`/`choices`/`end` first). Returns `false` if no
    /// such global is declared (no-op, not journaled).
    pub fn set_var(&self, name: &str, value: &JsValue) -> Result<bool, JsError> {
        let v = js_to_value(value);
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        session
            .set_var(name, v)
            .map_err(|e| JsError::new(&format!("set_var error: {e}")))
    }

    /// Enable the M-2b dev-tooling visibility override (play-from-here). When
    /// `allow` is `true`, host semantic access to `#@private` definitions —
    /// `getVar`/`setVar`, `goToPath`, `callFunction` — is permitted:
    /// enforcement is off. The studio sets this on a "play from here" session
    /// so it can start a flow at a private knot. Applies immediately and is
    /// re-applied across `restart`/`reload`. Default off. Mirrors
    /// `StoryRunner::set_dev_visibility_override`. See `docs/modules-spec.md`
    /// §4 boundary rule 3.
    #[wasm_bindgen(js_name = setDevVisibilityOverride)]
    pub fn set_dev_visibility_override(&self, allow: bool) {
        self.dev_visibility_override.set(allow);
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.story_mut().set_visibility_enforcement(!allow);
        }
    }

    /// Move the play head to a path (turn-boundary only, journaled).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen passes a JS array as an owned Vec across the boundary"
    )]
    pub fn go_to_path(&self, path: &str, args: Vec<JsValue>) -> Result<(), JsError> {
        let ink_args: Vec<Value> = args.iter().map(js_to_value).collect();
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        session
            .go_to_path(path, &ink_args)
            .map_err(|e| JsError::new(&format!("go_to_path error: {e}")))
    }

    /// Capture durable game state as a JSON `SaveState` (does not journal).
    pub fn save_state(&self) -> Result<String, JsError> {
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        serde_json::to_string(&session.save_state())
            .map_err(|e| JsError::new(&format!("save error: {e}")))
    }

    /// Load a `SaveState` JSON (turn-boundary only, journaled). Returns
    /// the `LoadReport` JSON (W14/#3307 compat honesty, RULED): a stale
    /// load's drops surface to the caller, never silently.
    pub fn load_state(&self, json: &str) -> Result<String, JsError> {
        let state: brink_runtime::SaveState =
            serde_json::from_str(json).map_err(|e| JsError::new(&format!("load error: {e}")))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let report = session
            .load_state(&state)
            .map_err(|e| JsError::new(&format!("load_state error: {e}")))?;
        serde_json::to_string(&report).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Live value editing (W16/#3309, RULED — paused-only at the panel,
    /// scalars only in v1): parse the author's input against a GLOBAL's
    /// current type and commit via `Story::set_variable` — the observed
    /// write path, so wake checks (and W18's watchpoints, when they land)
    /// see the edit. Returns `false` — no write — when the global doesn't
    /// exist, its current value isn't a scalar, or the input doesn't
    /// parse as its type (the panel's red-shake signal).
    pub fn debug_edit_global(&self, name: &str, input: &str) -> Result<bool, JsError> {
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let Some(parsed) = session
            .story()
            .variable(name)
            .and_then(|current| parse_scalar_edit(current, input))
        else {
            return Ok(false);
        };
        Ok(session.story_mut().set_variable(name, parsed))
    }

    /// Live value editing for a frame LOCAL (W16/#3309): parse the input
    /// against the temp slot's current type and commit through the
    /// `debug_set_temp` seam. `frame_idx` uses the debug snapshot's
    /// innermost-first `call_stack` ordering (the panel's own indices).
    /// Same `false`-means-refused contract as [`Self::debug_edit_global`].
    pub fn debug_edit_temp(&self, frame_idx: u32, slot: u16, input: &str) -> Result<bool, JsError> {
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let Some(parsed) = session
            .story()
            .debug_temp(frame_idx as usize, slot)
            .and_then(|current| parse_scalar_edit(current, input))
        else {
            return Ok(false);
        };
        Ok(session
            .story_mut()
            .debug_set_temp(frame_idx as usize, slot, parsed))
    }

    /// Export the session's structural transcript as JSON (RULED
    /// 2026-08-30, "Studio saves carry the structural transcript"):
    /// `OutputPart`s — `LineRef`s + slots, never resolved text — plus the
    /// fragment store and the program checksum, in the human-readable
    /// mirror of the `.brkt` content model (`transcript_json`). Pair with
    /// [`Self::render_transcript`] to re-render against whatever program
    /// is current at read time.
    pub fn export_transcript(&self) -> Result<String, JsError> {
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let story = session.story();
        let json = crate::transcript_json::export_transcript_json(
            story.transcript(),
            story.fragments(),
            story.program().source_checksum(),
        );
        serde_json::to_string(&json).map_err(|e| JsError::new(&format!("export error: {e}")))
    }

    /// Render a structural-transcript JSON (from [`Self::export_transcript`],
    /// possibly saved against an OLDER compile) against the CURRENT
    /// program and line tables. Returns a JSON array of
    /// `{ text, tags, source? }` lines — provenance included, so restored
    /// transcript rows keep their chips. Cross-compile re-render is the
    /// point (edit → reload re-renders the story-so-far); a `LineRef`
    /// whose container no longer exists is dropped rather than erroring.
    pub fn render_transcript(&self, json: &str) -> Result<String, JsError> {
        let t: crate::transcript_json::TranscriptJson =
            serde_json::from_str(json).map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let story = session.story();
        let (parts, fragments) =
            crate::transcript_json::decode_transcript_json(t, story.program().container_count());
        let lines: Vec<crate::value_marshal::DebugOutputLineJs> =
            brink_runtime::transcript::render_transcript_with_source(
                &parts,
                story.program(),
                story.line_tables(),
                None,
                &fragments,
            )
            .into_iter()
            .map(
                |(text, tags, source)| crate::value_marshal::DebugOutputLineJs {
                    text,
                    tags,
                    source: source.map(crate::value_marshal::source_location_to_js),
                },
            )
            .collect();
        serde_json::to_string(&lines).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Evaluate an ink function from the host, journaling a `Call` event. The
    /// function's own externals resolve through the isolated (non-journaling,
    /// no-op) handler — the visible story and journal window are untouched
    /// except for the top-level `Call` record.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen passes a JS array as an owned Vec across the boundary"
    )]
    pub fn call_function(&self, name: &str, args: Vec<JsValue>) -> Result<JsValue, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("call_function"))?;
        let ink_args: Vec<Value> = args.iter().map(js_to_value).collect();
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let value = session
            .call_function(name, &ink_args, &brink_runtime::FallbackHandler)
            .map_err(|e| JsError::new(&format!("call_function error: {e}")))?;
        Ok(value_to_js(&value))
    }

    // ── Snapshot / diff ───────────────────────────────────────────

    /// A typed snapshot of the current game state (globals + list membership,
    /// turn counts, callstack summary). Returns `StateSnapshot` JSON.
    pub fn snapshot(&self) -> Result<String, JsError> {
        let borrow = self.session.borrow();
        let session = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        serde_json::to_string(&session.snapshot())
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Pure diff of two `StateSnapshot` JSON values. Instance-method mirror of
    /// the standalone `diffSnapshots` export.
    pub fn diff(&self, a: &str, b: &str) -> Result<String, JsError> {
        diff_snapshots(a, b)
    }

    // ── Journal / persistence ─────────────────────────────────────

    /// The number of events currently recorded in the session journal — a
    /// cheap, synchronous dirty-signal source for the TS wrapper's
    /// deferred+debounced persistence-notification hook (#390). This method
    /// itself does no scheduling; it is polled by the JS side after each
    /// journal-mutating call to detect growth. Returns `0` if the session is
    /// not initialized.
    #[must_use]
    pub fn journal_event_count(&self) -> u32 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "journal is capped well under u32::MAX (SESSION_JOURNAL_CAP)"
        )]
        self.session
            .borrow()
            .as_ref()
            .map_or(0, |session| session.journal().len() as u32)
    }

    /// Export the session journal as JSON (the durable save artifact — embeds
    /// a fast-restore checkpoint). Persist this; `WebSession.restore` rebuilds
    /// a session from it.
    pub fn export_journal(&self) -> Result<String, JsError> {
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        serde_json::to_string(&session.export_journal())
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Rebuild a session from compiled story bytes + an exported journal.
    /// Fast-restores from the journal's embedded checkpoint when the program
    /// checksum matches; otherwise replays. A wasm constructor can only
    /// return the new instance, so the accompanying `ReplayOutcome` JSON is
    /// stashed — read it immediately after via `lastReplayOutcome`.
    #[wasm_bindgen(js_name = restore)]
    pub fn restore(
        story_bytes: &[u8],
        journal_json: &str,
        seed: Option<i32>,
        deferred: Option<Vec<String>>,
    ) -> Result<WebSession, JsError> {
        let (mut session, outcome) = Self::restore_impl(story_bytes, journal_json, seed)?;
        session.always_deferred = deferred.unwrap_or_default().into_iter().collect();
        let outcome_json = serde_json::to_string(&outcome)
            .map_err(|e| JsError::new(&format!("json error: {e}")))?;
        *session.last_replay_outcome.borrow_mut() = Some(outcome_json);
        Ok(session)
    }

    /// The `ReplayOutcome` JSON produced by the most recent `restore`/`reload`
    /// call on this session (`undefined` before either has run). Paired with
    /// `restore`/`reload` because wasm-bindgen constructors and `&mut self`
    /// methods can only return one value each.
    pub fn last_replay_outcome(&self) -> Option<String> {
        self.last_replay_outcome.borrow().clone()
    }

    /// Hot-reload: recompile-in-place against `story_bytes`, replaying the
    /// current journal against the new program. Returns `ReplayOutcome` JSON
    /// (`replayed` / `diverged` / `failed`) — the session's own state (globals,
    /// call position) reflects wherever the replay landed, even on divergence
    /// or failure (parked at the reached position, per the journal spec).
    pub fn reload(&mut self, story_bytes: &[u8]) -> Result<String, JsError> {
        let data = brink_format::read_inkb(story_bytes)
            .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let (prog, line_tables) =
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?;

        let journal = {
            let mut borrow = self.session.borrow_mut();
            let session = borrow
                .as_mut()
                .ok_or_else(|| JsError::new("session not initialized"))?;
            session.journal().clone()
        };

        *self.session.borrow_mut() = None;
        self.program = Arc::new(prog);
        self.base_line_tables.clone_from(&line_tables);
        self.data = data;

        let mut story =
            brink_runtime::Story::<FastRng>::new(Arc::clone(&self.program), line_tables);
        if let Some(s) = self.seed.get() {
            story.set_rng_seed(s);
        }
        // A dev session stays a dev session across hot-reload (play-from-here).
        if self.dev_visibility_override.get() {
            story.set_visibility_enforcement(false);
        }
        let (session, outcome) = brink_runtime::StorySession::replay(
            story,
            &journal,
            brink_runtime::ExternalReplayMode::Recorded,
            None,
        );
        *self.session.borrow_mut() = Some(session);
        self.busy.set(false);

        let outcome_json = serde_json::to_string(&outcome)
            .map_err(|e| JsError::new(&format!("json error: {e}")))?;
        *self.last_replay_outcome.borrow_mut() = Some(outcome_json.clone());
        Ok(outcome_json)
    }

    /// Resume a replay parked on a deferred external (live-mode replay only —
    /// recorded-mode replay never parks). Resolve the pending external first
    /// via `resolveExternal`, then call this. With no parked replay tail this
    /// steps the live story to its next pause instead. Returns `ReplayOutcome`
    /// JSON.
    pub fn continue_replay(&mut self) -> Result<String, JsError> {
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        let outcome = session.continue_replay(None);
        serde_json::to_string(&outcome).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Restart: create a fresh session from the same program (new empty
    /// journal), re-applying the host-set seed if any.
    pub fn restart(&mut self) {
        let mut story = brink_runtime::Story::<FastRng>::new(
            Arc::clone(&self.program),
            self.base_line_tables.clone(),
        );
        if let Some(s) = self.seed.get() {
            story.set_rng_seed(s);
        }
        // A dev session stays a dev session across restart (play-from-here).
        if self.dev_visibility_override.get() {
            story.set_visibility_enforcement(false);
        }
        let seed64 = self.seed.get().map(seed_to_journal_u64);
        *self.session.borrow_mut() = Some(brink_runtime::StorySession::new(story, seed64));
        self.busy.set(false);
        *self.last_replay_outcome.borrow_mut() = None;
    }
}

impl WebSession {
    /// Shared decode/link/restore-or-replay path for the `restore` constructor.
    fn restore_impl(
        story_bytes: &[u8],
        journal_json: &str,
        seed: Option<i32>,
    ) -> Result<(WebSession, brink_runtime::ReplayOutcome), JsError> {
        let data = brink_format::read_inkb(story_bytes)
            .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let (prog, line_tables) =
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?;
        let journal: brink_runtime::SessionJournal = serde_json::from_str(journal_json)
            .map_err(|e| JsError::new(&format!("journal decode error: {e}")))?;
        let program = Arc::new(prog);

        let mut story =
            brink_runtime::Story::<FastRng>::new(Arc::clone(&program), line_tables.clone());
        if let Some(s) = seed {
            story.set_rng_seed(s);
        }
        let (session, outcome) = brink_runtime::StorySession::restore(story, journal)
            .map_err(|e| JsError::new(&format!("restore error: {e}")))?;

        Ok((
            WebSession {
                program,
                base_line_tables: line_tables,
                data,
                session: RefCell::new(Some(session)),
                seed: Cell::new(seed),
                always_deferred: std::collections::BTreeSet::new(),
                busy: Cell::new(false),
                last_replay_outcome: RefCell::new(None),
                dev_visibility_override: Cell::new(false),
                breakpoints: RefCell::new(brink_runtime::BreakpointSet::new()),
                watched_globals: RefCell::new(Vec::new()),
            },
            outcome,
        ))
    }
}

/// Widen a host-set `i32` RNG seed to the journal's advisory `Option<u64>`
/// metadata field, preserving the bit pattern (not the numeric sign) so a
/// negative seed round-trips exactly rather than lossily reinterpreting sign.
/// Parse an author-typed edit against a value's CURRENT type (W16/#3309
/// value editing — scalars only in v1, RULED). `None` = refused: the
/// current value isn't a scalar, or the input doesn't parse as its type.
/// The edit can never change a value's type — an int slot only accepts an
/// int, a float slot a float (integral input included: `3` → `3.0`) — so
/// typed-mode invariants hold across an edit. String input strips one
/// layer of matching double quotes (the panel displays strings quoted, and
/// the seeded input carries them back).
fn parse_scalar_edit(current: &Value, input: &str) -> Option<Value> {
    let trimmed = input.trim();
    match current {
        Value::Int(_) => trimmed.parse::<i32>().ok().map(Value::Int),
        Value::Float(_) => trimmed
            .parse::<f32>()
            .ok()
            .filter(|f| f.is_finite())
            .map(Value::Float),
        Value::Bool(_) => match trimmed {
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            _ => None,
        },
        Value::String(_) => {
            let unquoted = trimmed
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(input);
            Some(Value::String(Arc::from(unquoted)))
        }
        _ => None,
    }
}

fn seed_to_journal_u64(seed: i32) -> u64 {
    u64::from(seed.cast_unsigned())
}

/// [`ExternalFnHandler`] that forces every name in `always_deferred` to park
/// (`Pending`) regardless of an available ink fallback body, and otherwise
/// falls through — `WebSession` registers no JS bindings of its own, so every
/// other external resolves via the ink fallback (or errors if none exists).
struct AlwaysDeferHandler<'a> {
    always_deferred: &'a std::collections::BTreeSet<String>,
}

impl ExternalFnHandler for AlwaysDeferHandler<'_> {
    fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
        if self.always_deferred.contains(name) {
            ExternalResult::Pending
        } else {
            ExternalResult::Fallback
        }
    }
}

/// Standalone pure diff of two `StateSnapshot` JSON values (mirrors
/// `brink_runtime::diff`, exposed for callers holding two exported snapshots
/// without a live `WebSession`, e.g. comparing two save slots).
#[wasm_bindgen(js_name = diffSnapshots)]
pub fn diff_snapshots(a: &str, b: &str) -> Result<String, JsError> {
    let a: brink_runtime::StateSnapshot = serde_json::from_str(a)
        .map_err(|e| JsError::new(&format!("snapshot decode error: {e}")))?;
    let b: brink_runtime::StateSnapshot = serde_json::from_str(b)
        .map_err(|e| JsError::new(&format!("snapshot decode error: {e}")))?;
    let d = brink_runtime::diff(&a, &b);
    serde_json::to_string(&d).map_err(|e| JsError::new(&format!("json error: {e}")))
}

/// `StepOutcome` JSON mirror — the wire-format fix (#387): splits
/// `awaiting_external` OUT of the `Line` union (unlike `StoryRunner`'s
/// `LineJs`) into its own tagged shape sharing only the `type` discriminant
/// convention.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StepOutcomeJs {
    Line {
        line: LineJs,
    },
    AwaitingExternal {
        deferred: bool,
        name: Option<String>,
    },
}

fn step_outcome_to_js<R: brink_runtime::StoryRng>(
    outcome: brink_runtime::StepOutcome,
    session: &brink_runtime::StorySession<R>,
) -> StepOutcomeJs {
    match outcome {
        brink_runtime::StepOutcome::Step(step) => StepOutcomeJs::Line {
            line: line_to_js(step),
        },
        brink_runtime::StepOutcome::AwaitingExternal => StepOutcomeJs::AwaitingExternal {
            // `WebSession` never registers an internal promise-in-flight park
            // (it has no JS-binding registry) — every `awaiting_external` it
            // surfaces is the deferred, out-of-band kind the host resolves via
            // `resolveExternal`. `deferred` is kept explicit (rather than
            // implied) so the TS discriminant stays self-describing and
            // future promise-in-flight support doesn't need a shape break.
            deferred: true,
            name: session.story().pending_external_name().map(str::to_owned),
        },
    }
}

// ── WebSession::resolve_debug_position tests (D9, #3187) ─────────────
//
// Host-target, unlike `websession_wasm_tests` below: `resolve_debug_position`
// takes/returns only plain Rust types, never a `JsValue` parameter, so it is
// exercised directly over a real `WebSession` — the actual session type
// `LocalSessionProvider` (`packages/studio-store`) drives — on the host
// target.
#[cfg(test)]
mod resolve_debug_position_tests {
    use super::WebSession;

    fn debug_bytes(src: &str) -> Vec<u8> {
        use std::collections::BTreeMap;

        use brink_environment::{OptionOverrides, Project};
        use brink_source_tree::InMemory;

        let mut files = BTreeMap::new();
        files.insert("main.ink".to_string(), src.to_string());
        let tree = InMemory::new(files);
        let overrides = OptionOverrides {
            debug_info: true,
            ..Default::default()
        };
        let env = Project::load(&tree, "main.ink", &overrides).expect("Project::load");
        let out = brink_environment::compile(&env).expect("compile");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        bytes
    }

    #[test]
    fn resolves_the_active_flows_position_to_source() {
        let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        let snap_json = session.debug_snapshot().expect("debug_snapshot");
        let snap: serde_json::Value = serde_json::from_str(&snap_json).expect("valid JSON");
        let pos = snap["position"]
            .as_object()
            .expect("position present at flow entry");
        let container_idx =
            u32::try_from(pos["container_idx"].as_u64().expect("container_idx")).expect("fits u32");
        let offset = u32::try_from(pos["offset"].as_u64().expect("offset")).expect("fits u32");

        let resolved_json = session
            .resolve_debug_position(container_idx, offset)
            .expect("resolve_debug_position serializes");
        let resolved: serde_json::Value = serde_json::from_str(&resolved_json).expect("valid JSON");
        assert_eq!(resolved["file"], serde_json::json!("main.ink"));
    }

    #[test]
    fn returns_null_json_without_debug_info() {
        let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
        let out = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
            .expect("test source compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        let session = WebSession::new(&bytes, None, None).expect("session constructs");

        let resolved_json = session
            .resolve_debug_position(0, 0)
            .expect("resolve_debug_position serializes even without debug info");
        assert_eq!(resolved_json, "null");
    }
}

// ── WebSession debug control tests (D8, #3186 — control-half bridge #3232) ──
//
// Host-target for the same reason as `resolve_debug_position_tests` above:
// `debugRun`/`debugStep`/`debugBreakpoint*` take/return only plain Rust
// types, never `JsValue`, so this exercises the real `WebSession` API —
// `LocalSessionProvider`'s actual backing — over the production entry
// points, per CLAUDE.md's "Rust-level tests over a real consumer" rule.
#[cfg(test)]
mod debug_control_tests {
    use super::WebSession;

    /// Compile with `--debug-info` on, same fixture shape as
    /// `resolve_debug_position_tests::debug_bytes`.
    fn debug_bytes(src: &str) -> Vec<u8> {
        use std::collections::BTreeMap;

        use brink_environment::{OptionOverrides, Project};
        use brink_source_tree::InMemory;

        let mut files = BTreeMap::new();
        files.insert("main.ink".to_string(), src.to_string());
        let tree = InMemory::new(files);
        let overrides = OptionOverrides {
            debug_info: true,
            ..Default::default()
        };
        let env = Project::load(&tree, "main.ink", &overrides).expect("Project::load");
        let out = brink_environment::compile(&env).expect("compile");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        bytes
    }

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("valid JSON")
    }

    /// Append an outcome's non-empty emitted lines to a test transcript.
    fn push_lines(transcript: &mut Vec<String>, outcome: &serde_json::Value) {
        for line in outcome["lines"].as_array().expect("lines array") {
            let text = line["text"].as_str().expect("text").trim_end();
            if !text.is_empty() {
                transcript.push(text.to_owned());
            }
        }
    }

    // ── W5/#3298: the interleaved play↔debug proof the ruling requires ──
    //
    // Play a line normally, hit a breakpoint via debugRun, step across the
    // code line, run to the choice point, choose (journaled), and run to
    // the end — asserting the TRANSCRIPT stays coherent: every text line
    // arrives exactly once, in story order, whichever loop produced it.
    // This is the wasm-level half; the provider's routing (which verb runs
    // when) is pinned in `packages/brink-studio`'s vitest suite.
    #[test]
    fn interleaved_play_and_debug_keep_one_coherent_transcript() {
        let src = "VAR gold = 12\n                   -> tavern\n                   === tavern ===\n                   The tavern is loud tonight.\n                   ~ gold = gold - 2\n                   Ale ordered.\n                   * [Order another] -> more\n                   * [Leave] -> END\n                   === more ===\n                   Another round.\n                   -> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");
        let mut transcript: Vec<String> = Vec::new();

        // 1. Ordinary play: the first prose line arrives on the journaled
        // road. (The production line-buffered loop runs AHEAD of what it
        // delivers — by now "Ale ordered." is already materialized in the
        // buffer, undelivered. That lookahead is exactly why the debug
        // verbs drain the shared delivery cursor.)
        let first = json(&session.continue_single().expect("continue_single"));
        let first_text = first["text"].as_str().expect("text").trim_end().to_owned();
        transcript.push(first_text.clone());
        assert_eq!(first_text, "The tavern is loud tonight.");

        // 2. Arm a breakpoint on `Another round.` (0-based line 9) through
        // the same source→program road the gutter uses (W2).
        let addr = json(
            &session
                .resolve_source_line("main.ink", 9)
                .expect("resolves"),
        );
        let bp = session.debug_breakpoint_add(
            u32::try_from(addr["container_idx"].as_u64().expect("idx")).expect("u32"),
            u32::try_from(addr["offset"].as_u64().expect("offset")).expect("u32"),
            Some("more-entry".to_owned()),
        );

        // 3. Free-run to the choice point: the drained delivery cursor
        // surfaces the looked-ahead "Ale ordered." exactly once, here.
        let to_choices = json(&session.debug_run(None).expect("debug_run"));
        assert_eq!(to_choices["reason"]["type"], serde_json::json!("choices"));
        push_lines(&mut transcript, &to_choices);
        assert_eq!(
            transcript,
            vec![
                "The tavern is loud tonight.".to_owned(),
                "Ale ordered.".to_owned(),
            ],
        );

        // 4. Choose on the JOURNALED road (choices stay journaled across
        // the unification — that is what keeps restore/replay coherent),
        // then free-run: halts at the breakpoint BEFORE the target knot's
        // text emits.
        session.choose(0).expect("choose");
        let run = json(&session.debug_run(None).expect("debug_run"));
        assert_eq!(run["reason"]["type"], serde_json::json!("breakpoint"));
        assert_eq!(run["reason"]["id"], serde_json::json!(bp));
        push_lines(&mut transcript, &run);
        assert!(
            !transcript.iter().any(|l| l == "Another round."),
            "stopping AT the breakpoint must not have emitted the line past it"
        );

        // 5. Line-step across it: the emitted text surfaces through the
        // outcome's lines delta — the coherence hole W5 closes.
        let mut steps = 0;
        while !transcript.iter().any(|l| l == "Another round.") {
            let step = json(
                &session
                    .debug_step_line("over", None)
                    .expect("debug_step_line"),
            );
            push_lines(&mut transcript, &step);
            steps += 1;
            assert!(steps < 8, "line-stepping never surfaced the text line");
        }

        // 6. Free-run to the end.
        let to_end = json(&session.debug_run(None).expect("debug_run"));
        assert_eq!(to_end["reason"]["type"], serde_json::json!("terminal"));
        push_lines(&mut transcript, &to_end);

        // 7. Coherence: every text line exactly once, in story order,
        // whichever loop produced it.
        assert_eq!(
            transcript,
            vec![
                "The tavern is loud tonight.".to_owned(),
                "Ale ordered.".to_owned(),
                "Another round.".to_owned(),
            ],
            "the interleaved loops must produce one coherent transcript"
        );
    }

    // A line step that LANDS on an armed breakpoint must claim the hit
    // (found live in the studio: the reveal stopped exactly at the armed
    // address with reason "step", the resume's past-entry rule then
    // skipped it, and the breakpoint could never fire under the unified
    // line-stepping reveal).
    #[test]
    fn a_line_step_landing_on_a_breakpoint_reports_the_breakpoint() {
        let src = "-> top\n=== top ===\nFirst line.\nSecond line.\nThird line.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        // Arm on `Second line.` (0-based 3), then line-step from the start.
        let addr = json(
            &session
                .resolve_source_line("main.ink", 3)
                .expect("resolves"),
        );
        let bp = session.debug_breakpoint_add(
            u32::try_from(addr["container_idx"].as_u64().expect("idx")).expect("u32"),
            u32::try_from(addr["offset"].as_u64().expect("offset")).expect("u32"),
            None,
        );

        let mut hit = false;
        for _ in 0..6 {
            let step = json(
                &session
                    .debug_step_line("over", None)
                    .expect("debug_step_line"),
            );
            if step["reason"]["type"] == serde_json::json!("breakpoint") {
                assert_eq!(step["reason"]["id"], serde_json::json!(bp));
                assert_eq!(
                    step["position"],
                    serde_json::json!({
                        "container_idx": addr["container_idx"],
                        "offset": addr["offset"],
                    }),
                    "the hit must be AT the armed address"
                );
                hit = true;
                break;
            }
            assert_ne!(
                step["reason"]["type"],
                serde_json::json!("terminal"),
                "stepped to the end without the breakpoint ever claiming a stop"
            );
        }
        assert!(hit, "line-stepping across an armed address must report it");
    }

    // The drain consumes the SAME cursor the journaled road delivers from
    // — a line surfaced by a debug advance must never be re-delivered by a
    // later `continue_single` (the double-line half of the coherence bug).
    #[test]
    fn debug_drained_lines_are_never_redelivered_by_the_journaled_road() {
        let src = "-> top\n=== top ===\nFirst line.\nSecond line.\nThird line.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        let first = json(&session.continue_single().expect("continue_single"));
        assert_eq!(first["text"], serde_json::json!("First line.\n"));

        // Debug steps drain whatever the lookahead has COMMITTED — a text
        // line commits only once the next non-whitespace output begins (or
        // a yield), so it can surface one advance later than it was
        // materialized. Step until it does.
        let mut drained: Vec<String> = Vec::new();
        for _ in 0..4 {
            let step = json(
                &session
                    .debug_step_line("over", None)
                    .expect("debug_step_line"),
            );
            drained.extend(
                step["lines"]
                    .as_array()
                    .expect("lines")
                    .iter()
                    .map(|l| l["text"].as_str().expect("text").trim_end().to_owned())
                    .filter(|t| !t.is_empty()),
            );
            if drained.contains(&"Second line.".to_owned()) {
                break;
            }
        }
        assert!(
            drained.contains(&"Second line.".to_owned()),
            "the looked-ahead line must surface through the debug outcomes: {drained:?}"
        );
        assert_eq!(
            drained.iter().filter(|t| *t == "Second line.").count(),
            1,
            "and exactly once"
        );

        // The journaled road resumes AFTER everything the drain delivered —
        // never re-delivering a drained line.
        let next = json(&session.continue_single().expect("continue_single"));
        let next_text = next["text"].as_str().unwrap_or("").trim_end();
        assert_ne!(
            next_text, "Second line.",
            "a drained line must not be re-delivered by the journaled road"
        );
    }

    // ── 2026-08-30 Continue ruling: run to the next content line ────────
    //
    // The verb behind the Player's Continue: each call delivers EXACTLY
    // one content line in its own outcome — the stop lands past the
    // glue/commit boundary, so there is no one-advance delivery lag
    // (#3321's felt half, fixed for this tier; the statement-tier steps
    // above still exhibit it, which stays open on #3321).
    #[test]
    fn debug_run_to_line_delivers_each_content_line_at_its_own_stop() {
        let src = "-> top\n=== top ===\nFirst line.\nSecond line.\nThird line.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        for expected in ["First line.", "Second line."] {
            let stop = json(&session.debug_run_to_line(None).expect("debug_run_to_line"));
            assert_eq!(
                stop["reason"]["type"],
                serde_json::json!("step"),
                "{expected}"
            );
            let mut lines = Vec::new();
            push_lines(&mut lines, &stop);
            assert_eq!(
                lines,
                vec![expected.to_owned()],
                "one line per stop, AT the stop"
            );
        }

        // The last line commits via the flush at the terminal yield — it
        // rides the terminal stop, exactly as on the production road.
        let end = json(&session.debug_run_to_line(None).expect("debug_run_to_line"));
        assert_eq!(end["reason"]["type"], serde_json::json!("terminal"));
        let mut lines = Vec::new();
        push_lines(&mut lines, &end);
        assert_eq!(lines, vec!["Third line.".to_owned()]);
    }

    // An armed breakpoint bounds the run-to-line advance — it stops AT
    // the breakpoint, before the line past it emits.
    #[test]
    fn debug_run_to_line_is_bounded_by_breakpoints() {
        let src = "-> top\n=== top ===\nFirst line.\nSecond line.\nThird line.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        let addr = json(
            &session
                .resolve_source_line("main.ink", 3)
                .expect("resolves"),
        );
        let bp = session.debug_breakpoint_add(
            u32::try_from(addr["container_idx"].as_u64().expect("idx")).expect("u32"),
            u32::try_from(addr["offset"].as_u64().expect("offset")).expect("u32"),
            None,
        );

        let stop = json(&session.debug_run_to_line(None).expect("debug_run_to_line"));
        assert_eq!(stop["reason"]["type"], serde_json::json!("breakpoint"));
        assert_eq!(stop["reason"]["id"], serde_json::json!(bp));
        let mut lines = Vec::new();
        push_lines(&mut lines, &stop);
        assert!(
            !lines.contains(&"Second line.".to_owned()),
            "the breakpoint stop must not run past the armed line: {lines:?}"
        );
    }

    // Continue after ordinary play: the line the production lookahead
    // materialized (but had not delivered) surfaces exactly once, at the
    // stop it commits on — here the choices yield, so one Continue click
    // delivers "Ale ordered." WITH the choice point (the line before a
    // choice belongs to its presentation, same as the production flush) —
    // and the journaled road never re-delivers it. (The wrapper's
    // already-committed short-circuit additionally caps a stop at one
    // content segment when a prior advance left a committed line in the
    // cursor — reachable through drain interleavings, not constructible
    // from cold ordinary play, which is why THIS pin drives the
    // commit-at-yield shape instead.)
    #[test]
    fn debug_run_to_line_after_ordinary_play_delivers_the_pending_line_once() {
        let src = "VAR gold = 12\n                   -> tavern\n                   === tavern ===\n                   The tavern is loud tonight.\n                   ~ gold = gold - 2\n                   Ale ordered.\n                   * [Order another] -> END\n                   * [Leave] -> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        let first = json(&session.continue_single().expect("continue_single"));
        assert_eq!(
            first["text"],
            serde_json::json!("The tavern is loud tonight.\n")
        );

        let stop = json(&session.debug_run_to_line(None).expect("debug_run_to_line"));
        assert_eq!(stop["reason"]["type"], serde_json::json!("choices"));
        let mut lines = Vec::new();
        push_lines(&mut lines, &stop);
        assert_eq!(lines, vec!["Ale ordered.".to_owned()]);

        // Choose on the journaled road; nothing is re-delivered.
        session.choose(1).expect("choose");
        let next = json(&session.continue_single().expect("continue_single"));
        assert_ne!(
            next["text"].as_str().unwrap_or("").trim_end(),
            "Ale ordered.",
            "a debug-delivered line must not come back on the journaled road"
        );
    }

    /// Two-file INCLUDE fixture for the provenance tests — the shape the
    /// studio playground actually runs (`main.ink` INCLUDE-ing the story).
    fn debug_bytes_two_files(main: &str, other: &str) -> Vec<u8> {
        use std::collections::BTreeMap;

        use brink_environment::{OptionOverrides, Project};
        use brink_source_tree::InMemory;

        let mut files = BTreeMap::new();
        files.insert("main.ink".to_string(), main.to_string());
        files.insert("other.ink".to_string(), other.to_string());
        let tree = InMemory::new(files);
        let overrides = OptionOverrides {
            debug_info: true,
            ..Default::default()
        };
        let env = Project::load(&tree, "main.ink", &overrides).expect("Project::load");
        let out = brink_environment::compile(&env).expect("compile");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        bytes
    }

    // An INCLUDE-d file's lines must carry ranges in THEIR OWN file's
    // coordinates — found live in the playground (its story is INCLUDE-d
    // from main.ink): every provenance chip pointed hundreds of lines past
    // the truth because the range was in include-expanded coordinates
    // while the file name was the include target's.
    #[test]
    fn included_file_lines_carry_ranges_in_their_own_file() {
        let main = "INCLUDE other.ink\n-> top\n";
        let other = "=== top ===\nPadding one.\nPadding two.\nThe included line.\n-> END\n";
        let session = WebSession::new(&debug_bytes_two_files(main, other), None, None)
            .expect("session constructs");

        let mut found = false;
        for _ in 0..6 {
            let line = json(&session.continue_single().expect("continue_single"));
            if line["text"]
                .as_str()
                .unwrap_or("")
                .contains("The included line.")
            {
                let source = &line["source"];
                assert_eq!(source["file"], serde_json::json!("other.ink"), "{line}");
                let start =
                    usize::try_from(source["range_start"].as_u64().expect("start")).expect("usize");
                let end =
                    usize::try_from(source["range_end"].as_u64().expect("end")).expect("usize");
                assert!(
                    end <= other.len() && other[start..end].contains("The included line."),
                    "range {start}..{end} must be in other.ink's own coordinates \
                     (file is {} bytes)",
                    other.len()
                );
                found = true;
                break;
            }
            if line["type"] == serde_json::json!("end") {
                break;
            }
        }
        assert!(found, "the included line never arrived");
    }

    // ── W7/#3300: transcript provenance ─────────────────────────────────
    //
    // Every delivered text line carries its `source` — the line-table
    // entry's source location — on BOTH drive roads, so the Player can
    // make each transcript row a provenance handle (hover file:line,
    // ⌘-click reveals).
    #[test]
    fn delivered_lines_carry_source_provenance_on_both_roads() {
        let src = "-> top\n=== top ===\nFirst line.\nSecond line.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        // Journaled road: continue_single's Line carries `source`.
        let first = json(&session.continue_single().expect("continue_single"));
        let source = &first["source"];
        assert_eq!(source["file"], serde_json::json!("main.ink"), "{first}");
        let start = source["range_start"].as_u64().expect("range_start");
        let end = source["range_end"].as_u64().expect("range_end");
        let text_at =
            &src[usize::try_from(start).expect("usize")..usize::try_from(end).expect("usize")];
        assert!(
            text_at.contains("First line."),
            "the range must cover the line's own source text, got {text_at:?}"
        );

        // Debug road: a drained line carries the same field.
        let stop = json(&session.debug_run_to_line(None).expect("debug_run_to_line"));
        let lines = stop["lines"].as_array().expect("lines");
        let second = lines
            .iter()
            .find(|l| l["text"].as_str().unwrap_or("").contains("Second line."))
            .expect("the crossed line is in the outcome");
        assert_eq!(second["source"]["file"], serde_json::json!("main.ink"));
        let s2 = second["source"]["range_start"].as_u64().expect("start");
        let e2 = second["source"]["range_end"].as_u64().expect("end");
        assert!(
            src[usize::try_from(s2).expect("usize")..usize::try_from(e2).expect("usize")]
                .contains("Second line."),
            "debug-road provenance must cover the line's source too"
        );
    }

    // ── W16/#3309: live value editing ───────────────────────────────────
    //
    // The issue's proof items: an edit is visible when the story
    // continues; a type mismatch is refused with NO write; a frame
    // local's edit shows in the snapshot and in later output. Parsing is
    // typed against the CURRENT value — an edit can never change a
    // value's type.
    #[test]
    fn value_edit_global_reflects_in_the_continued_story() {
        // The interpolating line sits BEHIND a choice: at a line stop the
        // VM has usually already buffered the next line (slots captured at
        // push time — the #3321 commit-lag reality), so a choice point is
        // the honest stop where an edit provably reaches the next read.
        let src = "VAR gold = 2\n-> top\n=== top ===\nFirst.\n* [Pay] -> pay\n=== pay ===\nYou have {gold} coin.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");
        let stop = json(&session.debug_run(None).expect("run"));
        assert_eq!(
            stop["reason"]["type"],
            serde_json::json!("choices"),
            "stopped at the choice point: {stop}"
        );

        // Refusals first — none of these may write.
        assert!(!session.debug_edit_global("gold", "abc").expect("call ok"));
        assert!(!session.debug_edit_global("gold", "3.5").expect("call ok"));
        assert!(!session.debug_edit_global("missing", "1").expect("call ok"));

        // The real edit commits…
        assert!(session.debug_edit_global("gold", "12").expect("call ok"));
        let snap = json(&session.debug_snapshot().expect("snapshot"));
        let gold = snap["globals"]
            .as_array()
            .expect("globals")
            .iter()
            .find(|g| g["name"] == serde_json::json!("gold"))
            .expect("gold present");
        assert_eq!(gold["value"], serde_json::json!("12"), "{snap}");

        // …and the story past the choice reads it.
        session.choose(0).expect("choose");
        let out = json(&session.debug_run(None).expect("continue"));
        assert!(
            out["lines"]
                .as_array()
                .expect("lines")
                .iter()
                .any(|l| l["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("You have 12 coin.")),
            "the edit must reach the story's own read: {out}"
        );
    }

    #[test]
    fn value_edit_temp_lands_in_the_frame_and_the_story() {
        // A temp edit needs a STATEMENT stop (`debug_step`), not a choice
        // stop: pending choices carry captured thread snapshots, and
        // choosing restores that thread — call stack, temps and all — so
        // an edit made at the choice point is overwritten by the capture
        // (measured here first). Statement stepping continues the SAME
        // thread, and the next line's push hasn't executed yet.
        let src = "-> top\n=== top ===\n~ temp bonus = 3\nFiller one.\nYou gain {bonus} points.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        // Step statements until `bonus` is live in the innermost frame.
        let mut slot: Option<u16> = None;
        for _ in 0..12 {
            session.debug_step("over", None).expect("step");
            let snap = json(&session.debug_snapshot().expect("snapshot"));
            let found = snap["call_stack"][0]["locals"].as_array().and_then(|ls| {
                ls.iter()
                    .find(|l| l["name"] == serde_json::json!("bonus"))
                    .cloned()
            });
            if let Some(local) = found {
                assert_eq!(local["value"]["value"], serde_json::json!(3), "{snap}");
                slot =
                    Some(u16::try_from(local["slot"].as_u64().expect("slot")).expect("fits u16"));
                break;
            }
        }
        let slot = slot.expect("bonus became live within the step budget");

        // Type mismatch refused; the honest edit lands; a frame that
        // doesn't exist refuses rather than writing anywhere.
        assert!(!session.debug_edit_temp(0, slot, "true").expect("call ok"));
        assert!(session.debug_edit_temp(0, slot, "12").expect("call ok"));
        assert!(!session.debug_edit_temp(9, slot, "12").expect("call ok"));

        let snap2 = json(&session.debug_snapshot().expect("snapshot"));
        let edited = snap2["call_stack"][0]["locals"]
            .as_array()
            .expect("locals")
            .iter()
            .find(|l| l["name"] == serde_json::json!("bonus"))
            .expect("bonus still live")["value"]["value"]
            .clone();
        assert_eq!(edited, serde_json::json!(12), "{snap2}");

        let out = json(&session.debug_run(None).expect("continue"));
        assert!(
            out["lines"]
                .as_array()
                .expect("lines")
                .iter()
                .any(|l| l["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("You gain 12 points.")),
            "the temp edit must reach the story's own read: {out}"
        );
    }

    // ── W18/#3311: break-on-write data breakpoints ──────────────────────
    //
    // The issue's proof item: a write to a watched global pauses at the
    // writing line with the watchpoint NAMED in the stop reason — on the
    // Continue tier (`debug_run_to_line`), the road ordinary Player
    // advances take, not just the raw run verb.
    #[test]
    fn watched_write_stops_the_continue_tier_with_the_name() {
        let src = "VAR gold = 2\n-> top\n=== top ===\nFirst.\n~ gold = 9\nSecond.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");
        assert!(session.debug_watchpoint_add("gold").expect("arm"));
        // Unknown global refuses; duplicate refuses.
        assert!(!session.debug_watchpoint_add("nope").expect("call ok"));
        assert!(!session.debug_watchpoint_add("gold").expect("call ok"));
        assert_eq!(
            session.debug_watchpoints().expect("list"),
            r#"["gold"]"#,
            "arm order, by name"
        );

        // The FIRST Continue already stops at the watchpoint: the VM runs
        // ahead of delivery (the #3321 commit boundary), so `~ gold = 9`
        // executes while "First." is still buffered — the stop lands at
        // the writing instruction, named, before ANY later content.
        let stop = json(&session.debug_run_to_line(None).expect("continue"));
        assert_eq!(
            stop["reason"]["type"],
            serde_json::json!("watchpoint"),
            "{stop}"
        );
        assert_eq!(stop["reason"]["name"], serde_json::json!("gold"), "{stop}");
        assert!(
            !stop["lines"]
                .as_array()
                .expect("lines")
                .iter()
                .any(|l| l["text"].as_str().unwrap_or("").contains("Second.")),
            "the stop lands BEFORE later content commits: {stop}"
        );

        // Disarm → the same road runs through freely; every line arrives
        // exactly once (the buffered "First." was not eaten by the stop).
        assert!(session.debug_watchpoint_remove("gold"));
        let mut texts: Vec<String> = Vec::new();
        for _ in 0..6 {
            let out = json(&session.debug_run_to_line(None).expect("continue"));
            for l in out["lines"].as_array().expect("lines") {
                if let Some(t) = l["text"].as_str() {
                    texts.push(t.to_owned());
                }
            }
            if out["reason"]["type"] == serde_json::json!("terminal") {
                break;
            }
        }
        let firsts = texts.iter().filter(|t| t.contains("First.")).count();
        let seconds = texts.iter().filter(|t| t.contains("Second.")).count();
        assert_eq!(
            (firsts, seconds),
            (1, 1),
            "each line exactly once: {texts:?}"
        );
    }

    #[test]
    fn watched_write_stops_the_run_road_too() {
        let src = "VAR hp = 3\n-> top\n=== top ===\nA line.\n~ hp = 1\nMore.\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");
        assert!(session.debug_watchpoint_add("hp").expect("arm"));
        let stop = json(&session.debug_run(None).expect("run"));
        assert_eq!(
            stop["reason"]["type"],
            serde_json::json!("watchpoint"),
            "{stop}"
        );
        assert_eq!(stop["reason"]["name"], serde_json::json!("hp"), "{stop}");
    }

    #[test]
    fn watched_write_in_a_choice_body_stops_the_post_choose_continue() {
        let src = "VAR torch = 6\n-> top\n=== top ===\nPick.\n* [Room]\n  ~ torch = torch - 1\n  Dark room.\n- -> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");
        assert!(session.debug_watchpoint_add("torch").expect("arm"));
        // Run to the choice point on the watched road.
        let stop = json(&session.debug_run(None).expect("run"));
        assert_eq!(
            stop["reason"]["type"],
            serde_json::json!("choices"),
            "{stop}"
        );
        // Choose (the production road — selection only, no VM steps), then
        // Continue: the body's write must stop the watched advance.
        session.choose(0).expect("choose");
        let out = json(&session.debug_run_to_line(None).expect("continue"));
        assert_eq!(
            out["reason"]["type"],
            serde_json::json!("watchpoint"),
            "the choice body's write must stop the post-choose continue: {out}"
        );
        assert_eq!(out["reason"]["name"], serde_json::json!("torch"), "{out}");
    }

    // ── Structural transcript (RULED 2026-08-30) ────────────────────────
    //
    // A save's transcript is `OutputPart`s, not resolved text: exported as
    // JSON from one session and re-rendered on ANOTHER session whose
    // program was compiled from EDITED source, the restored lines must
    // carry the NEW text — the whole point of the format. Provenance
    // survives the round trip; a container index the edited program no
    // longer has is dropped, never a panic.
    #[test]
    fn structural_transcript_rerenders_against_an_edited_program() {
        let src_a = "-> top\n=== top ===\nThe lantern flickers.\nA door creaks.\n-> END\n";
        // Same structure, edited prose — indices line up, text differs.
        let src_b = "-> top\n=== top ===\nThe lantern GUTTERS OUT.\nA door creaks.\n-> END\n";

        let session_a =
            WebSession::new(&debug_bytes(src_a), None, None).expect("session constructs");
        loop {
            let line = json(&session_a.continue_single().expect("continue"));
            if line["type"] == serde_json::json!("end") {
                break;
            }
        }
        let exported = session_a.export_transcript().expect("export");
        let envelope = json(&exported);
        assert_eq!(envelope["version"], serde_json::json!(1));
        assert!(
            envelope["parts"]
                .as_array()
                .expect("parts array")
                .iter()
                .any(|p| p["part"] == serde_json::json!("line")),
            "the export must be structural (LineRefs), not resolved text: {envelope}"
        );

        // Fresh session on the EDITED compile; render the old transcript.
        let session_b =
            WebSession::new(&debug_bytes(src_b), None, None).expect("session constructs");
        let rendered = json(&session_b.render_transcript(&exported).expect("render"));
        let lines = rendered.as_array().expect("array of lines");
        let texts: Vec<&str> = lines.iter().filter_map(|l| l["text"].as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("GUTTERS OUT")),
            "re-render must show the EDITED text, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("flickers")),
            "the save-time prose must NOT survive a re-render, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("A door creaks.")),
            "untouched lines re-render unchanged, got {texts:?}"
        );
        // Provenance survives — and points into the CURRENT source.
        let first = lines
            .iter()
            .find(|l| l["text"].as_str().unwrap_or("").contains("GUTTERS"))
            .expect("edited line present");
        let s = first["source"]["range_start"].as_u64().expect("start");
        let e = first["source"]["range_end"].as_u64().expect("end");
        assert!(
            src_b[usize::try_from(s).expect("usize")..usize::try_from(e).expect("usize")]
                .contains("GUTTERS OUT"),
            "provenance must cover the edited line in the NEW source"
        );
    }

    #[test]
    fn structural_transcript_render_drops_out_of_range_containers() {
        let src_big = "-> a\n=== a ===\nAlpha.\n-> b\n=== b ===\nBeta.\n-> END\n";
        let src_small = "-> a\n=== a ===\nAlpha.\n-> END\n";

        let session_a =
            WebSession::new(&debug_bytes(src_big), None, None).expect("session constructs");
        loop {
            let line = json(&session_a.continue_single().expect("continue"));
            if line["type"] == serde_json::json!("end") {
                break;
            }
        }
        let exported = session_a.export_transcript().expect("export");

        let session_b =
            WebSession::new(&debug_bytes(src_small), None, None).expect("session constructs");
        // Must not panic; whatever containers still exist render.
        let rendered = json(
            &session_b
                .render_transcript(&exported)
                .expect("render survives"),
        );
        assert!(rendered.is_array(), "renders to a line array: {rendered}");
    }

    // ── W14/#3307: the LoadReport reaches the session road ──────────────
    //
    // A clean load reports all-zeros; loading a save whose anonymous
    // choice-body id no longer exists in the (edited) program reports the
    // drop — surfaced, never silent (the ruled compat honesty). The
    // session layer used to DISCARD the runtime's report entirely.
    #[test]
    fn load_state_reports_clean_and_stale_loads_through_the_session() {
        let src_a = "VAR gold = 2\n-> shop\n=== shop ===\nWelcome.\n* [Cheap thing] -> shop\n+ [Browse] -> shop\n";
        // Every choice is gone → the anonymous body id has nowhere to land
        // (a same-position survivor could otherwise inherit the id — the
        // positional half of the identity cluster).
        let src_b = "VAR gold = 2\n-> shop\n=== shop ===\nWelcome.\n-> END\n";

        let session_a =
            WebSession::new(&debug_bytes(src_a), None, None).expect("session constructs");
        for _ in 0..3 {
            let line: serde_json::Value =
                serde_json::from_str(&session_a.continue_single().expect("continue"))
                    .expect("JSON");
            if line["type"] == serde_json::json!("choices") {
                break;
            }
        }
        session_a.choose(0).expect("choose the once-only");
        for _ in 0..3 {
            let line: serde_json::Value =
                serde_json::from_str(&session_a.continue_single().expect("continue"))
                    .expect("JSON");
            if line["type"] == serde_json::json!("choices") {
                break;
            }
        }
        let save = session_a.save_state().expect("save_state");

        // Clean: load back into a fresh session on the SAME program.
        let fresh = WebSession::new(&debug_bytes(src_a), None, None).expect("constructs");
        let clean: serde_json::Value =
            serde_json::from_str(&fresh.load_state(&save).expect("load")).expect("JSON");
        assert_eq!(
            clean["anonymous_states_dropped"],
            serde_json::json!(0),
            "{clean}"
        );
        assert_eq!(clean["unknown_globals"], serde_json::json!([]));
        // The durable state round-trips: the loaded session's snapshot
        // carries the saved turn index.
        let snap_saved: serde_json::Value =
            serde_json::from_str(&session_a.debug_snapshot().expect("snap")).expect("JSON");
        let snap_loaded: serde_json::Value =
            serde_json::from_str(&fresh.debug_snapshot().expect("snap")).expect("JSON");
        assert_eq!(snap_loaded["turn_index"], snap_saved["turn_index"]);
        assert_eq!(snap_loaded["globals"], snap_saved["globals"]);

        // Stale: the edited program can't place the anonymous visit.
        let edited = WebSession::new(&debug_bytes(src_b), None, None).expect("constructs");
        let stale: serde_json::Value =
            serde_json::from_str(&edited.load_state(&save).expect("load")).expect("JSON");
        assert!(
            stale["anonymous_states_dropped"].as_u64().unwrap_or(0) >= 1,
            "the dropped anonymous visit must be REPORTED: {stale}"
        );
    }

    #[test]
    fn breakpoint_add_remove_list_roundtrip() {
        let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        let id = session.debug_breakpoint_add(0, 0, Some("entry".to_owned()));
        let list = json(&session.debug_breakpoints().expect("debug_breakpoints"));
        let entries = list.as_array().expect("array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["id"], serde_json::json!(id));
        assert_eq!(entries[0]["name"], serde_json::json!("entry"));
        assert_eq!(entries[0]["enabled"], serde_json::json!(true));

        assert!(session.debug_breakpoint_set_enabled(id, false));
        let list = json(&session.debug_breakpoints().expect("debug_breakpoints"));
        assert_eq!(
            list.as_array().expect("array")[0]["enabled"],
            serde_json::json!(false)
        );

        assert!(session.debug_breakpoint_remove(id));
        let list = json(&session.debug_breakpoints().expect("debug_breakpoints"));
        assert!(list.as_array().expect("array").is_empty());
        // A second remove of the same (now-gone) id reports false, not an error.
        assert!(!session.debug_breakpoint_remove(id));
    }

    #[test]
    fn debug_step_into_reports_step_and_advances_depth_or_position() {
        let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");

        let before = json(&session.debug_snapshot().expect("debug_snapshot"));
        let outcome = json(
            &session
                .debug_step("into", None)
                .expect("debug_step serializes"),
        );
        // A single opcode-level step at the very start of a short story can
        // only be `Step` (nothing to break on yet, no choice reached, and
        // `-> END` is several instructions away) — see `StepMode::Into`'s
        // own doc ("execute exactly one instruction").
        assert_eq!(outcome["reason"]["type"], serde_json::json!("step"));
        let after = json(&session.debug_snapshot().expect("debug_snapshot"));
        // Forward progress happened — position/turn state differs from entry,
        // whichever the underlying instruction touched.
        assert_ne!(before["position"], after["position"]);
    }

    #[test]
    fn debug_run_stops_at_a_breakpoint_reached_after_the_first_step() {
        let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
        // Discover a real mid-flow position by stepping once on a scratch
        // session, then arm a fresh session's breakpoint there and confirm
        // `debugRun` halts on it before running to `-> END`.
        let scratch =
            WebSession::new(&debug_bytes(src), None, None).expect("scratch session constructs");
        let stepped = json(
            &scratch
                .debug_step("into", None)
                .expect("debug_step serializes"),
        );
        let pos = stepped["position"]
            .as_object()
            .expect("debug_step reports a position after one instruction");
        let container_idx =
            u32::try_from(pos["container_idx"].as_u64().expect("container_idx")).expect("fits u32");
        let offset = u32::try_from(pos["offset"].as_u64().expect("offset")).expect("fits u32");

        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");
        let id = session.debug_breakpoint_add(container_idx, offset, None);
        let outcome = json(&session.debug_run(None).expect("debug_run serializes"));
        assert_eq!(outcome["reason"]["type"], serde_json::json!("breakpoint"));
        assert_eq!(outcome["reason"]["id"], serde_json::json!(id));
        let stopped_pos = outcome["position"].as_object().expect("position present");
        assert_eq!(
            stopped_pos["container_idx"],
            serde_json::json!(container_idx)
        );
        assert_eq!(stopped_pos["offset"], serde_json::json!(offset));
    }

    #[test]
    fn debug_run_without_breakpoints_reaches_terminal() {
        let src = "hello\n-> END\n";
        let session = WebSession::new(&debug_bytes(src), None, None).expect("session constructs");
        let outcome = json(&session.debug_run(None).expect("debug_run serializes"));
        assert_eq!(outcome["reason"]["type"], serde_json::json!("terminal"));
    }
}

// ── WebSession wasm tests (#387) ───────────────────────────────────────
//
// Exercises the Story Session wasm bindings end-to-end through the real
// exported `WebSession` API. Gated to wasm32 (like `binding_wasm_tests`
// above): `WebSession`'s methods take `&JsValue`/`Vec<JsValue>`, and
// constructing a `JsValue` (e.g. `JsValue::from_f64`) panics off wasm32 even
// on success paths, not just the `JsError` rejection paths.
#[cfg(all(test, target_arch = "wasm32"))]
mod websession_wasm_tests {
    use super::WebSession;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_test::wasm_bindgen_test;

    fn session_bytes(src: &str) -> Vec<u8> {
        let out = brink_compiler::compile("main.ink", |_path| Ok(src.to_owned()))
            .expect("test source compiles");
        let mut b = Vec::new();
        brink_format::write_inkb(&out.data, &mut b);
        b
    }

    fn new_session(src: &str) -> WebSession {
        WebSession::new(&session_bytes(src), None, None).expect("session constructs")
    }

    #[wasm_bindgen_test]
    fn advance_splits_step_outcome_from_line() {
        // A single line immediately followed by `-> END` is now two
        // `advance` steps — a `"text"` `Line` carrying the content and its
        // `block_id`, then a payload-free terminal `"end"` `Line` — this
        // asserts the `StepOutcomeJs` envelope (`{ "type": "line", "line":
        // Line }`), not any particular `Line` variant.
        let s = new_session("Hello world.\n-> END\n");

        let json = s.advance().expect("advance succeeds");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "line", "{json}");
        assert_eq!(v["line"]["type"], "text", "{json}");
        assert_eq!(v["line"]["text"], "Hello world.\n", "{json}");
        assert!(v["line"]["block_id"].is_u64(), "{json}");
        // `element` (issue #1683): this fixture has no `@[convention(...,
        // attach = ...)]` handler in play, so the line reports the
        // degenerate narrative case with an empty data map (issue #2108
        // populates `data` only downstream of an attach handler) — guards
        // the marshal leg that a deleted `element: Some(ElementJs { .. })`
        // arm in `value_marshal.rs::line_to_js` would otherwise leave
        // untested.
        assert_eq!(v["line"]["element"]["kind"], "narrative", "{json}");
        assert_eq!(
            v["line"]["element"]["data"],
            serde_json::json!({}),
            "{json}"
        );

        let json = s.advance().expect("advance succeeds");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "line", "{json}");
        assert_eq!(v["line"]["type"], "end", "{json}");
        assert_eq!(v["line"]["text"], "", "{json}");
        assert!(v["line"]["block_id"].is_null(), "{json}");
        assert!(v["line"]["element"].is_null(), "{json}");
    }

    #[wasm_bindgen_test]
    fn deferred_external_always_parks_as_awaiting_external() {
        // A fallback body exists, but `deferred` forces an out-of-band park
        // anyway — proving `deferred: string[]` overrides fallback fallthrough.
        let bytes = session_bytes(
            "EXTERNAL ask(x)\nA{ask(1)}B\n-> END\n\
             === function ask(x) ===\n~ return 99\n",
        );
        let s = WebSession::new(&bytes, None, Some(vec!["ask".to_owned()]))
            .expect("session constructs");
        let json = s.advance().expect("advance succeeds");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "awaiting_external", "{json}");
        assert_eq!(v["deferred"], true, "{json}");
        assert_eq!(v["name"], "ask", "{json}");
        assert!(s.has_pending_external());
    }

    #[wasm_bindgen_test]
    fn resolve_external_unparks_and_journals() {
        let bytes = session_bytes(
            "EXTERNAL ask(x)\nA{ask(1)}B\n-> END\n\
             === function ask(x) ===\n~ return 99\n",
        );
        let s = WebSession::new(&bytes, None, Some(vec!["ask".to_owned()]))
            .expect("session constructs");
        s.advance().expect("parks on ask");
        s.resolve_external(&JsValue::from_f64(7.0));
        assert!(!s.has_pending_external());
        let json = s.continue_to_pause().expect("drains to end");
        assert!(json.contains("A7B"), "{json}");

        let journal = s.export_journal().expect("journal exports");
        let jv: serde_json::Value = serde_json::from_str(&journal).unwrap();
        // An integral-valued JS number (7.0) marshals to an ink `Int`, not a
        // `Float` (see `js_to_value`).
        let has_external_event = jv["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"]["type"] == "external" && e["kind"]["result"]["Int"] == 7);
        assert!(has_external_event, "{journal}");
    }

    #[wasm_bindgen_test]
    fn did_safe_exit_distinguishes_explicit_done_from_ran_out_of_content() {
        // Issue #1573: both cases deliver a `done`-type `Line`; `didSafeExit`
        // is the production-reachable way to tell them apart without an
        // extra `continueSingle` call. The terminal split means the `Hello.`
        // text and the payload-free `done` now arrive as two separate steps.
        let safe = new_session("Hello.\n-> DONE\n");
        let json = safe.continue_single().expect("first line");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "text", "{json}");
        let json = safe.continue_single().expect("second line");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "done", "{json}");
        assert!(safe.did_safe_exit());

        let unsafe_ = new_session("-> k\n== k ==\nHello.\n");
        let json = unsafe_.continue_single().expect("first line");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "text", "{json}");
        let json = unsafe_.continue_single().expect("second line");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "done", "{json}");
        assert!(!unsafe_.did_safe_exit());
    }

    #[wasm_bindgen_test]
    fn choose_and_continue_to_pause() {
        let s = new_session(
            "-> start\n=== start ===\n\
             * choice one\n    One.\n-> END\n\
             * choice two\n    Two.\n-> END\n",
        );
        let json = s.continue_to_pause().expect("reaches choices");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let last = v.as_array().unwrap().last().unwrap();
        assert_eq!(last["type"], "choices", "{json}");

        s.choose(0).expect("choose succeeds");
        let json = s.continue_to_pause().expect("drains to end");
        assert!(json.contains("One."), "{json}");
    }

    #[wasm_bindgen_test]
    fn set_var_turn_boundary_and_save_load_round_trip() {
        let s = new_session("VAR hp = 10\nHP {hp}.\n-> END\n");
        // Fresh, not-started session is at a boundary: set_var succeeds.
        assert!(
            s.set_var("hp", &JsValue::from_f64(3.0))
                .expect("boundary set_var ok")
        );
        let json = s.continue_to_pause().expect("drains");
        assert!(json.contains("HP 3."), "{json}");

        let saved = s.save_state().expect("save_state serializes");
        let s2 = new_session("VAR hp = 10\nHP {hp}.\n-> END\n");
        s2.load_state(&saved).expect("load_state applies");
        let snap = s2.snapshot().expect("snapshot serializes");
        assert!(snap.contains("\"hp\":{\"Int\":3}"), "{snap}");
    }

    #[wasm_bindgen_test]
    fn set_var_mid_turn_is_rejected() {
        let s = new_session("VAR hp = 10\nHP {hp}.\n-> more\n=== more ===\nMore.\n-> END\n");
        s.continue_single().expect("first line");
        // Mid-turn (more content pending): set_var must be rejected, not queued.
        assert!(s.set_var("hp", &JsValue::from_f64(3.0)).is_err());
    }

    #[wasm_bindgen_test]
    fn snapshot_diff_and_standalone_diff_snapshots_agree() {
        let s = new_session("VAR hp = 10\nHP {hp}.\n-> END\n");
        let before = s.snapshot().expect("snapshot 1");
        s.set_var("hp", &JsValue::from_f64(5.0))
            .expect("set_var ok");
        let after = s.snapshot().expect("snapshot 2");

        let via_method = s.diff(&before, &after).expect("instance diff");
        let via_standalone = super::diff_snapshots(&before, &after).expect("standalone diff");
        assert_eq!(via_method, via_standalone);
        let v: serde_json::Value = serde_json::from_str(&via_method).unwrap();
        assert_eq!(
            v["changed_globals"]["hp"],
            serde_json::json!([{"Int": 10}, {"Int": 5}]),
            "{via_method}"
        );
    }

    #[wasm_bindgen_test]
    fn restart_resets_journal_and_state() {
        let mut s = new_session("VAR hp = 10\nHP {hp}.\n-> END\n");
        s.continue_to_pause().expect("drains");
        s.restart();
        let json = s.export_journal().expect("journal exports");
        let jv: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(jv["events"], serde_json::json!([]), "{json}");
    }

    #[wasm_bindgen_test]
    fn export_and_restore_round_trip() {
        let src = "VAR hp = 10\nHP {hp}.\n-> END\n";
        let bytes = session_bytes(src);
        let s = WebSession::new(&bytes, None, None).expect("session constructs");
        s.continue_to_pause().expect("drains to end");
        let journal = s.export_journal().expect("journal exports");

        let restored = WebSession::restore(&bytes, &journal, None, None).expect("restore succeeds");
        let outcome = restored
            .last_replay_outcome()
            .expect("restore records an outcome");
        let ov: serde_json::Value = serde_json::from_str(&outcome).unwrap();
        assert_eq!(ov["type"], "replayed", "{outcome}");

        let snap = restored.snapshot().expect("snapshot serializes");
        assert!(snap.contains("\"hp\":{\"Int\":10}"), "{snap}");
    }

    #[wasm_bindgen_test]
    fn reload_replays_journal_and_returns_outcome() {
        let src = "VAR hp = 10\nHP {hp}.\n-> more\n=== more ===\nMore.\n-> END\n";
        let bytes = session_bytes(src);
        let mut s = WebSession::new(&bytes, None, None).expect("session constructs");
        s.continue_to_pause().expect("drains to end");

        // Recompile the identical source (a real hot-reload would change it;
        // here we only assert the replay-outcome plumbing round-trips).
        let outcome_json = s.reload(&bytes).expect("reload succeeds");
        let ov: serde_json::Value = serde_json::from_str(&outcome_json).unwrap();
        assert_eq!(ov["type"], "replayed", "{outcome_json}");
        assert_eq!(
            s.last_replay_outcome().as_deref(),
            Some(outcome_json.as_str()),
            "lastReplayOutcome mirrors reload's own return"
        );
    }

    #[wasm_bindgen_test]
    fn call_function_journals_without_mutating_visible_story() {
        let s = new_session(
            "VAR hp = 10\nHP {hp}.\n-> END\n\
             === function bonus(x) ===\n~ return x + 1\n",
        );
        let result = s
            .call_function("bonus", vec![JsValue::from_f64(4.0)])
            .expect("call_function succeeds");
        assert_eq!(result.as_f64(), Some(5.0));

        let json = s.export_journal().expect("journal exports");
        let jv: serde_json::Value = serde_json::from_str(&json).unwrap();
        let has_call_event = jv["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"]["type"] == "call" && e["kind"]["name"] == "bonus");
        assert!(has_call_event, "{json}");

        // The visible story is untouched: it hasn't even started yet.
        let text = s.continue_to_pause().expect("drains");
        assert!(text.contains("HP 10."), "{text}");
    }

    #[wasm_bindgen_test]
    fn debug_snapshot_reflects_live_position() {
        // Unlike `program_model`/`program_inkt` (compile-bound, captured once),
        // `debug_snapshot` must track wherever the session currently is — the
        // studio's live-inspector needs a fresh position after every advance.
        let s = new_session("VAR hp = 10\nHP {hp}.\n-> more\n=== more ===\nMore.\n-> END\n");
        let before = s.debug_snapshot().expect("debug_snapshot serializes");
        s.continue_single().expect("advance past the first line");
        let after = s.debug_snapshot().expect("debug_snapshot serializes again");
        assert_ne!(before, after, "position must move: {before} == {after}");

        // #3182: the whole-string inequality above doesn't prove `position`
        // itself crossed the wasm JSON boundary — it would pass just as
        // happily if `debug_snapshot_to_js` dropped `position:` entirely and
        // only `current_location`/`call_stack` moved. Parse both and assert
        // directly on the `position` field.
        let before_v: serde_json::Value = serde_json::from_str(&before).unwrap();
        let after_v: serde_json::Value = serde_json::from_str(&after).unwrap();
        let before_pos = &before_v["position"];
        let after_pos = &after_v["position"];
        assert!(
            before_pos["container_idx"].is_u64() && before_pos["offset"].is_u64(),
            "position.container_idx/offset must be present on the wire: {before}"
        );
        assert!(
            after_pos["container_idx"].is_u64() && after_pos["offset"].is_u64(),
            "position.container_idx/offset must be present on the wire: {after}"
        );
        assert_ne!(
            before_pos, after_pos,
            "position itself must change across continue_single: {before_pos} == {after_pos}"
        );
    }

    #[wasm_bindgen_test]
    fn program_model_and_inkt_are_static_for_the_loaded_program() {
        let s = new_session("VAR hp = 10\nHP {hp}.\n-> END\n");
        let model = s.program_model().expect("program_model serializes");
        let mv: serde_json::Value = serde_json::from_str(&model).unwrap();
        assert!(
            mv["globals"]
                .as_array()
                .unwrap()
                .iter()
                .any(|g| g["name"] == "hp"),
            "{model}"
        );
        let inkt = s.program_inkt().expect("program_inkt renders");
        assert!(inkt.contains("hp"), "{inkt}");
    }

    #[wasm_bindgen_test]
    fn shared_flow_writes_are_visible_to_the_driving_session() {
        // The session's own flow shares globals with whatever `advance`/
        // `continue_single` drives — same story instance, reached through the
        // documented `story()`/`story_mut()` escape hatch.
        let s = new_session("VAR x = 0\n{x}\n-> END\n=== bump ===\n~ x = 9\nbumped.\n-> END\n");
        s.spawn_flow("f", Some("bump".to_owned())).expect("spawn");
        let flow_line = s.continue_flow("f").expect("continue flow");
        assert!(flow_line.contains("bumped"), "{flow_line}");

        assert!(
            s.flow_names().expect("flow_names").contains("\"f\""),
            "flow should be listed before destroy"
        );

        let default_line = s.continue_single().expect("default flow line");
        assert!(
            default_line.contains('9'),
            "default flow sees the shared flow's write; got {default_line}"
        );

        s.destroy_flow("f").expect("destroy");
        assert!(
            !s.flow_names()
                .expect("flow_names after destroy")
                .contains("\"f\"")
        );
    }

    #[wasm_bindgen_test]
    fn continue_flow_maximally_returns_every_line_to_the_terminal() {
        // Per the `Step`/`OutputLine` contract (CLAUDE.md "Runtime public
        // API" / docs/execution-model): terminal variants carry no payload
        // of their own — any text produced before the boundary already
        // arrived as its own preceding `"text"` `Line`. So driving
        // "One.\nTwo.\n" to its `-> END` yields three `Line`s: a `Text` for
        // "One.", a `Text` for "Two.", and a payload-free terminal `End` —
        // matching native `Story::continue_flow_maximally_shared`/
        // `drive_to_terminal` exactly (verified directly against the native
        // API for this same story/flow before writing this assertion).
        let s = new_session("-> END\n=== bump ===\nOne.\nTwo.\n-> END\n");
        s.spawn_flow("f", Some("bump".to_owned())).expect("spawn");
        let json = s
            .continue_flow_maximally("f")
            .expect("drives the flow to its terminal line");
        let lines: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = lines.as_array().expect("array of Line");
        assert_eq!(arr.len(), 3, "{json}"); // "One.", "Two.", then the payload-free terminal `end`
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[0]["text"], "One.\n");
        assert_eq!(arr[1]["type"], "text");
        assert_eq!(arr[1]["text"], "Two.\n");
        assert_eq!(arr[2]["type"], "end");
        assert_eq!(arr[2]["text"], "");
        let full_text: String = arr
            .iter()
            .map(|l| l["text"].as_str().expect("text field"))
            .collect();
        assert_eq!(full_text, "One.\nTwo.\n");
    }

    /// #999: a shared flow that emits text forever must error at the wasm
    /// leg's `continue_flow_maximally` binding — the same
    /// `RuntimeError::LineLimitExceeded` bound `continue_story` enforces for
    /// the primary flow — rather than the caller looping `continue_flow`
    /// client-side without a cap and growing an array/hanging on an
    /// infinite-emitting story.
    #[wasm_bindgen_test]
    fn continue_flow_maximally_errors_at_line_limit_on_infinite_emitting_flow() {
        let s = new_session("-> spam\n\n=== spam ===\nLine.\n-> spam\n");
        s.spawn_flow("f", None).expect("spawn");
        let err = s
            .continue_flow_maximally("f")
            .expect_err("infinite-emitting flow should error, not hang or grow unbounded");
        let js_err: JsValue = err.into();
        let message: String = js_err.unchecked_into::<js_sys::Error>().message().into();
        assert!(
            message.contains("line limit exceeded"),
            "error shape should match `continue_story`'s `LineLimitExceeded`; got: {message}"
        );
    }

    #[wasm_bindgen_test]
    fn flow_debug_snapshot_and_choose_flow() {
        let s = new_session(
            "Root.\n-> END\n\
             === start ===\n\
             * choice one\n    One.\n-> END\n\
             * choice two\n    Two.\n-> END\n",
        );
        s.spawn_flow("f", Some("start".to_owned())).expect("spawn");
        let line = s.continue_flow("f").expect("continue flow");
        assert!(line.contains("\"type\":\"choices\""), "{line}");
        s.choose_flow("f", 0).expect("choose in flow");
        // Selecting the choice first echoes its own label text, then the
        // content beneath it — same two-line shape `continue_single` produces
        // for the default flow at a visible-text choice.
        let echoed = s.continue_flow("f").expect("continue after choice");
        assert!(echoed.contains("choice one"), "{echoed}");
        let after = s.continue_flow("f").expect("continue to choice body");
        assert!(after.contains("One."), "{after}");

        let snap = s.flow_debug_snapshot("f").expect("flow_debug_snapshot");
        assert!(snap.contains("\"status\""), "{snap}");

        // #3182: frame-level assertion — the innermost call frame for a
        // flow that's mid-body (past its choice, inside "One.") must carry
        // a real `(container_idx, offset)` position on the wire, not just
        // the snapshot as a whole containing a `"status"` key.
        let snap_v: serde_json::Value = serde_json::from_str(&snap).unwrap();
        let frame = &snap_v["call_stack"][0];
        assert!(
            frame["position"]["container_idx"].is_u64() && frame["position"]["offset"].is_u64(),
            "innermost call_stack frame must carry position.container_idx/offset: {snap}"
        );
    }
}
