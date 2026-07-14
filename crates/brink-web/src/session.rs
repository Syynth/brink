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

    /// Load a `SaveState` JSON (turn-boundary only, journaled).
    pub fn load_state(&self, json: &str) -> Result<(), JsError> {
        let state: brink_runtime::SaveState =
            serde_json::from_str(json).map_err(|e| JsError::new(&format!("load error: {e}")))?;
        let mut borrow = self.session.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("session not initialized"))?;
        session
            .load_state(&state)
            .map_err(|e| JsError::new(&format!("load_state error: {e}")))
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
            },
            outcome,
        ))
    }
}

/// Widen a host-set `i32` RNG seed to the journal's advisory `Option<u64>`
/// metadata field, preserving the bit pattern (not the numeric sign) so a
/// negative seed round-trips exactly rather than lossily reinterpreting sign.
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
        brink_runtime::StepOutcome::Line(line) => StepOutcomeJs::Line {
            line: line_to_js(line),
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
    use wasm_bindgen::JsValue;
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
        // A single line immediately followed by `-> END` resolves as one
        // `advance` step whose `Line` is the terminal `end` variant (not a
        // separate `text` step) — this asserts the `StepOutcomeJs` envelope
        // (`{ "type": "line", "line": Line }`), not any particular `Line`
        // variant.
        let s = new_session("Hello world.\n-> END\n");
        let json = s.advance().expect("advance succeeds");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "line", "{json}");
        assert_eq!(v["line"]["type"], "end", "{json}");
        assert_eq!(v["line"]["text"], "Hello world.\n", "{json}");
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
    }
}
