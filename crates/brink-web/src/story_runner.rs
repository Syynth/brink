use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use brink_format::Value;
use brink_runtime::{FastRng, ReplayRecorder};
use wasm_bindgen::prelude::*;

use crate::external_binding::{BusyGuard, JsHandler, RecordingReplayHandler, reentrant_error};
use crate::program_model;
use crate::speculation::{SpeculateOptionsJs, WebSpeculation};
use crate::value_marshal::{LineJs, debug_snapshot_to_js, js_to_value, line_to_js, value_to_js};

// ── Runtime ─────────────────────────────────────────────────────────

/// A running story instance. Owns Program + Story to avoid lifetime issues in wasm.
#[wasm_bindgen]
pub struct StoryRunner {
    // Shared ownership: Story holds its own `Arc::clone`, so there is no
    // self-referential borrow to work around (no pinning, no unsafe).
    program: Arc<brink_runtime::Program>,
    base_line_tables: Vec<Vec<brink_format::LineEntry>>,
    /// The decoded `StoryData`, retained for `program_inkt` (Program Explorer).
    /// Independent owned value — `link` only borrows it.
    data: brink_format::StoryData,
    story: RefCell<Option<brink_runtime::Story<FastRng>>>,
    /// Synchronous ink→engine external bindings: ink `EXTERNAL` name → JS
    /// callback `(args) => value`. Built into a [`JsHandler`] on each
    /// `continue_*` call. Single-threaded wasm, so a `RefCell` suffices.
    bindings: RefCell<HashMap<String, js_sys::Function>>,
    /// When `true`, an external with no registered binding resolves to `null`
    /// (no-op) instead of falling through to its ink fallback body / erroring.
    /// Lets shipped content call host verbs a given build doesn't know without
    /// dead-ending. Default `false` (strict — current behavior).
    lenient_unbound: Cell<bool>,
    /// Explicit RNG seed, if the host set one. Re-applied on `reset` so re-runs
    /// stay deterministic. `None` leaves the runtime default (0).
    seed: Cell<Option<i32>>,
    /// The `Promise` returned by an async binding the flow is parked on (the
    /// `bindExternal` callback returned a thenable). Taken by the JS wrapper via
    /// [`take_pending_promise`](Self::take_pending_promise), awaited, then fed
    /// back through [`resolve_external`](Self::resolve_external). One slot —
    /// single flow parks on at most one external.
    pending_promise: RefCell<Option<js_sys::Promise>>,
    /// Reentrancy guard: set while a VM-stepping method is running, so a binding
    /// callback (or a second `continueAsync`) that re-enters gets a clean error
    /// + `console.warn` instead of a `RefCell` double-borrow panic.
    busy: Cell<bool>,
    /// Records every external resolved during live playback so a hot-reload
    /// ([`reload`](Self::reload)) can replay them faithfully — query-gated
    /// branches reproduce and effect bindings don't re-fire. Lives Rust-side
    /// across reloads (the program is swapped in place, this is kept); never
    /// crosses the wasm boundary. See `docs/replay-recording-spec.md`.
    recorder: RefCell<ReplayRecorder>,
    /// While `true`, visible playback (`continue_*`/`advance_one`) serves
    /// externals from `recorder` instead of invoking JS bindings — used by the
    /// studio to silently re-walk the recorded choice log after a reload.
    /// Bracketed by [`begin_replay`](Self::begin_replay) /
    /// [`end_replay`](Self::end_replay).
    replaying: Cell<bool>,
    /// The M-2b dev-tooling visibility override (play-from-here). When `true`,
    /// host semantic access to `#@private` definitions is **allowed** — the
    /// story runs with visibility enforcement off. Default `false`: production
    /// hosts respect visibility. Re-applied across `reset`/`reload` like the
    /// seed so a dev session stays a dev session. See `docs/modules-spec.md`
    /// §4 boundary rule 3.
    dev_visibility_override: Cell<bool>,
}

#[wasm_bindgen]
impl StoryRunner {
    /// Create a new story runner from compiled story bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(story_bytes: &[u8]) -> Result<StoryRunner, JsError> {
        let data = brink_format::read_inkb(story_bytes)
            .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let (prog, line_tables) =
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?;
        let program = Arc::new(prog);

        let story = brink_runtime::Story::<FastRng>::new(Arc::clone(&program), line_tables.clone());

        Ok(StoryRunner {
            program,
            base_line_tables: line_tables,
            data,
            story: RefCell::new(Some(story)),
            bindings: RefCell::new(HashMap::new()),
            lenient_unbound: Cell::new(false),
            seed: Cell::new(None),
            pending_promise: RefCell::new(None),
            busy: Cell::new(false),
            recorder: RefCell::new(ReplayRecorder::new()),
            replaying: Cell::new(false),
            dev_visibility_override: Cell::new(false),
        })
    }

    /// The compiled program rendered as `.inkt` text for the Program Explorer:
    /// checksum, name table, globals, lists, externals, address paths, and
    /// containers with bytecode disassembly. Static for the loaded program.
    pub fn program_inkt(&self) -> Result<String, JsError> {
        let mut out = String::new();
        brink_format::write_inkt(&self.data, &mut out)
            .map_err(|e| JsError::new(&format!("inkt error: {e}")))?;
        Ok(out)
    }

    /// Structured model of the compiled program for the Program Explorer:
    /// globals / lists / externals tables plus a knot/stitch tree with
    /// per-knot, name-resolved bytecode disassembly. Returns JSON.
    pub fn program_model(&self) -> Result<String, JsError> {
        let model = program_model::build(&self.data);
        serde_json::to_string(&model).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The compiler's line table, project-wide (`INCLUDE`s already resolved
    /// by the compile), for host-side analysis (#366) — cast detection,
    /// per-speaker word counts, the #362 line-fit metrics epic. Every entry
    /// carries its text (plain or a slot/select template) and, when known,
    /// its source span (`file` + byte `range_start`/`range_end` in that
    /// file). Reuses the exact `LinesJson` shape the `export-xliff` CLI path
    /// produces — same project-wide, includes-resolved compiled-line table,
    /// already built for translator consumption; this just exposes it to
    /// hosts. Returns JSON (`brink_intl::LinesJson`). Static for the loaded
    /// program (does not require a running `Story`).
    pub fn lines_table(&self) -> Result<String, JsError> {
        let lines = brink_intl::export_lines(&self.data, self.data.source_checksum);
        serde_json::to_string(&lines).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// The source-identity checksum of the currently loaded program, formatted
    /// `0x{:08x}` — identical to `program_checksum(story_bytes)`, but read
    /// directly off the already-decoded program (survives `reload`). Used by
    /// Tier-1 speculative-eval fragment caching (F5.1,
    /// `docs/speculative-eval-spec.md`) to key a compiled fragment to the
    /// program version it was compiled against.
    pub fn checksum(&self) -> String {
        format!("0x{:08x}", self.data.source_checksum)
    }

    /// Register a synchronous external-function binding: when the story calls
    /// `EXTERNAL <name>(...)`, `f` is invoked with the call arguments and its
    /// return value is fed back to the story. Arguments arrive as native JS
    /// values (number / boolean / string / null); the return is read back the
    /// same way (an integer-valued number becomes an ink int, otherwise a
    /// float).
    ///
    /// Re-registering the same name replaces the previous binding. A binding
    /// that throws resolves to `null` (a thrown host callback is a bug; the
    /// exception is not propagated into the VM).
    pub fn bind_external(&self, name: &str, f: js_sys::Function) {
        self.bindings.borrow_mut().insert(name.to_owned(), f);
    }

    /// Remove a previously registered external binding.
    pub fn unbind_external(&self, name: &str) {
        self.bindings.borrow_mut().remove(name);
    }

    /// Control how an external with **no** registered binding resolves.
    /// `false` (default): fall through to the ink fallback body, erroring if
    /// none exists. `true`: resolve to `null` (no-op), so content can call
    /// host verbs this build doesn't know without dead-ending.
    pub fn set_lenient_unbound(&self, lenient: bool) {
        self.lenient_unbound.set(lenient);
    }

    /// The current lenient-unbound setting (see `set_lenient_unbound`). Used
    /// by Tier-1 speculative-eval fragment evaluation (F5.1) to match a
    /// scratch runner's policy to this one's before running a fragment.
    pub fn lenient_unbound(&self) -> bool {
        self.lenient_unbound.get()
    }

    /// The names of every currently registered external-function binding,
    /// sorted. Used by Tier-1 speculative-eval fragment evaluation (F5.1) to
    /// copy this runner's live bindings onto a scratch runner built from a
    /// recompiled program (`compile_fragment`), so a query/effect external
    /// the fragment touches resolves the same way it would here — a fresh
    /// `StoryRunner` otherwise starts with none.
    pub fn binding_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.bindings.borrow().keys().cloned().collect();
        names.sort();
        names
    }

    /// The JS callback registered for `name`, if any. See `binding_names`.
    pub fn get_binding(&self, name: &str) -> Option<js_sys::Function> {
        self.bindings.borrow().get(name).cloned()
    }

    /// Read a global ink variable by name as a native JS value. Returns
    /// `undefined` if no such variable is declared, `null` if it exists and
    /// holds null. (Non-scalar globals — lists — currently read as `null`.)
    pub fn get_var(&self, name: &str) -> JsValue {
        let borrow = self.story.borrow();
        match borrow.as_ref().and_then(|s| s.variable(name)) {
            Some(v) => value_to_js(v),
            None => JsValue::UNDEFINED,
        }
    }

    /// Set a global ink variable by name from a native JS value
    /// (number / boolean / string / null). Returns `false` if no such
    /// variable is declared.
    pub fn set_var(&self, name: &str, value: &JsValue) -> bool {
        let v = js_to_value(value);
        let mut borrow = self.story.borrow_mut();
        borrow.as_mut().is_some_and(|s| s.set_variable(name, v))
    }

    /// Set the RNG seed, making `RANDOM`/shuffle output reproducible. Applies
    /// immediately and is re-applied across [`reset`](Self::reset) so re-runs
    /// stay deterministic. Set before the first `continue_*` for a fully
    /// deterministic playthrough.
    pub fn set_seed(&self, seed: i32) {
        self.seed.set(Some(seed));
        if let Some(story) = self.story.borrow_mut().as_mut() {
            story.set_rng_seed(seed);
        }
    }

    /// Enable the M-2b dev-tooling visibility override (play-from-here). When
    /// `allow` is `true`, host semantic access to `#@private` definitions —
    /// `getVar`/`setVar`, `goToPath`/`goToPathWithArgs`, `callFunction` — is
    /// permitted: the story runs with visibility enforcement **off**. Editors
    /// and debug hosts set this so they can start flows at private knots and
    /// inspect private state; production hosts leave it `false` (the default)
    /// to respect visibility. Applies immediately and persists across
    /// `reset`/`reload`. This is a host capability, not a language switch —
    /// the compiled program is identical either way. See `docs/modules-spec.md`
    /// §4 boundary rule 3.
    #[wasm_bindgen(js_name = setDevVisibilityOverride)]
    pub fn set_dev_visibility_override(&self, allow: bool) {
        self.dev_visibility_override.set(allow);
        if let Some(story) = self.story.borrow_mut().as_mut() {
            story.set_visibility_enforcement(!allow);
        }
    }

    /// Whether the dev-tooling visibility override is currently on.
    #[wasm_bindgen(js_name = devVisibilityOverride)]
    pub fn dev_visibility_override(&self) -> bool {
        self.dev_visibility_override.get()
    }

    /// Capture the durable, name-keyed game state as a JSON string (globals,
    /// visit/turn counts, turn index, RNG). Human-inspectable; store as-is in
    /// localStorage / a save slot. Use [`save_bytes`](Self::save_bytes) for a
    /// compact release blob. Does not capture execution position.
    pub fn save(&self) -> Result<String, JsError> {
        let borrow = self.story.borrow();
        let story = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        serde_json::to_string(&story.save_state())
            .map_err(|e| JsError::new(&format!("save error: {e}")))
    }

    /// Like [`save`](Self::save) but a compact `MessagePack` byte blob (release).
    /// Same format as `save`; load with [`load_bytes`](Self::load_bytes).
    pub fn save_bytes(&self) -> Result<Vec<u8>, JsError> {
        let borrow = self.story.borrow();
        let story = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        rmp_serde::to_vec_named(&story.save_state())
            .map_err(|e| JsError::new(&format!("save error: {e}")))
    }

    /// Reconcile a JSON save (from [`save`](Self::save)) into the running
    /// story. Returns a `LoadReport` JSON (`{ unknown_globals: [...],
    /// unresolved_renames: [...], anonymous_states_dropped: N }`) of what
    /// the load couldn't apply — all empty/zero means a clean load.
    /// Tolerant of story patches.
    pub fn load(&self, json: &str) -> Result<String, JsError> {
        let state: brink_runtime::SaveState =
            serde_json::from_str(json).map_err(|e| JsError::new(&format!("load error: {e}")))?;
        self.apply_load(&state)
    }

    /// Like [`load`](Self::load) but from a `MessagePack` blob produced by
    /// [`save_bytes`](Self::save_bytes).
    pub fn load_bytes(&self, bytes: &[u8]) -> Result<String, JsError> {
        let state: brink_runtime::SaveState =
            rmp_serde::from_slice(bytes).map_err(|e| JsError::new(&format!("load error: {e}")))?;
        self.apply_load(&state)
    }

    /// Evaluate an ink function from the host, out-of-band: the visible story is
    /// untouched and the call returns synchronously. Arguments and the return
    /// value are native JS values (number / boolean / string / null). Externals
    /// the function calls resolve through the registered (synchronous) bindings.
    ///
    /// Errors if the function name is unknown, if a called external is async
    /// (can't resolve synchronously), or on a runtime error.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen passes a JS array as an owned Vec across the boundary"
    )]
    pub fn call_function(&self, name: &str, args: Vec<JsValue>) -> Result<JsValue, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("call_function"))?;
        let ink_args: Vec<Value> = args.iter().map(js_to_value).collect();
        let bindings = self.bindings.borrow();
        let handler = JsHandler {
            bindings: &bindings,
            lenient: self.lenient_unbound.get(),
            pending: &self.pending_promise,
        };
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        let value = story
            .call_function(name, &ink_args, &handler)
            .map_err(|e| JsError::new(&format!("call_function error: {e}")))?;
        Ok(value_to_js(&value))
    }

    /// Continue the story maximally. Returns JSON array of `Line` objects.
    ///
    /// Errors if a binding suspends (returns a Promise) — use the async
    /// `advance_one`/`resolve_external` path (or `@brink/wasm`'s `continueAsync`)
    /// for stories with async bindings.
    pub fn continue_story(&self) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("continue_story"))?;
        let bindings = self.bindings.borrow();
        let mut rec = self.recorder.borrow_mut();
        let handler = RecordingReplayHandler {
            inner: JsHandler {
                bindings: &bindings,
                lenient: self.lenient_unbound.get(),
                pending: &self.pending_promise,
            },
            recorder: RefCell::new(&mut rec),
            replaying: self.replaying.get(),
        };
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        let lines = story
            .continue_maximally_with(&handler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;

        let resp: Vec<LineJs> = lines.into_iter().map(line_to_js).collect();

        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Continue the story by a single line. Returns JSON for one `Line` object.
    /// Errors on a suspending binding (see [`continue_story`](Self::continue_story)).
    pub fn continue_single(&self) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("continue_single"))?;
        let bindings = self.bindings.borrow();
        let mut rec = self.recorder.borrow_mut();
        let handler = RecordingReplayHandler {
            inner: JsHandler {
                bindings: &bindings,
                lenient: self.lenient_unbound.get(),
                pending: &self.pending_promise,
            },
            recorder: RefCell::new(&mut rec),
            replaying: self.replaying.get(),
        };
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        let line = story
            .continue_single_with(&handler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;

        let resp = line_to_js(line);

        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Advance one step, surfacing a suspended async binding instead of
    /// erroring. Returns one `Line` JSON, or `{ "type": "awaiting_external",
    /// "name": … }` when a binding returned a `Promise` — at which point the
    /// caller takes it via [`take_pending_promise`](Self::take_pending_promise),
    /// awaits it, calls [`resolve_external`](Self::resolve_external), and steps
    /// again. (`@brink/wasm`'s `continueAsync` drives this loop for you.)
    pub fn advance_one(&self) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("advance_one"))?;
        let bindings = self.bindings.borrow();
        let mut rec = self.recorder.borrow_mut();
        let handler = RecordingReplayHandler {
            inner: JsHandler {
                bindings: &bindings,
                lenient: self.lenient_unbound.get(),
                pending: &self.pending_promise,
            },
            recorder: RefCell::new(&mut rec),
            replaying: self.replaying.get(),
        };
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        let resp = match story
            .advance_with(&handler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?
        {
            brink_runtime::StepOutcome::Step(step) => line_to_js(step),
            brink_runtime::StepOutcome::AwaitingExternal => LineJs {
                r#type: "awaiting_external",
                text: String::new(),
                tags: Vec::new(),
                block_id: None,
                choices: None,
                name: story.pending_external_name().map(str::to_owned),
            },
        };
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Resolve the external the flow is parked on with a native JS value
    /// (the awaited result of an async binding's `Promise`). No-op if not
    /// awaiting. Called by the JS wrapper between `advance_one` steps.
    pub fn resolve_external(&self, value: &JsValue) {
        if self.busy.get() {
            web_sys::console::warn_1(&JsValue::from_str(
                "brink: reentrant 'resolve_external' ignored",
            ));
            return;
        }
        let v = js_to_value(value);
        if let Some(story) = self.story.borrow_mut().as_mut() {
            // Record the resolved async external (live mode only) so a reload
            // can replay it. The flow is still parked, so its name + args are
            // available here.
            if !self.replaying.get() {
                let info = story
                    .pending_external_name()
                    .map(|n| (n.to_owned(), story.pending_external_args().to_vec()));
                if let Some((name, args)) = info {
                    self.recorder.borrow_mut().record(&name, &args, &v);
                }
            }
            story.resolve_external(v);
        }
    }

    /// Take the `Promise` a suspended async binding returned, for the caller to
    /// `await`. `undefined` if none is pending. After awaiting, feed the result
    /// to [`resolve_external`](Self::resolve_external).
    pub fn take_pending_promise(&self) -> JsValue {
        if self.busy.get() {
            web_sys::console::warn_1(&JsValue::from_str(
                "brink: reentrant 'take_pending_promise' ignored",
            ));
            return JsValue::UNDEFINED;
        }
        self.pending_promise
            .borrow_mut()
            .take()
            .map_or(JsValue::UNDEFINED, JsValue::from)
    }

    /// Choose an option by index.
    pub fn choose(&self, index: usize) -> Result<(), JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        story
            .choose(index)
            .map_err(|e| JsError::new(&format!("choose error: {e}")))
    }

    /// Move the play head to a named knot/stitch path — ink's
    /// `ChoosePathString` equivalent (`resetCallstack: true` semantics).
    /// Subsequent `continue_*` calls run from there. The session keeps its
    /// state: globals and visit/turn counts survive, the jump itself counts
    /// as a visit to the target (exactly like a `-> path` divert), and the
    /// transcript so far is kept (append-only) while pending choices are
    /// cleared.
    ///
    /// Errors on an unknown path (naming it), and refuses to jump while the
    /// flow is parked on an unresolved async external — resolve it (or
    /// `reset`) first; a pending host call is never silently abandoned.
    pub fn go_to_path(&self, path: &str) -> Result<(), JsError> {
        let _guard = BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("go_to_path"))?;
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        story
            .choose_path_string(path)
            .map_err(|e| JsError::new(&format!("go_to_path error: {e}")))
    }

    /// Like [`go_to_path`](Self::go_to_path) but **binds the target knot's
    /// declared parameters** from `args` — host-directed entry into a
    /// parameterized knot (`=== call(action, present) ===`). Args arrive as
    /// native JS values; semantics otherwise match `go_to_path`. Errors if the
    /// argument count doesn't match the knot's declared parameters.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen passes a JS array as an owned Vec across the boundary"
    )]
    pub fn go_to_path_with_args(&self, path: &str, args: Vec<JsValue>) -> Result<(), JsError> {
        let _guard = BusyGuard::acquire(&self.busy)
            .ok_or_else(|| reentrant_error("go_to_path_with_args"))?;
        let ink_args: Vec<Value> = args.iter().map(js_to_value).collect();
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        story
            .choose_path_string_with_args(path, &ink_args)
            .map_err(|e| JsError::new(&format!("go_to_path_with_args error: {e}")))
    }

    /// Reset: create a fresh story from the same program.
    pub fn reset(&self) {
        let mut story = brink_runtime::Story::<FastRng>::new(
            Arc::clone(&self.program),
            self.base_line_tables.clone(),
        );
        // Re-apply the host-set seed so a reset replays deterministically.
        if let Some(seed) = self.seed.get() {
            story.set_rng_seed(seed);
        }
        // A dev session stays a dev session across reset (play-from-here).
        if self.dev_visibility_override.get() {
            story.set_visibility_enforcement(false);
        }
        *self.story.borrow_mut() = Some(story);
        // A reset is a fresh playthrough: drop the recording and leave replay.
        *self.recorder.borrow_mut() = ReplayRecorder::new();
        self.replaying.set(false);
    }

    /// Hot-reload: swap in a freshly compiled program **in place**, preserving
    /// the session's external bindings, RNG seed, and replay recording, then
    /// reset the play head to the start. The studio calls this on recompile,
    /// then brackets a silent re-walk of the saved choice log with
    /// [`begin_replay`](Self::begin_replay) / [`end_replay`](Self::end_replay)
    /// so externals are served from the recording (query-gated branches
    /// reproduce, effect bindings don't re-fire) instead of re-invoked.
    ///
    /// Errors on decode/link failure — the previously loaded program keeps
    /// running unchanged.
    pub fn reload(&mut self, story_bytes: &[u8]) -> Result<(), JsError> {
        let data = brink_format::read_inkb(story_bytes)
            .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let (prog, line_tables) =
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?;

        // Drop the old story first, then swap in the new program/data/tables
        // and rebuild.
        *self.story.borrow_mut() = None;
        self.program = Arc::new(prog);
        self.data = data;
        self.base_line_tables.clone_from(&line_tables);

        let mut story =
            brink_runtime::Story::<FastRng>::new(Arc::clone(&self.program), line_tables);
        if let Some(seed) = self.seed.get() {
            story.set_rng_seed(seed);
        }
        // A dev session stays a dev session across hot-reload (play-from-here).
        if self.dev_visibility_override.get() {
            story.set_visibility_enforcement(false);
        }
        *self.story.borrow_mut() = Some(story);

        // Clear transient async state; keep bindings, seed, and the recorder.
        *self.pending_promise.borrow_mut() = None;
        self.busy.set(false);
        self.replaying.set(false);
        Ok(())
    }

    /// Enter replay mode and reset the replay cursor: subsequent visible
    /// playback (`continue_*` / `advance_one`) serves externals from the
    /// recording and re-runs nothing. Used by the studio to silently re-walk
    /// the saved choice log after [`reload`](Self::reload).
    pub fn begin_replay(&self) {
        self.recorder.borrow_mut().reset_cursor();
        self.replaying.set(true);
    }

    /// Leave replay mode: visible playback resumes invoking JS bindings and
    /// recording their results (appending to the existing log).
    pub fn end_replay(&self) {
        self.replaying.set(false);
    }

    /// Whether any external has been recorded this session. The studio uses
    /// this to decide whether a post-reload choice re-walk should enter replay
    /// mode (serve recorded externals) or run live — on a fresh load the
    /// recording is empty, so replay would only produce fallbacks.
    #[must_use]
    pub fn has_recording(&self) -> bool {
        !self.recorder.borrow().is_empty()
    }

    /// Whether the last execution cycle of the **default** flow ended with
    /// a safe exit (an explicit `-> DONE`), as opposed to the flow running
    /// out of content. Both deliver a `done`-type `Line`; read this right
    /// after one to tell them apart — `false` means the next
    /// `continueStory`/`continueSingle`/`advanceOne` call will error
    /// instead of returning more text. `false` if no story is loaded.
    ///
    /// This reflects only the default flow — it does **not** track flows
    /// spawned/continued via `spawnFlow`/`continueFlow`/
    /// `continueFlowMaximally`. See
    /// [`brink_runtime::Story::did_safe_exit`] (issue #1573).
    #[must_use]
    pub fn did_safe_exit(&self) -> bool {
        self.story
            .borrow()
            .as_ref()
            .is_some_and(brink_runtime::Story::did_safe_exit)
    }

    /// Structured, name-resolved snapshot of the runtime's current state for
    /// the studio State View — status, current location, globals, call stack,
    /// visit counts, pending choices, and rng. Returns JSON (`DebugState`).
    pub fn debug_snapshot(&self) -> Result<String, JsError> {
        let borrow = self.story.borrow();
        let story = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        let js = debug_snapshot_to_js(story.debug_snapshot());
        serde_json::to_string(&js).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    // ── Shared flows (#200) ─────────────────────────────────────────
    // A "+ new flow" in the studio spawns a named flow that SHARES this
    // story's globals / visit counts / rng with the default flow, while
    // keeping its own call stack — true ink concurrent-flow semantics, run
    // locally. (`spawn_flow` here is the runtime's shared-context variant; the
    // isolated `Story::spawn_flow` is bevy-brink's per-entity model.)

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
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        story
            .spawn_flow_shared(name, container_idx)
            .map_err(|e| JsError::new(&format!("spawn_flow error: {e}")))
    }

    /// Advance a shared flow by one line. Returns one `Line` JSON.
    pub fn continue_flow(&self, name: &str) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("continue_flow"))?;
        let bindings = self.bindings.borrow();
        // A plain handler — flow stepping is not part of the default flow's
        // choice-log replay, so it must not touch the recorder.
        let handler = JsHandler {
            bindings: &bindings,
            lenient: self.lenient_unbound.get(),
            pending: &self.pending_promise,
        };
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        let line = story
            .continue_flow_single_with(name, &handler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        serde_json::to_string(&line_to_js(line))
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Run a shared flow to its next terminal line. Returns JSON array of
    /// `Line` objects, capped at `FlowInstance::LINE_LIMIT` (10,000) lines —
    /// the same bound `continue_story` enforces for the primary flow, so an
    /// infinite-emitting flow errors instead of hanging or exhausting memory on the host
    /// (`FlowHandle.continueMaximally`'s wasm leg).
    pub fn continue_flow_maximally(&self, name: &str) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy)
            .ok_or_else(|| reentrant_error("continue_flow_maximally"))?;
        let bindings = self.bindings.borrow();
        // A plain handler — flow stepping is not part of the default flow's
        // choice-log replay, so it must not touch the recorder.
        let handler = JsHandler {
            bindings: &bindings,
            lenient: self.lenient_unbound.get(),
            pending: &self.pending_promise,
        };
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        let lines = story
            .continue_flow_maximally_shared_with(name, &handler)
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;
        let resp: Vec<LineJs> = lines.into_iter().map(line_to_js).collect();
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Select a choice in a shared flow.
    pub fn choose_flow(&self, name: &str, index: usize) -> Result<(), JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("choose_flow"))?;
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        story
            .choose_flow_shared(name, index)
            .map_err(|e| JsError::new(&format!("choose_flow error: {e}")))
    }

    /// Destroy a shared flow.
    pub fn destroy_flow(&self, name: &str) -> Result<(), JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("destroy_flow"))?;
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        story
            .destroy_flow(name)
            .map_err(|e| JsError::new(&format!("destroy_flow error: {e}")))
    }

    /// JSON array of active flow names (sorted, deterministic).
    pub fn flow_names(&self) -> Result<String, JsError> {
        let borrow = self.story.borrow();
        let story = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        serde_json::to_string(&story.flow_names())
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Per-flow debug snapshot (State View) for a named flow.
    pub fn flow_debug_snapshot(&self, name: &str) -> Result<String, JsError> {
        let borrow = self.story.borrow();
        let story = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        let snap = story
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
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        serde_json::to_string(&story.wake_check())
            .map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Fork a [`WebSpeculation`] — a sandboxed, side-effect-proof speculative
    /// run over the current story state (F4.3). It exposes its own composable
    /// verbs (`go_to_path`/`advance`/`choose`/`eval_function`/
    /// `resume_function_eval`/…); nothing it does ever reaches this runner's
    /// live story, and dropping it discards everything.
    ///
    /// `options_json` — a JSON object, all fields optional (`"{}"`/`""` for
    /// every default):
    ///
    /// ```json
    /// {
    ///   "steps": 100000,        // VM step budget for one `advance`/`evalFunction`/`resumeFunctionEval` call
    ///   "lines": 1000,          // total visible-line budget for this speculation
    ///   "context": "watch",     // "watch" | "eval" — gates Effect-kind externals
    ///   "liveEffects": false,   // arm Effect externals (only takes effect under "eval")
    ///   "kinds": { "name": "query" | "effect" }  // per-external policy tiering
    /// }
    /// ```
    ///
    /// A name absent from `kinds` is conservatively treated as `"effect"` —
    /// it never fires live under the default `"watch"` context. See
    /// [`brink_runtime::KindTieredHandler`].
    pub fn speculate(&self, options_json: &str) -> Result<WebSpeculation, JsError> {
        let opts: SpeculateOptionsJs = if options_json.trim().is_empty() {
            SpeculateOptionsJs::default()
        } else {
            serde_json::from_str(options_json)
                .map_err(|e| JsError::new(&format!("speculate options error: {e}")))?
        };

        let default_budget = brink_runtime::Budget::default();
        let budget = brink_runtime::Budget {
            steps: opts.steps.unwrap_or(default_budget.steps),
            lines: opts.lines.unwrap_or(default_budget.lines),
        };
        let context = match opts.context.as_deref() {
            None | Some("watch") => brink_runtime::EvalContext::Watch,
            Some("eval") => brink_runtime::EvalContext::Eval,
            Some(other) => {
                return Err(JsError::new(&format!(
                    "speculate: unknown context '{other}' (expected 'watch' or 'eval')"
                )));
            }
        };
        let mut kinds = HashMap::new();
        for (name, kind) in &opts.kinds {
            let k = match kind.as_str() {
                "query" => brink_runtime::PolicyKind::Query,
                "effect" => brink_runtime::PolicyKind::Effect,
                other => {
                    return Err(JsError::new(&format!(
                        "speculate: unknown kind '{other}' for external '{name}' \
                         (expected 'query' or 'effect')"
                    )));
                }
            };
            kinds.insert(name.clone(), k);
        }

        let borrow = self.story.borrow();
        let story = borrow
            .as_ref()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        let speculation = story.speculate();

        Ok(WebSpeculation {
            program: Arc::clone(&self.program),
            speculation: RefCell::new(speculation),
            budget,
            bindings: RefCell::new(self.bindings.borrow().clone()),
            lenient_unbound: self.lenient_unbound.get(),
            kinds,
            context,
            live_effects: opts.live_effects,
            pending_promise: RefCell::new(None),
            busy: Cell::new(false),
            externals_live: RefCell::new(Vec::new()),
            externals_fallback: RefCell::new(Vec::new()),
        })
    }
}

// ── Save/load helper (not wasm-exported: takes a Rust SaveState) ─────

impl StoryRunner {
    /// Reconcile a decoded `SaveState` into the running story and return the
    /// `LoadReport` as JSON. Shared by `load` (JSON) and `load_bytes` (msgpack).
    fn apply_load(&self, state: &brink_runtime::SaveState) -> Result<String, JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;
        let report = story.load_state(state);
        serde_json::to_string(&report).map_err(|e| JsError::new(&format!("load report error: {e}")))
    }
}
