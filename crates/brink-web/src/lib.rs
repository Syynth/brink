use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use brink_format::Value;
use brink_ide::session::IdeSession;
use brink_runtime::{ExternalFnHandler, ExternalResult, FastRng, ReplayRecorder};
use rowan::{TextRange, TextSize};
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

mod program_model;

// ── Compilation ─────────────────────────────────────────────────────

/// Compile ink source and return JSON with diagnostics or story data.
#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    let result = brink_compiler::compile("main.ink", |_path| Ok(source.to_owned()));

    match result {
        Ok(output) => {
            let warnings: Vec<DiagnosticJs> = output
                .warnings
                .iter()
                .map(|d| diagnostic_to_js(d, source))
                .collect();

            let data = output.data;
            let mut bytes = Vec::new();
            brink_format::write_inkb(&data, &mut bytes);

            let resp = CompileResult {
                ok: true,
                story_bytes: Some(bytes),
                warnings,
                error: None,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
        Err(e) => {
            let mut diagnostics = Vec::new();
            let mut error_msg = None;

            match e {
                brink_compiler::CompileError::Diagnostics(diags) => {
                    diagnostics = diags.iter().map(|d| diagnostic_to_js(d, source)).collect();
                }
                other => {
                    error_msg = Some(format!("{other}"));
                }
            }

            let resp = CompileResult {
                ok: false,
                story_bytes: None,
                warnings: diagnostics,
                error: error_msg,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
    }
}

/// The source-identity checksum of compiled `.inkb` bytes, formatted as
/// `0x{:08x}` — identical to `ProgramModel.checksum`. Lets the studio compare a
/// running program's identity against its latest compile *without* constructing
/// a `StoryRunner` (live-inspector degraded mode, #181 / spec §5): a mismatch
/// means the running program is not the studio's current source.
#[wasm_bindgen]
pub fn program_checksum(story_bytes: &[u8]) -> Result<String, JsError> {
    let data = brink_format::read_inkb(story_bytes)
        .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
    Ok(format!("0x{:08x}", data.source_checksum))
}

#[derive(Serialize)]
struct CompileResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    story_bytes: Option<Vec<u8>>,
    warnings: Vec<DiagnosticJs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct DiagnosticJs {
    message: String,
    start: u32,
    end: u32,
    severity: String,
    /// Path of the file this diagnostic belongs to. In a multi-file project a
    /// diagnostic may live in an included file rather than the entry, so the
    /// editor uses this to place it on the right tab.
    file: String,
}

// ── Runtime ─────────────────────────────────────────────────────────

/// A running story instance. Owns Program + Story to avoid lifetime issues in wasm.
#[wasm_bindgen]
pub struct StoryRunner {
    // We store Program in a Box to get a stable address, and the Story
    // borrows from it. We use raw pointer + RefCell to work around the
    // self-referential borrow.
    program: Box<brink_runtime::Program>,
    base_line_tables: Vec<Vec<brink_format::LineEntry>>,
    /// The decoded `StoryData`, retained for `program_inkt` (Program Explorer).
    /// Independent owned value — `link` only borrows it.
    data: brink_format::StoryData,
    // Safety: story borrows from program which is heap-pinned and never moved.
    // We only access story through &mut self methods (single-threaded wasm).
    story: RefCell<Option<brink_runtime::Story<'static, FastRng>>>,
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
        let program = Box::new(prog);

        // Safety: we pin the Program in a Box and keep it alive for the
        // lifetime of StoryRunner. The Story borrows it, but we transmute
        // the lifetime to 'static. This is safe because:
        // 1. program is heap-allocated and never moved
        // 2. story is dropped before program (struct drop order)
        // 3. wasm is single-threaded
        let program_ptr: *const brink_runtime::Program = &raw const *program;
        // SAFETY: program is heap-allocated via Box and never moved. Story borrows
        // it for 'static, but StoryRunner keeps the Box alive and drops story first
        // (struct field drop order). wasm is single-threaded.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };

        let story = brink_runtime::Story::<FastRng>::new(program_ref, line_tables.clone());

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
    /// story. Returns a `LoadReport` JSON (`{ unknown_globals: [...] }`) of
    /// saved globals the current story no longer has — empty means a clean
    /// load. Tolerant of story patches.
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
            brink_runtime::StepOutcome::Line(line) => line_to_js(line),
            brink_runtime::StepOutcome::AwaitingExternal => LineJs {
                r#type: "awaiting_external",
                text: String::new(),
                tags: Vec::new(),
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
        let program_ptr: *const brink_runtime::Program = &raw const *self.program;
        // SAFETY: same invariants as in `new` — Box is pinned and outlives the Story.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };

        let mut story =
            brink_runtime::Story::<FastRng>::new(program_ref, self.base_line_tables.clone());
        // Re-apply the host-set seed so a reset replays deterministically.
        if let Some(seed) = self.seed.get() {
            story.set_rng_seed(seed);
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

        // Drop the old story first — it borrows the program we're about to
        // replace — then swap the heap-pinned program/data/tables and rebuild.
        // Write into the existing Box (stable address) rather than reallocating.
        *self.story.borrow_mut() = None;
        *self.program = prog;
        self.data = data;
        self.base_line_tables.clone_from(&line_tables);

        let program_ptr: *const brink_runtime::Program = &raw const *self.program;
        // SAFETY: same invariants as `new` — the Box is pinned and outlives the
        // Story, and the old Story was dropped above before this new borrow.
        // wasm is single-threaded.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };
        let mut story = brink_runtime::Story::<FastRng>::new(program_ref, line_tables);
        if let Some(seed) = self.seed.get() {
            story.set_rng_seed(seed);
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

// ── Reentrancy guard ─────────────────────────────────────────────────

/// RAII guard for [`StoryRunner::busy`]. [`acquire`](Self::acquire) returns
/// `None` if a VM-stepping method is already running (a binding callback or a
/// concurrent driver re-entered), so the caller can warn + error cleanly
/// instead of hitting a `RefCell` double-borrow panic. Clears the flag on drop,
/// including on an early `?`/error return.
struct BusyGuard<'a>(&'a Cell<bool>);

impl<'a> BusyGuard<'a> {
    fn acquire(flag: &'a Cell<bool>) -> Option<Self> {
        if flag.get() {
            return None;
        }
        flag.set(true);
        Some(BusyGuard(flag))
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// `console.warn` + the `JsError` returned when a stepping method is re-entered.
fn reentrant_error(method: &str) -> JsError {
    web_sys::console::warn_1(&JsValue::from_str(&format!(
        "brink: reentrant '{method}' ignored — a binding callback or concurrent \
         driver re-entered the runner"
    )));
    JsError::new("reentrant brink call")
}

// ── External-function bindings ───────────────────────────────────────

/// [`ExternalFnHandler`] backed by the runner's registry of JS callbacks.
///
/// Resolves a bound external by invoking its JS function with the call
/// arguments (as native JS values) and reading the return back into a
/// [`Value`]. An unbound external either falls back to the ink body
/// (`lenient = false`) or resolves to `null` (`lenient = true`).
struct JsHandler<'a> {
    bindings: &'a HashMap<String, js_sys::Function>,
    lenient: bool,
    /// Where an async binding's returned `Promise` is stashed so the JS wrapper
    /// can await it (see [`StoryRunner::take_pending_promise`]).
    pending: &'a RefCell<Option<js_sys::Promise>>,
}

impl ExternalFnHandler for JsHandler<'_> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        let Some(f) = self.bindings.get(name) else {
            return if self.lenient {
                ExternalResult::Resolved(Value::Null)
            } else {
                ExternalResult::Fallback
            };
        };
        let js_args = js_sys::Array::new();
        for a in args {
            js_args.push(&value_to_js(a));
        }
        match f.apply(&JsValue::NULL, &js_args) {
            Ok(ret) => {
                // Unified async: a binding that returns a Promise suspends the
                // flow. Stash the Promise; the JS wrapper awaits it and feeds
                // the result back via `resolve_external`.
                if ret.is_instance_of::<js_sys::Promise>() {
                    *self.pending.borrow_mut() = Some(ret.unchecked_into::<js_sys::Promise>());
                    ExternalResult::Pending
                } else {
                    ExternalResult::Resolved(js_to_value(&ret))
                }
            }
            // A registered binding that throws is a host bug; don't propagate
            // the JS exception into the VM — warn, resolve to null, carry on.
            Err(e) => {
                web_sys::console::warn_2(
                    &JsValue::from_str(&format!("brink: binding '{name}' threw; resolving null:")),
                    &e,
                );
                ExternalResult::Resolved(Value::Null)
            }
        }
    }
}

/// Composes the runner's [`JsHandler`] with the replay [`ReplayRecorder`] for
/// visible playback (`continue_*` / `advance_one`):
///
/// - **Live** (`replaying = false`): delegate to `inner` (the JS bindings) and
///   record each inline-`Resolved` result. A `Pending` (async) result is
///   recorded later by [`StoryRunner::resolve_external`]; `Fallback` isn't
///   recorded.
/// - **Replay** (`replaying = true`): serve the next recorded result for
///   `name` + `args` and re-run nothing (so query-gated branches reproduce and
///   effect bindings don't re-fire). An uncovered / divergent call returns
///   [`ExternalResult::Fallback`] (the ink fallback body) — the cursor latches
///   diverged, matching the shared primitive.
struct RecordingReplayHandler<'a> {
    inner: JsHandler<'a>,
    recorder: RefCell<&'a mut ReplayRecorder>,
    replaying: bool,
}

impl ExternalFnHandler for RecordingReplayHandler<'_> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        if self.replaying {
            return match self.recorder.borrow_mut().take_recorded(name, args) {
                Some(v) => ExternalResult::Resolved(v),
                None => ExternalResult::Fallback,
            };
        }
        let result = self.inner.call(name, args);
        if let ExternalResult::Resolved(v) = &result {
            self.recorder.borrow_mut().record(name, args, v);
        }
        result
    }
}

/// Map an ink [`Value`] to a native JS value for a binding argument. Only the
/// scalar variants cross the boundary; VM-internal variants (pointers, divert
/// targets, fragment refs, lists) map to `null` for now.
fn value_to_js(v: &Value) -> JsValue {
    match v {
        Value::Int(i) => JsValue::from_f64(f64::from(*i)),
        Value::Float(f) => JsValue::from_f64(f64::from(*f)),
        Value::Bool(b) => JsValue::from_bool(*b),
        Value::String(s) => JsValue::from_str(s),
        _ => JsValue::NULL,
    }
}

/// Read a JS value returned from a binding back into an ink [`Value`].
/// `null`/`undefined` → `Null`; booleans → `Bool`; an integer-valued finite
/// number → `Int`, otherwise `Float`; strings → `String`. Anything else → `Null`.
fn js_to_value(js: &JsValue) -> Value {
    if js.is_null() || js.is_undefined() {
        return Value::Null;
    }
    if let Some(b) = js.as_bool() {
        return Value::Bool(b);
    }
    if let Some(n) = js.as_f64() {
        if n.is_finite() && n.fract() == 0.0 && n.abs() <= f64::from(i32::MAX) {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "guarded: finite, integral, within i32 range"
            )]
            return Value::Int(n as i32);
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "ink floats are f32; JS numbers narrow to f32 at the boundary"
        )]
        return Value::Float(n as f32);
    }
    if let Some(s) = js.as_string() {
        return Value::String(Arc::from(s));
    }
    Value::Null
}

// ── Debug snapshot JSON mirror ───────────────────────────────────────

#[derive(Serialize)]
struct DebugStateJs {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_location: Option<String>,
    turn_index: u32,
    globals: Vec<DebugGlobalJs>,
    call_stack: Vec<DebugFrameJs>,
    visit_counts: Vec<DebugVisitJs>,
    pending_choices: Vec<DebugChoiceJs>,
    rng: DebugRngJs,
}

#[derive(Serialize)]
struct DebugGlobalJs {
    name: String,
    value: String,
}

#[derive(Serialize)]
struct DebugFrameJs {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    temps: usize,
}

#[derive(Serialize)]
struct DebugVisitJs {
    path: String,
    count: u32,
}

#[derive(Serialize)]
struct DebugChoiceJs {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
}

#[derive(Serialize)]
struct DebugRngJs {
    seed: i32,
    previous: i32,
}

fn debug_snapshot_to_js(s: brink_runtime::DebugSnapshot) -> DebugStateJs {
    DebugStateJs {
        status: s.status,
        current_location: s.current_location,
        turn_index: s.turn_index,
        globals: s
            .globals
            .into_iter()
            .map(|g| DebugGlobalJs {
                name: g.name,
                value: g.value,
            })
            .collect(),
        call_stack: s
            .call_stack
            .into_iter()
            .map(|f| DebugFrameJs {
                kind: f.kind,
                location: f.location,
                temps: f.temps,
            })
            .collect(),
        visit_counts: s
            .visit_counts
            .into_iter()
            .map(|v| DebugVisitJs {
                path: v.path,
                count: v.count,
            })
            .collect(),
        pending_choices: s
            .pending_choices
            .into_iter()
            .map(|c| DebugChoiceJs {
                text: c.text,
                target: c.target,
            })
            .collect(),
        rng: DebugRngJs {
            seed: s.rng.seed,
            previous: s.rng.previous,
        },
    }
}

fn line_to_js(line: brink_runtime::Line) -> LineJs {
    match line {
        brink_runtime::Line::Text { text, tags } => LineJs {
            r#type: "text",
            text,
            tags,
            choices: None,
            name: None,
        },
        brink_runtime::Line::Choices {
            text,
            tags,
            choices,
        } => LineJs {
            r#type: "choices",
            text,
            tags,
            choices: Some(
                choices
                    .into_iter()
                    .map(|c| ChoiceJs {
                        text: c.text,
                        index: c.index,
                        tags: c.tags,
                    })
                    .collect(),
            ),
            name: None,
        },
        brink_runtime::Line::Done { text, tags } => LineJs {
            r#type: "done",
            text,
            tags,
            choices: None,
            name: None,
        },
        brink_runtime::Line::End { text, tags } => LineJs {
            r#type: "end",
            text,
            tags,
            choices: None,
            name: None,
        },
    }
}

#[derive(Serialize)]
struct LineJs {
    r#type: &'static str,
    text: String,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<ChoiceJs>>,
    /// External name for the `awaiting_external` variant; omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Serialize)]
struct ChoiceJs {
    text: String,
    index: usize,
    tags: Vec<String>,
}

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
    // Same self-referential Box-pinning as `StoryRunner`: `program` is
    // heap-allocated and never moved; `session` borrows it as `'static` but
    // is dropped first (struct field order), and wasm is single-threaded.
    program: Box<brink_runtime::Program>,
    base_line_tables: Vec<Vec<brink_format::LineEntry>>,
    session: RefCell<Option<brink_runtime::StorySession<'static, FastRng>>>,
    /// Explicit RNG seed, if the host set one. Re-applied on `restart`/
    /// `reload` so re-runs stay deterministic.
    seed: Cell<Option<i32>>,
    /// External names that always park as `awaiting_external` regardless of
    /// whether the story defines a fallback body — the `deferred: string[]`
    /// constructor option. Checked before falling through to the ink body.
    always_deferred: std::collections::BTreeSet<String>,
    /// Reentrancy guard, mirroring `StoryRunner::busy`.
    busy: Cell<bool>,
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
        let program = Box::new(prog);

        let program_ptr: *const brink_runtime::Program = &raw const *program;
        // SAFETY: program is heap-allocated via Box and never moved. The
        // session borrows it for 'static, but WebSession keeps the Box alive
        // and drops `session` first (struct field drop order). wasm is
        // single-threaded. Identical invariants to `StoryRunner::new`.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };

        let mut story = brink_runtime::Story::<FastRng>::new(program_ref, line_tables.clone());
        if let Some(s) = seed {
            story.set_rng_seed(s);
        }
        let session = brink_runtime::StorySession::new(story, seed.map(seed_to_journal_u64));

        Ok(WebSession {
            program,
            base_line_tables: line_tables,
            session: RefCell::new(Some(session)),
            seed: Cell::new(seed),
            always_deferred: deferred.unwrap_or_default().into_iter().collect(),
            busy: Cell::new(false),
            last_replay_outcome: RefCell::new(None),
        })
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
        *self.program = prog;
        self.base_line_tables.clone_from(&line_tables);

        let program_ptr: *const brink_runtime::Program = &raw const *self.program;
        // SAFETY: same invariants as `new` — the Box is pinned and outlives
        // the session, and the old session was dropped above before this new
        // borrow. wasm is single-threaded.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };
        let mut story = brink_runtime::Story::<FastRng>::new(program_ref, line_tables);
        if let Some(s) = self.seed.get() {
            story.set_rng_seed(s);
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
        let program_ptr: *const brink_runtime::Program = &raw const *self.program;
        // SAFETY: same invariants as `new` — Box is pinned and outlives the session.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };
        let mut story =
            brink_runtime::Story::<FastRng>::new(program_ref, self.base_line_tables.clone());
        if let Some(s) = self.seed.get() {
            story.set_rng_seed(s);
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
        let program = Box::new(prog);

        let program_ptr: *const brink_runtime::Program = &raw const *program;
        // SAFETY: same invariants as `WebSession::new`.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };
        let mut story = brink_runtime::Story::<FastRng>::new(program_ref, line_tables.clone());
        if let Some(s) = seed {
            story.set_rng_seed(s);
        }
        let (session, outcome) = brink_runtime::StorySession::restore(story, journal)
            .map_err(|e| JsError::new(&format!("restore error: {e}")))?;

        Ok((
            WebSession {
                program,
                base_line_tables: line_tables,
                session: RefCell::new(Some(session)),
                seed: Cell::new(seed),
                always_deferred: std::collections::BTreeSet::new(),
                busy: Cell::new(false),
                last_replay_outcome: RefCell::new(None),
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
    session: &brink_runtime::StorySession<'_, R>,
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

// ── EditorSession ───────────────────────────────────────────────────

/// Stateful IDE session for the web editor. Wraps `IdeSession` and exposes
/// all IDE queries as methods that return JSON strings.
/// A view context scopes the editor to a sub-region of a file.
/// When active, `update_source` splices the fragment into the full file
/// at `[start, end)`, and IDE responses adjust offsets relative to the view.
#[derive(Clone, Copy)]
struct ViewContext {
    /// Byte offset where the view begins in the full file.
    start: u32,
    /// Byte offset where the view ends (exclusive) in the full file.
    end: u32,
    /// 0-based line number of the view start (for line-based IDE responses).
    start_line: u32,
    /// Whether `full[original_end..]` started with `\n` when the context was set.
    /// When true, `update_source` ensures a `\n` separator is maintained after
    /// the fragment, so edits at the end don't merge with the next section.
    trailing_newline: bool,
}

/// A document handle's state: the file it addresses plus an optional
/// sub-file view context (fragment handles).
struct DocState {
    path: String,
    view: Option<ViewContext>,
}

#[wasm_bindgen]
pub struct EditorSession {
    session: IdeSession,
    /// The active file path for IDE queries (legacy singleton API).
    active_path: String,
    /// Optional sub-file view context for focused editing (legacy singleton API).
    view: Option<ViewContext>,
    /// Open document handles, keyed by id. `BTreeMap` for deterministic
    /// iteration order (project rule: never iterate a `HashMap` where order
    /// can affect output).
    docs: BTreeMap<u32, DocState>,
    /// Next document-handle id. Starts at 1 — 0 is the "invalid handle"
    /// sentinel returned by `open_document`/`open_fragment` on failure.
    next_doc_id: u32,
}

impl Default for EditorSession {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl EditorSession {
    /// Create a new empty editor session.
    #[wasm_bindgen(constructor)]
    pub fn new() -> EditorSession {
        EditorSession {
            session: IdeSession::new(),
            active_path: "main.ink".to_owned(),
            view: None,
            docs: BTreeMap::new(),
            next_doc_id: 1,
        }
    }

    /// Update the active file's source text. Reparses, lowers, and analyzes.
    ///
    /// When a view context is active, `source` is treated as a fragment that
    /// gets spliced into the full file at `[view.start, view.end)`.
    pub fn update_source(&mut self, source: &str) {
        if let Some(view) = self.view {
            let full = self
                .session
                .file_id(&self.active_path)
                .and_then(|id| self.session.source(id).map(str::to_owned))
                .unwrap_or_default();
            let outcome = splice_fragment(&full, &view, source);
            if let Some(v) = &mut self.view {
                v.end = outcome.new_view_end;
            }
            self.session
                .update_and_analyze(&self.active_path, outcome.spliced);
        } else {
            self.session
                .update_and_analyze(&self.active_path, source.to_owned());
        }
    }

    /// Add or update a file by path. Re-analyzes the project.
    pub fn update_file(&mut self, path: &str, source: &str) {
        self.session.update_and_analyze(path, source.to_owned());
    }

    /// Remove a file from the project.
    pub fn remove_file(&mut self, path: &str) {
        self.session.remove_file(path);
    }

    /// Register (or replace) the host-capability manifest from a JSON string,
    /// then re-analyze. The manifest describes the host's external-function
    /// vocabulary (types, semantic types) for author-time validation and
    /// richer hover/completion. Tooling-only — never affects the runtime.
    pub fn set_host_manifest(&mut self, json: &str) -> Result<(), JsError> {
        let manifest: brink_ir::HostManifest = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid host manifest: {e}")))?;
        self.session.set_host_manifest(manifest);
        Ok(())
    }

    /// Clear any registered host manifest, then re-analyze.
    pub fn clear_host_manifest(&mut self) {
        self.session.clear_host_manifest();
    }

    /// Register (or replace) the dialogue dialect (#368) from a JSON string.
    /// The dialect describes the project's dialogue-line conventions (cues,
    /// parentheticals, dialogue chains) so `line_contexts` can classify
    /// lines without hardcoding any one convention. Tooling-only — never
    /// affects the runtime or analysis; consumed at query time by
    /// `line_contexts`/`line_contexts_doc`. Mirrors `set_host_manifest`.
    pub fn set_dialect(&mut self, json: &str) -> Result<(), JsError> {
        let dialect: brink_ir::DialogueDialect = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid dialect: {e}")))?;
        brink_ir::dialect::validate(&dialect)
            .map_err(|errs| JsError::new(&format!("invalid dialect: {errs:?}")))?;
        let resolved = brink_ir::ResolvedDialect::compile(&dialect)
            .map_err(|e| JsError::new(&format!("invalid dialect: {e}")))?;
        self.session.set_dialect(resolved);
        Ok(())
    }

    /// Clear the registered dialect. `line_contexts` reverts to plain
    /// structural classification.
    pub fn clear_dialect(&mut self) {
        self.session.clear_dialect();
    }

    /// Push the host's current values for `host`-source semantic types (Tier 3,
    /// #174) from a JSON object `{ "<type>": [{ "value", "label", "detail"? }] }`
    /// — a full snapshot that **replaces** the cache. The attached host (e.g.
    /// RPG Maker MZ) calls this with its named switches / items / … so the
    /// argument picker + value-label inlay hints stay current. Tooling-only;
    /// no re-analyze (values are consumed at query time, not in analysis).
    pub fn set_host_values(&mut self, json: &str) -> Result<(), JsError> {
        let values: brink_ide::HostValues = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid host values: {e}")))?;
        self.session.set_host_values(values);
        Ok(())
    }

    /// Clear the host-pushed value cache (e.g. on host disconnect). The picker
    /// degrades to plain literal entry for `host`-source params.
    pub fn clear_host_values(&mut self) {
        self.session.clear_host_values();
    }

    /// Set the severity policy for manifest-driven external diagnostics:
    /// `"error"` (default — a registered manifest is binding) or `"off"`.
    pub fn set_external_check(&mut self, level: &str) -> Result<(), JsError> {
        let severity = match level {
            "error" => brink_analyzer::ExternalCheckSeverity::Error,
            "off" => brink_analyzer::ExternalCheckSeverity::Off,
            other => {
                return Err(JsError::new(&format!(
                    "unknown external-check level `{other}` (expected \"error\" or \"off\")"
                )));
            }
        };
        self.session.set_external_check(severity);
        Ok(())
    }

    /// Switch the active file for IDE queries. Returns false if the file is not loaded.
    /// Clears any active view context (view is file-specific).
    pub fn set_active_file(&mut self, path: &str) -> bool {
        if self.session.file_id(path).is_some() {
            path.clone_into(&mut self.active_path);
            self.view = None;
            true
        } else {
            false
        }
    }

    /// Set a view context scoping the editor to `[start, end)` of the active file.
    /// IDE queries will adjust offsets relative to this range.
    pub fn set_view_context(&mut self, start: u32, end: u32) {
        self.view = Some(self.compute_view_context(&self.active_path, start, end));
    }

    /// Clear the view context, returning to full-file mode.
    pub fn clear_view_context(&mut self) {
        self.view = None;
    }

    /// Get the source text for the current view. Returns the fragment if a view
    /// context is active, or the full file otherwise. Returns a JSON string.
    pub fn get_view_source(&self) -> String {
        self.get_view_source_impl(&self.active_path, self.view.as_ref())
    }

    // ── Document handles ────────────────────────────────────────────
    //
    // Multi-document addressing: each handle pairs a file path with an
    // optional fragment view, so N live editor views can issue IDE queries
    // independently. The legacy active-file/view-context singleton above is
    // untouched by everything below. See the `*_doc` query variants.

    /// Open a full-file document handle on `path`. Returns the handle id,
    /// or `0` (never a valid id) if the file is not loaded.
    pub fn open_document(&mut self, path: &str) -> u32 {
        if self.session.file_id(path).is_none() {
            return 0;
        }
        self.insert_doc(DocState {
            path: path.to_owned(),
            view: None,
        })
    }

    /// Open a fragment document handle scoping `path` to `[start, end)`
    /// (UTF-16 offsets, same convention as `set_view_context`). Returns the
    /// handle id, or `0` (never a valid id) if the file is not loaded.
    pub fn open_fragment(&mut self, path: &str, start: u32, end: u32) -> u32 {
        if self.session.file_id(path).is_none() {
            return 0;
        }
        let view = self.compute_view_context(path, start, end);
        self.insert_doc(DocState {
            path: path.to_owned(),
            view: Some(view),
        })
    }

    /// Close a document handle. Returns `false` if the handle was unknown.
    pub fn close_document(&mut self, doc: u32) -> bool {
        self.docs.remove(&doc).is_some()
    }

    /// Replace a document's content: full-file replace for file handles,
    /// fragment splice for fragment handles (the handle's own view range is
    /// updated to cover the new fragment). Reparses, lowers, and analyzes.
    ///
    /// Returns a change-spec JSON object `{path, start, end, text?}`
    /// describing what actually changed in the file, in UTF-16 **file**
    /// coordinates: `[start, end)` is the replaced range of the file's
    /// previous content. The inserted text is the `source` argument the
    /// caller already has — unless `text` is present, in which case the
    /// fragment splice appended a `\n` separator and `text` carries what was
    /// actually inserted (`source` + `"\n"`). Returns `"null"` for an
    /// unknown handle.
    ///
    /// Other handles on the same file keep their ranges as-is; rebasing
    /// sibling fragment views from the change spec is the caller's job.
    pub fn update_document(&mut self, doc: u32, source: &str) -> String {
        let Some(state) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        let path = state.path.clone();
        let view = state.view;
        let full = self
            .session
            .file_id(&path)
            .and_then(|id| self.session.source(id).map(str::to_owned))
            .unwrap_or_default();

        let spec = if let Some(view) = view {
            let outcome = splice_fragment(&full, &view, source);
            if let Some(v) = self.docs.get_mut(&doc).and_then(|s| s.view.as_mut()) {
                v.end = outcome.new_view_end;
            }
            let spec = ChangeSpecJs {
                path: path.clone(),
                start: byte_to_utf16(&full, outcome.replaced_start),
                end: byte_to_utf16(&full, outcome.replaced_end),
                text: outcome.inserted_separator.then(|| format!("{source}\n")),
            };
            self.session.update_and_analyze(&path, outcome.spliced);
            spec
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink files are always < 4GB"
            )]
            let full_len = full.len() as u32;
            let spec = ChangeSpecJs {
                path: path.clone(),
                start: 0,
                end: byte_to_utf16(&full, full_len),
                text: None,
            };
            self.session.update_and_analyze(&path, source.to_owned());
            spec
        };
        serde_json::to_string(&spec).unwrap_or_default()
    }

    // ── Document-handle query variants ──────────────────────────────
    //
    // Same offset conventions as the singleton queries above (UTF-16,
    // view-relative per handle) and same JSON response shapes. An unknown
    // handle returns the same empty sentinel as a missing file.

    /// Get the source text for a document handle's view (fragment or full
    /// file). Returns a JSON string, or `"null"` for an unknown handle.
    pub fn get_view_source_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.get_view_source_impl(&d.path, d.view.as_ref())
    }

    /// Compute per-line context for a document handle. Returns JSON array of `LineContext`.
    pub fn line_contexts_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.line_contexts_impl(&d.path, d.view.as_ref())
    }

    /// Compute semantic tokens for a document handle. Returns JSON array of tokens.
    pub fn semantic_tokens_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.semantic_tokens_impl(&d.path, d.view.as_ref())
    }

    /// Compute completions for a document handle at the given offset. Returns JSON array.
    pub fn completions_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.completions_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute hover info for a document handle at the given offset. Returns JSON or "null".
    pub fn hover_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.hover_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute goto-definition for a document handle at the given offset. Returns JSON or "null".
    pub fn goto_definition_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.goto_definition_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Find all references for a document handle. Returns JSON array.
    pub fn find_references_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.find_references_impl(&d.path, d.view.as_ref(), offset, true)
    }

    /// Check if rename is possible for a document handle. Returns JSON or "null".
    pub fn prepare_rename_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.prepare_rename_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute code actions for a document handle. Returns JSON array.
    pub fn code_actions_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.code_actions_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute inlay hints for a document handle. Returns JSON array.
    pub fn inlay_hints_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.inlay_hints_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Color hints (`hex_color` argument literals) for a document handle, for
    /// the built-in color picker (#174-adjacent). Returns JSON array of
    /// `{ start, end, value }` (UTF-16 offsets).
    pub fn color_hints_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.color_hints_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Argument-widget sites for a document handle (argument-widget spec §4):
    /// every call's per-parameter slots + state (Filled / Empty / Expr), for
    /// inline editing and empty-slot filling. Returns a JSON array of
    /// `{ callee, slots: [{ param_name, widget?, type_name?, state }] }`
    /// (UTF-16 offsets).
    pub fn argument_widgets_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.argument_widgets_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Compute signature help for a document handle. Returns JSON or "null".
    pub fn signature_help_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.signature_help_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute folding ranges for a document handle. Returns JSON array.
    pub fn folding_ranges_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.folding_ranges_impl(&d.path, d.view.as_ref())
    }

    /// Compute document symbols (outline) for a document handle. Returns JSON array.
    pub fn document_symbols_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.document_symbols_impl(&d.path)
    }

    /// Convert a line element for a document handle. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element_doc(&self, doc: u32, offset: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.convert_element_impl(&d.path, d.view.as_ref(), offset, target)
    }

    /// Format a document handle's file (sort knots). Returns the formatted
    /// source as a JSON string.
    pub fn format_document_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "\"\"".to_owned();
        };
        self.format_document_impl(&d.path)
    }

    /// Get the current active file path.
    pub fn active_file(&self) -> String {
        self.active_path.clone()
    }

    /// List all loaded files. Returns JSON `[{path}]`.
    pub fn list_files(&self) -> String {
        let db = self.session.db();
        let files: Vec<ProjectFileJs> = db
            .file_ids()
            .filter_map(|id| {
                db.file_path(id)
                    .map(|p| ProjectFileJs { path: p.to_owned() })
            })
            .collect();
        serde_json::to_string(&files).unwrap_or_default()
    }

    /// Get the source text for a file. Returns JSON string or `"null"`.
    pub fn get_file_source(&self, path: &str) -> String {
        let source = self
            .session
            .file_id(path)
            .and_then(|id| self.session.source(id));
        match source {
            Some(s) => serde_json::to_string(s).unwrap_or_default(),
            None => "null".to_owned(),
        }
    }

    /// Get document symbols for a specific file. Returns JSON `DocumentSymbol[]`.
    pub fn file_symbols(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(manifest)) =
            (self.session.hir(file_id), self.session.manifest(file_id))
        else {
            return "[]".to_owned();
        };

        let source = self.session.source(file_id).unwrap_or("");
        let syms = brink_ide::document::document_symbols(hir, manifest, source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();
        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compile the project using all loaded files. Returns JSON `CompileResult`.
    pub fn compile_project(&self, entry: &str) -> String {
        let session = &self.session;
        // Carry the registered host manifest into compilation so its
        // diagnostics (type/arity/domain) surface alongside compiler output.
        let result = brink_compiler::compile_with_options(
            entry,
            |path| {
                session
                    .file_id(path)
                    .and_then(|id| session.source(id))
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("file not found: {path}"),
                        )
                    })
            },
            session.analysis_options(),
        );

        // Convert a diagnostic against its OWN file's source (offsets are
        // file-relative); the resolved diagnostic already carries that file's
        // path, so an INCLUDEd file's error lands on the right tab instead of
        // collapsing onto the entry.
        let to_js = |d: &brink_compiler::ResolvedDiagnostic| {
            let src = session.source(d.file).unwrap_or("");
            diagnostic_to_js(d, src)
        };

        match result {
            Ok(output) => {
                let warnings: Vec<DiagnosticJs> = output.warnings.iter().map(to_js).collect();

                let data = output.data;
                let mut bytes = Vec::new();
                brink_format::write_inkb(&data, &mut bytes);

                let resp = CompileResult {
                    ok: true,
                    story_bytes: Some(bytes),
                    warnings,
                    error: None,
                };
                serde_json::to_string(&resp).unwrap_or_default()
            }
            Err(e) => {
                let mut diagnostics = Vec::new();
                let mut error_msg = None;

                match e {
                    brink_compiler::CompileError::Diagnostics(diags) => {
                        diagnostics = diags.iter().map(to_js).collect();
                    }
                    other => {
                        error_msg = Some(format!("{other}"));
                    }
                }

                let resp = CompileResult {
                    ok: false,
                    story_bytes: None,
                    warnings: diagnostics,
                    error: error_msg,
                };
                serde_json::to_string(&resp).unwrap_or_default()
            }
        }
    }

    /// Get project outline — all files with their symbols. Returns JSON `[{path, symbols}]`.
    pub fn project_outline(&self) -> String {
        let db = self.session.db();
        let mut outline: Vec<FileOutlineJs> = Vec::new();

        for id in db.file_ids() {
            let Some(path) = db.file_path(id) else {
                continue;
            };
            let (Some(hir), Some(manifest)) = (db.hir(id), db.manifest(id)) else {
                outline.push(FileOutlineJs {
                    path: path.to_owned(),
                    symbols: Vec::new(),
                });
                continue;
            };

            let source = db.source(id).unwrap_or("");
            let syms = brink_ide::document::document_symbols(hir, manifest, source);
            let items: Vec<DocumentSymbolJs> = syms
                .into_iter()
                .map(|s| convert_document_symbol(s, source))
                .collect();
            outline.push(FileOutlineJs {
                path: path.to_owned(),
                symbols: items,
            });
        }

        // Sort by path for deterministic output
        outline.sort_by(|a, b| a.path.cmp(&b.path));
        serde_json::to_string(&outline).unwrap_or_default()
    }

    /// Whole-project story graph (studio-shell spec §4.1): knot/stitch nodes
    /// plus `END`/`DONE` pseudo-nodes, and divert/choice/tunnel/thread edges.
    /// Function knots and function-call edges are excluded. Node spans and
    /// edge-occurrence spans are UTF-16 offsets in their own file; each edge
    /// lists the divert sites that produced it (#371). Deterministically
    /// ordered (nodes by id, edges by from/to/kind, occurrences by
    /// file/span). Returns JSON `StoryGraph`, or `"null"` when no analysis
    /// is available.
    pub fn story_graph(&self) -> String {
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };
        let db = self.session.db();
        let files: Vec<(brink_ir::FileId, &brink_ir::HirFile)> = db
            .file_ids()
            .filter_map(|id| db.hir(id).map(|hir| (id, hir)))
            .collect();
        let graph = brink_ide::story_graph::story_graph(analysis, &files);

        let nodes: Vec<StoryGraphNodeJs> = graph
            .nodes
            .into_iter()
            .map(|n| {
                let (file, start, end) = match (n.file, n.range) {
                    (Some(f), Some(r)) => {
                        let src = db.source(f).unwrap_or("");
                        (
                            db.file_path(f).map(str::to_owned),
                            Some(byte_to_utf16(src, r.start().into())),
                            Some(byte_to_utf16(src, r.end().into())),
                        )
                    }
                    _ => (None, None, None),
                };
                StoryGraphNodeJs {
                    id: n.id,
                    name: n.name,
                    kind: story_node_kind_str(n.kind),
                    file,
                    start,
                    end,
                    parent: n.parent,
                }
            })
            .collect();
        let edges: Vec<StoryGraphEdgeJs> = graph
            .edges
            .into_iter()
            .map(|e| StoryGraphEdgeJs {
                from: e.from,
                to: e.to,
                kind: story_edge_kind_str(e.kind),
                occurrences: e
                    .occurrences
                    .iter()
                    .filter_map(|o| {
                        let file = db.file_path(o.file)?.to_owned();
                        let src = db.source(o.file).unwrap_or("");
                        Some(StoryGraphEdgeOccurrenceJs {
                            file,
                            start: byte_to_utf16(src, o.range.start().into()),
                            end: byte_to_utf16(src, o.range.end().into()),
                        })
                    })
                    .collect(),
            })
            .collect();

        serde_json::to_string(&StoryGraphJs { nodes, edges }).unwrap_or_default()
    }

    /// Compute per-line context from the HIR. Returns JSON array of `LineContext`.
    pub fn line_contexts(&self) -> String {
        self.line_contexts_impl(&self.active_path, self.view.as_ref())
    }

    /// Compute semantic tokens. Returns JSON array of tokens.
    pub fn semantic_tokens(&self) -> String {
        self.semantic_tokens_impl(&self.active_path, self.view.as_ref())
    }

    /// Compute completions at the given byte offset. Returns JSON array.
    pub fn completions(&self, offset: u32) -> String {
        self.completions_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute hover info at the given byte offset. Returns JSON or "null".
    pub fn hover(&self, offset: u32) -> String {
        self.hover_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute goto-definition at the given byte offset. Returns JSON or "null".
    pub fn goto_definition(&self, offset: u32) -> String {
        self.goto_definition_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Find all references. Returns JSON array.
    pub fn find_references(&self, offset: u32) -> String {
        self.find_references_impl(&self.active_path, self.view.as_ref(), offset, true)
    }

    /// Find all references at an explicit file path + offset, with control over
    /// whether the declaration itself is included. Document-agnostic: resolves
    /// the file by `path` against the session, not the active document. Returns
    /// a JSON `Location[]` array (`"[]"` if the path or analysis is unavailable).
    pub fn find_references_at(&self, path: &str, offset: u32, include_declaration: bool) -> String {
        self.find_references_impl(path, None, offset, include_declaration)
    }

    /// Find all references to a symbol identified by its canonical name. Resolves
    /// the symbol via the analysis index; returns `"[]"` (fail-safe, deterministic)
    /// if the name is unknown or ambiguous (more than one matching definition).
    /// Otherwise locates the symbol's declaration (file + range start) and returns
    /// its references as a JSON `Location[]` array.
    pub fn references_to_symbol(&self, symbol_name: &str, include_declaration: bool) -> String {
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };
        // Resolve the symbol name to a single definition. Unknown or ambiguous
        // names fail safe to an empty result rather than guessing.
        let ids = match analysis.index.by_name.get(symbol_name) {
            Some(ids) if ids.len() == 1 => ids,
            _ => return "[]".to_owned(),
        };
        let Some(info) = analysis.index.symbols.get(&ids[0]) else {
            return "[]".to_owned();
        };
        let Some(path) = self.session.file_path(info.file) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(info.file) else {
            return "[]".to_owned();
        };
        // The impl expects a UTF-16, view-relative offset; with no view that is
        // the file-absolute UTF-16 offset of the declaration's name start.
        let offset = byte_to_utf16(source, info.range.start().into());
        let path = path.to_owned();
        self.find_references_impl(&path, None, offset, include_declaration)
    }

    /// Check if rename is possible. Returns JSON or "null".
    pub fn prepare_rename(&self, offset: u32) -> String {
        self.prepare_rename_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute code actions. Returns JSON array.
    pub fn code_actions(&self, offset: u32) -> String {
        self.code_actions_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Apply a code action selected from [`code_actions`](Self::code_actions).
    ///
    /// `data_json` is the `data` field of a `CodeAction` (the self-describing,
    /// internally-tagged discriminator). `offset` is the cursor position the
    /// action was offered at — unused for the source-level actions (format /
    /// sort / structural move) but accepted for parity with the other queries
    /// and so future cursor-scoped actions need no signature change.
    ///
    /// Returns `StructuralResult`-shaped JSON: `new_source` for the primary file plus
    /// any `cross_file_edits` for structural moves, or `ok: false` with an
    /// `error` when the data is malformed or the action is a no-op.
    pub fn resolve_code_action(&self, data_json: &str, offset: u32) -> String {
        self.resolve_code_action_impl(&self.active_path, self.view.as_ref(), data_json, offset)
    }

    /// Document-handle variant of [`resolve_code_action`](Self::resolve_code_action).
    pub fn resolve_code_action_doc(&self, doc: u32, data_json: &str, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return error_json("unknown document handle");
        };
        self.resolve_code_action_impl(&d.path, d.view.as_ref(), data_json, offset)
    }

    /// Compute inlay hints. Returns JSON array.
    pub fn inlay_hints(&self, start: u32, end: u32) -> String {
        self.inlay_hints_impl(&self.active_path, self.view.as_ref(), start, end)
    }

    /// Compute signature help. Returns JSON or "null".
    pub fn signature_help(&self, offset: u32) -> String {
        self.signature_help_impl(&self.active_path, self.view.as_ref(), offset)
    }

    /// Compute folding ranges. Returns JSON array.
    pub fn folding_ranges(&self) -> String {
        self.folding_ranges_impl(&self.active_path, self.view.as_ref())
    }

    /// Compute document symbols (outline). Returns JSON array.
    pub fn document_symbols(&self) -> String {
        self.document_symbols_impl(&self.active_path)
    }

    /// Convert a line element to a different type. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element(&self, offset: u32, target: &str) -> String {
        self.convert_element_impl(&self.active_path, self.view.as_ref(), offset, target)
    }

    /// Get resolved INCLUDE paths for a file. Returns JSON `[{path, resolved, loaded}]`.
    pub fn file_includes(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(hir) = self.session.hir(file_id) else {
            return "[]".to_owned();
        };

        let db = self.session.db();
        let items: Vec<IncludeInfoJs> = hir
            .includes
            .iter()
            .map(|inc| {
                let resolved = brink_db::resolve_include_path(path, &inc.file_path);
                let loaded = db.file_id(&resolved).is_some();
                IncludeInfoJs {
                    path: inc.file_path.clone(),
                    resolved,
                    loaded,
                }
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Format the document (sort knots). Returns the formatted source as a JSON string.
    pub fn format_document(&self) -> String {
        self.format_document_impl(&self.active_path)
    }

    /// Reorder a stitch within its parent knot. Returns JSON `StructuralResult` or error string.
    ///
    /// `path`: file containing the knot.
    /// `direction`: 1 = down, -1 = up.
    pub fn reorder_stitch(&self, path: &str, knot: &str, stitch: &str, direction: i32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let dir = if direction >= 0 {
            brink_ide::structural_move::Direction::Down
        } else {
            brink_ide::structural_move::Direction::Up
        };

        match brink_ide::structural_move::reorder_stitch(source, knot, stitch, dir) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Move a stitch from one knot to another. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing both knots.
    pub fn move_stitch(&self, path: &str, src_knot: &str, stitch: &str, dest_knot: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::move_stitch(
            source, analysis, file_id, src_knot, stitch, dest_knot,
        ) {
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Rename or move a file, rewriting every `INCLUDE` that resolves to it
    /// (inbound) plus the moved file's own relative includes (outbound).
    /// Returns JSON `StructuralResult` or error: `new_source` is the moved file's
    /// content to write at `new`, `cross_file_edits` carry the referencing
    /// files' rewrites. The op computes edits only — the caller applies them
    /// (write `new`, remove `old`).
    pub fn rename_file(&self, old: &str, new: &str) -> String {
        match brink_ide::file_rename::rename_file(&self.session, old, new) {
            Ok(result) => structural_result_json(&self.session, &result, old),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Atomically rename or move a directory (#314): relocate every file under
    /// `old_prefix` to `new_prefix`, rewriting all affected `INCLUDE`s against a
    /// single pre-move snapshot (moved files' outbound includes, inbound includes
    /// from files outside the folder, and intra-folder sibling includes — all
    /// mutually consistent). Returns JSON `DirMoveResult`: `moved_files` are the
    /// relocated files (`old_path`, `new_path`, rewritten `new_source`),
    /// `cross_file_edits` carry the outside referrers' rewrites. `safe` +
    /// `introduced_diagnostics` are the shared safe-by-default breakage gate. The
    /// op computes edits only — the caller writes the new files, removes the old
    /// ones, and applies the inbound edits.
    pub fn rename_dir(&self, old_prefix: &str, new_prefix: &str) -> String {
        match brink_ide::dir_rename::rename_dir(&self.session, old_prefix, new_prefix) {
            Ok(result) => dir_move_result_json(&self.session, &result),
            Err(e) => dir_error_json(&e.to_string()),
        }
    }

    /// Ensure `current` `INCLUDE`s `target` (#312 F core).
    ///
    /// Returns JSON `{ ok, already_reachable, edit?: TextEdit, error? }`. When
    /// `target` is already reachable from `current`'s INCLUDE graph the op is a
    /// no-op (`already_reachable: true`, no `edit`). Otherwise `edit` is the
    /// byte-range insertion the caller applies to `current`'s source.
    pub fn auto_import_include(&self, current: &str, target: &str) -> String {
        let resp = match brink_ide::auto_import::ensure_include(&self.session, current, target) {
            Ok(result) => AutoImportJs {
                ok: true,
                already_reachable: result.already_reachable,
                edit: result.edit,
                error: None,
            },
            Err(e) => AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some(e.to_string()),
            },
        };
        serde_json::to_string(&resp).unwrap_or_default()
    }

    /// Auto-import `target` into the file backing document handle `doc` (#312 F,
    /// completion-accept path). Same `{ ok, already_reachable, edit?, error? }`
    /// shape as [`auto_import_include`], but the edit's `from`/`to` are
    /// **whole-file UTF-16** offsets (the INCLUDE block is a whole-file concept
    /// regardless of a fragment view), so the editor can apply it to the file
    /// source directly. Idempotent — no edit when `target` is already reachable.
    pub fn auto_import_include_doc(&self, doc: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("unknown document handle".to_owned()),
            })
            .unwrap_or_default();
        };
        let current = d.path.clone();
        let resp = match brink_ide::auto_import::ensure_include(&self.session, &current, target) {
            Ok(result) => {
                // Convert the byte-offset edit to whole-file UTF-16 so it can be
                // applied against the file source (or a whole-file view).
                let edit = result.edit.and_then(|e| {
                    let source = self.source_of(&current)?;
                    Some(brink_ide::line_convert::TextEdit {
                        from: byte_to_utf16(source, e.from),
                        to: byte_to_utf16(source, e.to),
                        insert: e.insert,
                    })
                });
                AutoImportJs {
                    ok: true,
                    already_reachable: result.already_reachable,
                    edit,
                    error: None,
                }
            }
            Err(e) => AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some(e.to_string()),
            },
        };
        serde_json::to_string(&resp).unwrap_or_default()
    }

    /// Auto-import `target` into the file backing document handle `doc` **and
    /// apply the INCLUDE edit out-of-band**, rebasing every open fragment view
    /// on that file (#312 F, fragment-view completion-accept path).
    ///
    /// A fragment (symbol-tab / "play from here") view cannot dispatch the
    /// whole-file INCLUDE edit into its own CM document — the INCLUDE lives
    /// above the fragment. So the caller applies it here. A raw whole-file
    /// replace ([`update_file`]) would prepend the INCLUDE but leave every open
    /// fragment handle's stored `ViewContext` pointing at pre-shift byte
    /// offsets, so the next fragment splice would clobber the INCLUDE line and
    /// surrounding content. This method inserts the INCLUDE *and* shifts the
    /// byte range (and start line) of every fragment view on the file that
    /// begins at/after the insertion point, keeping them consistent.
    ///
    /// Returns the same `{ ok, already_reachable, edit?, error? }` shape as
    /// [`auto_import_include_doc`]. On success the `edit` (whole-file UTF-16)
    /// **describes the shift that was already applied** — the caller must NOT
    /// re-apply it; it exists only so the caller can rebase its own TS-side
    /// fragment-range mirror by the UTF-16 delta before inserting the symbol
    /// text into the fragment view. When `target` is already reachable this is
    /// a no-op (`already_reachable: true`, no `edit`).
    pub fn auto_import_apply_include_doc(&mut self, doc: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("unknown document handle".to_owned()),
            })
            .unwrap_or_default();
        };
        let current = d.path.clone();
        let result = match brink_ide::auto_import::ensure_include(&self.session, &current, target) {
            Ok(result) => result,
            Err(e) => {
                return serde_json::to_string(&AutoImportJs {
                    ok: false,
                    already_reachable: false,
                    edit: None,
                    error: Some(e.to_string()),
                })
                .unwrap_or_default();
            }
        };

        // Already reachable, or no edit produced: nothing to apply.
        let Some(edit) = result.edit.filter(|_| !result.already_reachable) else {
            return serde_json::to_string(&AutoImportJs {
                ok: true,
                already_reachable: result.already_reachable,
                edit: None,
                error: None,
            })
            .unwrap_or_default();
        };

        // `ensure_include` returns byte offsets for `from`/`to` into the current
        // file source. Apply the insertion to the whole-file source.
        let Some(source) = self.source_of(&current).map(str::to_owned) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("current file source unavailable".to_owned()),
            })
            .unwrap_or_default();
        };
        let from = (edit.from as usize).min(source.len());
        let to = (edit.to as usize).clamp(from, source.len());
        let mut merged = String::with_capacity(source.len() + edit.insert.len());
        merged.push_str(&source[..from]);
        merged.push_str(&edit.insert);
        merged.push_str(&source[to..]);

        // Rebase every open fragment view on this file whose range starts at or
        // after the insertion point. The edit removes `to - from` bytes and
        // inserts `edit.insert`, so downstream offsets shift by the net delta;
        // start lines shift by (inserted newlines − removed newlines).
        #[expect(
            clippy::cast_possible_wrap,
            reason = "ink files are always < 4GB, so byte counts fit i64"
        )]
        let byte_delta = edit.insert.len() as i64 - (to - from) as i64;
        let removed_newlines = count_newlines(&source[from..to]);
        let inserted_newlines = count_newlines(&edit.insert);
        let line_delta = i64::from(inserted_newlines) - i64::from(removed_newlines);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "ink files are always < 4GB"
        )]
        let insert_at = from as u32;
        for state in self.docs.values_mut() {
            if state.path != current {
                continue;
            }
            let Some(view) = state.view.as_mut() else {
                continue;
            };
            rebase_view(view, insert_at, byte_delta, line_delta);
        }

        // The whole-file UTF-16 edit that was applied, so the caller can rebase
        // its own TS-side fragment range mirror by the UTF-16 delta. This edit
        // is NOT for the caller to re-apply (it is already applied) — it merely
        // describes the shift.
        let applied_edit = brink_ide::line_convert::TextEdit {
            from: byte_to_utf16(&source, edit.from),
            to: byte_to_utf16(&source, edit.to),
            insert: edit.insert,
        };

        self.session.update_and_analyze(&current, merged);

        serde_json::to_string(&AutoImportJs {
            ok: true,
            already_reachable: false,
            edit: Some(applied_edit),
            error: None,
        })
        .unwrap_or_default()
    }

    /// Promote a stitch to a top-level knot. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing the knot.
    pub fn promote_stitch(&self, path: &str, knot: &str, stitch: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::promote_stitch_to_knot(
            source, analysis, file_id, knot, stitch,
        ) {
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder a knot within the top-level knot list. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing the knot.
    /// `direction`: 1 = down, -1 = up.
    pub fn reorder_knot(&self, path: &str, knot: &str, direction: i32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let dir = if direction >= 0 {
            brink_ide::structural_move::Direction::Down
        } else {
            brink_ide::structural_move::Direction::Up
        };

        match brink_ide::structural_move::reorder_knot(source, knot, dir) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder all stitches in a knot to match `order` (a permutation of the
    /// knot's stitch names). Used by drag-and-drop and multi-select moves,
    /// which know the full destination order. Returns JSON `StructuralResult` or error.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen requires owned Vec<String> across the boundary"
    )]
    pub fn reorder_stitches(&self, path: &str, knot: &str, order: Vec<String>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        match brink_ide::structural_move::reorder_stitches(source, knot, &order) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder all top-level knots to match `order` (a permutation of the knot
    /// names). Returns JSON `StructuralResult` or error.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen requires owned Vec<String> across the boundary"
    )]
    pub fn reorder_knots(&self, path: &str, order: Vec<String>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        match brink_ide::structural_move::reorder_knots(source, &order) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Demote a top-level knot to a stitch inside another knot. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing both knots.
    pub fn demote_knot(&self, path: &str, knot: &str, dest_knot: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::demote_knot_to_stitch(
            source, analysis, file_id, knot, dest_knot,
        ) {
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Delete a knot (`stitch` empty) or a stitch, safe-by-default (#316).
    ///
    /// Removes the knot's whole region (header, body, nested stitches) or the
    /// named stitch's region, then runs the breakage gate: every divert /
    /// thread / tunnel / call that targeted the removed symbol now dangles, and
    /// those introduced diagnostics travel out so the caller can show a breakage
    /// report and apply the delete only on an explicit force. Returns the
    /// unified `StructuralResult` JSON (`new_source` for `path`, `safe`,
    /// `introduced_diagnostics`) or an error.
    pub fn delete_symbol(&self, path: &str, knot: &str, stitch: &str) -> String {
        let stitch = (!stitch.is_empty()).then_some(stitch);
        match brink_ide::structural_delete::delete_symbol(&self.session, path, knot, stitch) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Extract the selected lines into a new top-level `=== name ===` knot,
    /// replacing the selection with a tunnel call `-> name ->` (#315 H).
    ///
    /// `start_offset`/`end_offset` are whole-file UTF-16 offsets into `path`'s
    /// source (converted to bytes here). The selection is snapped to whole lines;
    /// the new knot is appended at end of file and ends with a `->->` return.
    /// Returns the unified `StructuralResult` JSON — `safe` is false and
    /// `introduced_diagnostics` is populated when the extraction pulls a
    /// weave/gather label or a local/temp reference out of scope. On failure a
    /// `StructuralResult`-shaped error is returned.
    pub fn extract_to_knot(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        name: &str,
    ) -> String {
        let Some(source) = self.source_of(path) else {
            return error_json("file not loaded");
        };
        let start = utf16_to_byte(source, start_offset) as usize;
        let end = utf16_to_byte(source, end_offset) as usize;
        match brink_ide::extract::extract_to_knot(&self.session, path, start, end, name) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Extract the selected lines into a new `=== function name() ===`, replacing
    /// the selection with the call — `{name()}` for a single value expression,
    /// `~ name()` for a statement (#315 H). Same offset/gate semantics as
    /// [`extract_to_knot`](Self::extract_to_knot).
    pub fn extract_to_function(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        name: &str,
    ) -> String {
        let Some(source) = self.source_of(path) else {
            return error_json("file not loaded");
        };
        let start = utf16_to_byte(source, start_offset) as usize;
        let end = utf16_to_byte(source, end_offset) as usize;
        match brink_ide::extract::extract_to_function(&self.session, path, start, end, name) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Rename a knot or stitch by name, safe-by-default. Returns a
    /// `StructuralResult`-shaped JSON payload (`new_source` for `path`,
    /// `cross_file_edits` for referencing files) extended with
    /// `introduced_diagnostics` and a `safe` flag. When `safe` is false the
    /// rename would introduce the listed diagnostics — the caller shows a
    /// breakage report and applies the (already-computed) edits only on an
    /// explicit force. An empty `stitch` renames the knot itself.
    pub fn rename_symbol(&self, path: &str, knot: &str, stitch: &str, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(hir) = self.session.hir(file_id) else {
            return error_json("no analysis");
        };
        let stitch = (!stitch.is_empty()).then_some(stitch);
        let Some(offset) = brink_ide::rename::declaration_offset(hir, knot, stitch) else {
            return error_json("symbol not found");
        };
        match brink_ide::rename::rename_safe(&self.session, file_id, offset, new_name) {
            Some(result) => structural_result_json(&self.session, &result, path),
            None => error_json("cannot rename this symbol"),
        }
    }

    /// Rename the symbol at a UTF-16 **file** offset, safe-by-default — the
    /// offset-based sibling of `rename_symbol`, used by the editor's F2 (which
    /// resolves any symbol under the cursor, not just knots/stitches). Returns
    /// the same `RenameResultJs` payload. The offset is a whole-file UTF-16
    /// offset (the caller folds any fragment-view origin in); it is converted
    /// to a byte offset here.
    pub fn rename_symbol_at(&self, path: &str, offset: u32, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let abs_offset = self.to_absolute(path, None, offset);
        match brink_ide::rename::rename_safe(
            &self.session,
            file_id,
            TextSize::new(abs_offset),
            new_name,
        ) {
            Some(result) => structural_result_json(&self.session, &result, path),
            None => error_json("cannot rename this symbol"),
        }
    }
}

// ── View context helpers (private, not wasm-exported) ───────────────
//
// Every IDE query is parameterized over `(path, view)` — the file being
// addressed and an optional sub-file view context. The legacy singleton API
// passes `(active_path, view)`; the document-handle API passes the handle's
// entry. Both funnel into the same `*_impl` bodies below.

impl EditorSession {
    /// Allocate the next handle id and insert `state`. Ids are monotonically
    /// increasing and never reused within a session.
    fn insert_doc(&mut self, state: DocState) -> u32 {
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        self.docs.insert(id, state);
        id
    }

    /// Source text of a file, if loaded.
    fn source_of(&self, path: &str) -> Option<&str> {
        self.session
            .file_id(path)
            .and_then(|id| self.session.source(id))
    }

    /// Convert a UTF-16 view-relative offset (the boundary convention) to a
    /// file-absolute **byte** offset for `brink-ide`/rowan.
    ///
    /// When a view context is given the offset is relative to the displayed
    /// fragment (`source[view.start..view.end]`); otherwise it's relative to
    /// the whole file.
    fn to_absolute(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> u32 {
        let Some(source) = self.source_of(path) else {
            return offset;
        };
        match view {
            Some(v) => {
                let start = floor_char_boundary(source, v.start as usize);
                let end = floor_char_boundary(source, (v.end as usize).max(start));
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink files are always < 4GB"
                )]
                let abs = start as u32 + utf16_to_byte(&source[start..end], offset);
                abs
            }
            None => utf16_to_byte(source, offset),
        }
    }

    /// Convert a file-absolute **byte** offset (from `brink-ide`) to a
    /// UTF-16 view-relative offset for the editor.
    /// Returns `None` if the offset is outside the view range.
    fn to_relative(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> Option<u32> {
        let source = self.source_of(path)?;
        match view {
            Some(v) => {
                if offset < v.start || offset > v.end {
                    return None;
                }
                let start = floor_char_boundary(source, v.start as usize);
                let end = floor_char_boundary(source, (v.end as usize).max(start));
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink files are always < 4GB"
                )]
                let byte_in_fragment = (offset as usize).saturating_sub(start) as u32;
                Some(byte_to_utf16(&source[start..end], byte_in_fragment))
            }
            None => Some(byte_to_utf16(source, offset)),
        }
    }

    /// Convert a file-absolute line number (0-based) to a view-relative line.
    /// Returns `None` if the line is before the view start.
    fn to_relative_line(view: Option<&ViewContext>, line: u32) -> Option<u32> {
        view.map_or(Some(line), |v| {
            (line >= v.start_line).then(|| line - v.start_line)
        })
    }

    /// Compute the end line of the view in the current source.
    fn view_end_line(&self, path: &str, view: &ViewContext) -> Option<u32> {
        let source = self.source_of(path)?;
        let byte_end = (view.end as usize).min(source.len());
        Some(count_newlines(&source[..byte_end]))
    }

    /// Compute a `ViewContext` scoping `path` to `[start, end)` (UTF-16
    /// offsets): converts the boundary offsets to bytes, trims trailing blank
    /// lines (keeping at most one newline), detects the newline separator at
    /// the boundary, and records the 0-based start line.
    fn compute_view_context(&self, path: &str, start: u32, end: u32) -> ViewContext {
        // Boundary offsets are UTF-16 code units; convert to bytes for the
        // internal byte-indexed logic below (and stored ViewContext range).
        let (start, end) = match self.source_of(path) {
            Some(s) => (utf16_to_byte(s, start), utf16_to_byte(s, end)),
            None => (start, end),
        };
        // Check if there's a newline right at `end` (the separator between this
        // section and the next). If so, we'll ensure it's preserved after splices.
        // Trim trailing blank lines from the view range and check if there's a
        // newline separator at the boundary that should be preserved across splices.
        let (end, trailing_newline) = self.source_of(path).map_or((end, false), |s| {
            let e = (end as usize).min(s.len());
            let start_usize = (start as usize).min(e);
            let view = &s[start_usize..e];
            // Trim trailing newlines (keep at most one)
            let trimmed = view.trim_end_matches('\n');
            let keep = if trimmed.len() < view.len() {
                trimmed.len() + 1
            } else {
                view.len()
            };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink files are always < 4GB"
            )]
            let trimmed_end = (start_usize + keep).min(e) as u32;
            // Check if there's a newline right after the trimmed end
            let has_nl = s.as_bytes().get(trimmed_end as usize) == Some(&b'\n')
                || (trimmed_end > 0
                    && s.as_bytes().get((trimmed_end as usize).wrapping_sub(1)) == Some(&b'\n'));
            (trimmed_end, has_nl)
        });

        let start_line = self.source_of(path).map_or(0, |s| {
            let byte_start = (start as usize).min(s.len());
            count_newlines(&s[..byte_start])
        });
        ViewContext {
            start,
            end,
            start_line,
            trailing_newline,
        }
    }
}

// ── IDE query implementations (private, parameterized) ──────────────

impl EditorSession {
    fn line_contexts_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source), Some(root)) = (
            self.session.hir(file_id),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let contexts = match self.session.dialect() {
            Some(dialect) => {
                brink_ide::line_context::line_contexts_with_dialect(hir, source, &root, dialect)
            }
            None => brink_ide::line_context::line_contexts(hir, source, &root),
        };
        if let Some(v) = view {
            let start = v.start_line as usize;
            let end_line = self
                .view_end_line(path, v)
                .map_or(contexts.len(), |l| l as usize);
            let slice = &contexts[start..end_line.min(contexts.len())];
            serde_json::to_string(slice).unwrap_or_default()
        } else {
            serde_json::to_string(&contexts).unwrap_or_default()
        }
    }

    fn semantic_tokens_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source), Some(root)) = (
            self.session.analysis(),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let raw = brink_ide::semantic_tokens::semantic_tokens(source, &root, analysis, file_id);

        let tokens: Vec<TokenJs> = raw
            .iter()
            .filter_map(|t| {
                let line = Self::to_relative_line(view, t.line)?;
                Some(TokenJs {
                    line,
                    start_char: t.start_char,
                    length: t.length,
                    token_type: t.token_type,
                    modifiers: t.modifiers,
                })
            })
            .collect();

        serde_json::to_string(&tokens).unwrap_or_default()
    }

    fn completions_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let ctx = brink_ide::detect_completion_context(source, abs_offset as usize);
        let scope = brink_ide::cursor_scope(source, abs_offset as usize);

        // Auto-import (#312 F): symbols declared in files NOT reachable from the
        // current file's INCLUDE graph are still offered, but tagged as
        // out-of-scope so the editor can render a "from <file>" affordance and
        // insert the INCLUDE on accept. Reachability includes the current file
        // itself; locals (params/temps) carry no owning importable file.
        let reachable = self
            .session
            .file_id(path)
            .map(|id| self.session.db().reachable_from(id));

        let symbol_items = analysis
            .index
            .symbols
            .values()
            .filter(|info| brink_ide::is_visible_in_context(&ctx, info, &scope))
            .map(|info| {
                let is_local = matches!(
                    info.kind,
                    brink_ir::SymbolKind::Param | brink_ir::SymbolKind::Temp
                );
                // A symbol is out of scope when its declaring file is not
                // reachable from the current file. Locals are never imported.
                let out_of_scope = !is_local
                    && reachable
                        .as_ref()
                        .is_some_and(|set| !set.contains(&info.file));
                let source_file = if out_of_scope {
                    self.session.file_path(info.file).map(str::to_owned)
                } else {
                    None
                };
                CompletionItemJs {
                    name: info.name.clone(),
                    kind: symbol_kind_str(info.kind).to_owned(),
                    // Callables get a typed signature from /// docs or the host
                    // manifest, if any; otherwise the kind-derived detail.
                    detail: typed_detail(analysis, info).or_else(|| info.detail.clone()),
                    insert: None,
                    out_of_scope,
                    source_file,
                }
            });

        // Host value picker (#174): in an argument slot whose param has a value
        // source, offer its labelled values first (display the label, insert the
        // literal) — static items from the manifest, or `host` items from the
        // pushed cache.
        let mut items: Vec<CompletionItemJs> = Vec::new();
        if matches!(ctx, brink_ide::CompletionContext::FunctionArgs) {
            items.extend(
                brink_ide::signature::argument_value_completions(
                    analysis,
                    source,
                    abs_offset as usize,
                    Some(self.session.host_values()),
                )
                .into_iter()
                .map(|v| CompletionItemJs {
                    name: v.label,
                    kind: "value".to_owned(),
                    detail: v.detail,
                    insert: Some(v.value),
                    out_of_scope: false,
                    source_file: None,
                }),
            );
        }

        // Multiple definitions of one name (#312 F): when a name is declared in
        // several out-of-scope files, keep only the nearest by relative-path
        // distance so the auto-import targets a single deterministic file. In-
        // scope duplicates (already reachable) are left untouched — they insert
        // no INCLUDE. `dedupe_out_of_scope` sorts, so the result is stable.
        let symbol_items = dedupe_out_of_scope(path, symbol_items.collect());
        items.extend(symbol_items);

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn hover_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let project_files = [(file_id, path.to_owned(), source.to_owned())];

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::hover::hover(
            analysis,
            file_id,
            source,
            TextSize::new(abs_offset),
            &project_files,
        ) {
            Some(info) => {
                let js = HoverInfoJs {
                    content: info.content,
                    start: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.start().into())),
                    end: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.end().into())),
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    fn goto_definition_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::navigation::goto_definition(analysis, file_id, TextSize::new(abs_offset)) {
            Some(loc) => {
                let db = self.session.db();
                let file_path = db.file_path(loc.file).unwrap_or_default().to_owned();
                let (start, end) = if loc.file == file_id {
                    // Same file: adjust to view-relative UTF-16 offsets
                    (
                        self.to_relative(path, view, loc.range.start().into())
                            .unwrap_or(loc.range.start().into()),
                        self.to_relative(path, view, loc.range.end().into())
                            .unwrap_or(loc.range.end().into()),
                    )
                } else {
                    // Cross-file: convert bytes to UTF-16 in the target file
                    let src = self.session.source(loc.file).unwrap_or("");
                    (
                        byte_to_utf16(src, loc.range.start().into()),
                        byte_to_utf16(src, loc.range.end().into()),
                    )
                };
                let js = LocationJs {
                    file: file_path,
                    start,
                    end,
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    fn find_references_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        offset: u32,
        include_declaration: bool,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let refs = brink_ide::navigation::find_references(
            analysis,
            file_id,
            TextSize::new(abs_offset),
            include_declaration,
        );

        let db = self.session.db();
        let items: Vec<LocationJs> = refs
            .iter()
            .filter_map(|loc| {
                if loc.file == file_id {
                    // Same file: adjust offsets, filter out-of-view
                    let start = self.to_relative(path, view, loc.range.start().into())?;
                    let end = self.to_relative(path, view, loc.range.end().into())?;
                    Some(LocationJs {
                        file: db.file_path(loc.file).unwrap_or_default().to_owned(),
                        start,
                        end,
                    })
                } else {
                    // Cross-file: convert bytes to UTF-16 in the target file
                    let src = self.session.source(loc.file).unwrap_or("");
                    Some(LocationJs {
                        file: db.file_path(loc.file).unwrap_or_default().to_owned(),
                        start: byte_to_utf16(src, loc.range.start().into()),
                        end: byte_to_utf16(src, loc.range.end().into()),
                    })
                }
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn prepare_rename_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::rename::prepare_rename(analysis, file_id, TextSize::new(abs_offset)) {
            Some(range) => {
                let start = self.to_relative(path, view, range.start().into());
                let end = self.to_relative(path, view, range.end().into());
                match (start, end) {
                    (Some(s), Some(e)) => {
                        let js = LocationJs {
                            file: path.to_owned(),
                            start: s,
                            end: e,
                        };
                        serde_json::to_string(&js).unwrap_or_default()
                    }
                    _ => "null".to_owned(),
                }
            }
            None => "null".to_owned(),
        }
    }

    fn code_actions_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let actions = brink_ide::code_actions::code_actions(source, abs_offset as usize);

        let items: Vec<CodeActionJs> = actions
            .iter()
            .map(|a| CodeActionJs {
                title: a.title.clone(),
                kind: code_action_kind_str(&a.kind).to_owned(),
                data: serde_json::to_value(&a.data).unwrap_or(serde_json::Value::Null),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn resolve_code_action_impl(
        &self,
        path: &str,
        _view: Option<&ViewContext>,
        data_json: &str,
        _offset: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let data: brink_ide::code_actions::CodeActionData = match serde_json::from_str(data_json) {
            Ok(d) => d,
            Err(e) => return error_json(&format!("invalid code-action data: {e}")),
        };

        // Structural moves (move / promote / demote) need analysis context;
        // everything else (format / sort / reorder) is a pure source rewrite.
        if let Some(analysis) = self.session.analysis()
            && let Some(result) =
                brink_ide::code_actions::resolve_structural_action(source, analysis, file_id, &data)
        {
            return gated_move_json(&self.session, result, path);
        }

        match brink_ide::code_actions::resolve_code_action(source, &data) {
            Some(new_source) => move_result_json_simple(new_source, path),
            None => error_json("code action produced no change"),
        }
    }

    fn inlay_hints_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::inlay_hints::inlay_hints(
            &root,
            analysis,
            range,
            Some(self.session.host_values()),
        );

        let items: Vec<InlayHintJs> = hints
            .iter()
            .filter_map(|h| {
                let offset = self.to_relative(path, view, h.offset.into())?;
                Some(InlayHintJs {
                    offset,
                    label: h.label.clone(),
                    kind: inlay_hint_kind_str(&h.kind).to_owned(),
                    padding_right: h.padding_right,
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn color_hints_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::color::color_hints(&root, analysis, range);

        let items: Vec<ColorHintJs> = hints
            .iter()
            .filter_map(|h| {
                let start = self.to_relative(path, view, h.start.into())?;
                let end = self.to_relative(path, view, h.end.into())?;
                Some(ColorHintJs {
                    start,
                    end,
                    value: h.value.clone(),
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn argument_widgets_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        use brink_ide::argument_widgets::SlotState;
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let sites = brink_ide::argument_widgets::argument_widgets(
            &root,
            analysis,
            range,
            Some(self.session.host_values()),
        );

        let out: Vec<CallWidgetSiteJs> = sites
            .iter()
            .map(|site| {
                let slots = site
                    .slots
                    .iter()
                    .map(|slot| {
                        // Map byte offsets to UTF-16; a slot whose offsets fall
                        // outside the view degrades to a non-actionable Expr.
                        let state = match &slot.state {
                            SlotState::Filled { start, end, value } => {
                                match (
                                    self.to_relative(path, view, (*start).into()),
                                    self.to_relative(path, view, (*end).into()),
                                ) {
                                    (Some(start), Some(end)) => SlotStateJs::Filled {
                                        start,
                                        end,
                                        value: value.clone(),
                                    },
                                    _ => SlotStateJs::Expr,
                                }
                            }
                            SlotState::Empty {
                                insert_at,
                                needs_leading_comma,
                            } => match self.to_relative(path, view, (*insert_at).into()) {
                                Some(insert_at) => SlotStateJs::Empty {
                                    insert_at,
                                    needs_leading_comma: *needs_leading_comma,
                                },
                                None => SlotStateJs::Expr,
                            },
                            SlotState::Expr => SlotStateJs::Expr,
                        };
                        SlotWidgetJs {
                            param_name: slot.param_name.clone(),
                            widget: slot.widget.clone(),
                            type_name: slot.type_name.clone(),
                            values: slot
                                .values
                                .iter()
                                .map(|v| ValueItemJs {
                                    value: v.value.clone(),
                                    label: v.label.clone(),
                                    detail: v.detail.clone(),
                                })
                                .collect(),
                            state,
                        }
                    })
                    .collect();
                // The call-name span (UTF-16) anchors the form glyph; default to
                // 0 if it falls outside the view (the studio guards end > start).
                let name_start = self
                    .to_relative(path, view, site.name_start.into())
                    .unwrap_or(0);
                let name_end = self
                    .to_relative(path, view, site.name_end.into())
                    .unwrap_or(0);

                // Arg-group widgets (UTF-16); a group with an out-of-view span is
                // dropped (it stays a per-slot affordance).
                let groups: Vec<GroupWidgetSiteJs> = site
                    .groups
                    .iter()
                    .filter_map(|g| self.group_widget_js(path, view, g))
                    .collect();

                // Declared groups carry no document spans, so they need no view
                // translation — the Form renders them and seeds from `slots`.
                let declared_groups: Vec<DeclaredGroupJs> =
                    site.declared_groups.iter().map(declared_group_js).collect();

                CallWidgetSiteJs {
                    callee: site.callee.clone(),
                    name_start,
                    name_end,
                    slots,
                    groups,
                    declared_groups,
                }
            })
            .collect();

        serde_json::to_string(&out).unwrap_or_default()
    }

    /// Map one arg-group widget to its JSON shape (UTF-16); `None` when a span
    /// falls outside the view (the group degrades to per-slot affordances).
    fn group_widget_js(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        g: &brink_ide::argument_widgets::GroupWidgetSite,
    ) -> Option<GroupWidgetSiteJs> {
        use brink_ide::argument_widgets::GroupState;
        let state = match &g.state {
            GroupState::Filled { spans, values } => {
                let mut js_spans = Vec::with_capacity(spans.len());
                for (s, e) in spans {
                    js_spans.push((
                        self.to_relative(path, view, (*s).into())?,
                        self.to_relative(path, view, (*e).into())?,
                    ));
                }
                GroupStateJs::Filled {
                    spans: js_spans,
                    values: values.clone(),
                }
            }
            GroupState::Empty {
                insert_at,
                needs_leading_comma,
            } => GroupStateJs::Empty {
                insert_at: self.to_relative(path, view, (*insert_at).into())?,
                needs_leading_comma: *needs_leading_comma,
            },
        };
        Some(GroupWidgetSiteJs {
            ty: g.ty.clone(),
            surface: g.surface.clone(),
            param_indices: g.param_indices.clone(),
            param_names: g.param_names.clone(),
            state,
            context: g.context.iter().cloned().collect(),
            context_params: g.context_params.iter().cloned().collect(),
        })
    }

    fn signature_help_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::signature::signature_help(analysis, source, abs_offset as usize) {
            Some(info) => {
                let js = SignatureInfoJs {
                    label: info.label,
                    documentation: info.documentation,
                    parameters: info
                        .parameters
                        .iter()
                        .map(|p| ParamLabelJs {
                            label: p.label.clone(),
                        })
                        .collect(),
                    active_parameter: info.active_parameter,
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    fn folding_ranges_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source)) = (self.session.hir(file_id), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        // Structural folds (#313 G et al.) — never auto-collapsed by a host.
        let mut ranges = brink_ide::folding::folding_ranges(hir, source);

        // Machinery/narrative fold runs (#365): computed from the same
        // per-line classification `line_contexts_impl` exposes, so a
        // registered dialect's declared `nature` (#368) flows into the fold
        // computation exactly as it flows into `line_contexts`.
        if let Some(root) = self.session.syntax_root(file_id) {
            let ctx = match self.session.dialect() {
                Some(dialect) => {
                    brink_ide::line_context::line_contexts_with_dialect(hir, source, &root, dialect)
                }
                None => brink_ide::line_context::line_contexts(hir, source, &root),
            };
            ranges.extend(brink_ide::folding::machinery_and_narrative_folds(
                hir, source, &ctx,
            ));
        }

        let items: Vec<FoldRangeJs> = ranges
            .iter()
            .filter_map(|r| {
                let start_line = Self::to_relative_line(view, r.start_line)?;
                let end_line = Self::to_relative_line(view, r.end_line)?;
                Some(FoldRangeJs {
                    start_line,
                    end_line,
                    collapsed_text: r.collapsed_text.clone(),
                    from_line_start: r.from_line_start,
                    kind: fold_kind_str(r.kind),
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn document_symbols_impl(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(manifest)) =
            (self.session.hir(file_id), self.session.manifest(file_id))
        else {
            return "[]".to_owned();
        };

        let source = self.session.source(file_id).unwrap_or("");
        let syms = brink_ide::document::document_symbols(hir, manifest, source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn convert_element_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        offset: u32,
        target: &str,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let (Some(hir), Some(source), Some(root)) = (
            self.session.hir(file_id),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "null".to_owned();
        };

        let convert_target = match target {
            "narrative" => brink_ide::line_convert::ConvertTarget::Narrative,
            "choice" => brink_ide::line_convert::ConvertTarget::Choice { sticky: false },
            "sticky_choice" => brink_ide::line_convert::ConvertTarget::Choice { sticky: true },
            "gather" => brink_ide::line_convert::ConvertTarget::Gather,
            "choice_body" => brink_ide::line_convert::ConvertTarget::ChoiceBody,
            _ => return "null".to_owned(),
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::line_convert::convert_element(
            source,
            hir,
            &root,
            abs_offset,
            convert_target,
        ) {
            Some(edit) => match (
                self.to_relative(path, view, edit.from),
                self.to_relative(path, view, edit.to),
            ) {
                (Some(from), Some(to)) => {
                    let adjusted = brink_ide::line_convert::TextEdit {
                        from,
                        to,
                        insert: edit.insert,
                    };
                    serde_json::to_string(&adjusted).unwrap_or_default()
                }
                _ => "null".to_owned(),
            },
            None => "null".to_owned(),
        }
    }

    fn format_document_impl(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "\"\"".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "\"\"".to_owned();
        };

        let formatted = brink_ide::sort_knots_in_source(source);
        serde_json::to_string(&formatted).unwrap_or_default()
    }

    fn get_view_source_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let source = self.source_of(path);
        match (source, view) {
            (Some(s), Some(v)) => {
                let start = (v.start as usize).min(s.len());
                let end = (v.end as usize).min(s.len());
                serde_json::to_string(&s[start..end]).unwrap_or_default()
            }
            (Some(s), None) => serde_json::to_string(s).unwrap_or_default(),
            _ => "null".to_owned(),
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn count_newlines(s: &str) -> u32 {
    s.matches('\n').count() as u32
}

// ── UTF-16 ↔ byte offset conversion ─────────────────────────────────
//
// The wasm `EditorSession` boundary speaks UTF-16 code-unit offsets, to
// match CodeMirror / JS string indexing on the TypeScript side. Internally
// (rowan, `TextSize`, `&str`) everything is byte offsets. These helpers
// translate at the boundary. Both clamp to the end of `s` when the input
// falls past the string, and round a position that lands inside a multi-unit
// boundary up to the next char start (CodeMirror never produces such inputs,
// but the clamp keeps us panic-free).

/// Convert a byte offset within `s` to a UTF-16 code-unit offset.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn byte_to_utf16(s: &str, byte: u32) -> u32 {
    let byte = byte as usize;
    let mut units = 0u32;
    for (i, c) in s.char_indices() {
        if i >= byte {
            return units;
        }
        units += c.len_utf16() as u32;
    }
    units
}

/// Convert a UTF-16 code-unit offset within `s` to a byte offset.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn utf16_to_byte(s: &str, utf16: u32) -> u32 {
    let mut units = 0u32;
    for (i, c) in s.char_indices() {
        if units >= utf16 {
            return i as u32;
        }
        units += c.len_utf16() as u32;
    }
    s.len() as u32
}

/// Largest byte index `<= i` that is a char boundary in `s` (clamped to `len`).
/// Keeps fragment slicing panic-free if a stored byte offset ever lands inside
/// a multibyte char.
fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

// ── Fragment view rebasing ──────────────────────────────────────────

/// Shift a fragment `ViewContext` in place to account for an out-of-band
/// whole-file edit that inserted/removed content at byte `insert_at`.
///
/// `byte_delta` is the net byte change of the edit (inserted − removed) and
/// `line_delta` the net newline change. Only views that begin at or after
/// `insert_at` move — a view before the edit is unaffected. This keeps the
/// stored byte range (and start line) of every open fragment handle consistent
/// with the mutated file, so a subsequent fragment splice targets the correct
/// window instead of clobbering the shifted content.
fn rebase_view(view: &mut ViewContext, insert_at: u32, byte_delta: i64, line_delta: i64) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "offsets stay within a <4GB file and never go negative for a valid edit"
    )]
    let shift = |offset: u32| -> u32 {
        if offset < insert_at {
            offset
        } else {
            (i64::from(offset) + byte_delta).max(0) as u32
        }
    };
    let start_moves = view.start >= insert_at;
    // The insertion point sits at (or before) the view start for the auto-import
    // case (INCLUDE block above the fragment), so both boundaries move together.
    view.start = shift(view.start);
    view.end = shift(view.end);
    if start_moves {
        // The view's start byte shifted, so its first line shifts by the net
        // newline delta of the edit.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "line counts stay within a <4GB file"
        )]
        if line_delta >= 0 {
            view.start_line = view.start_line.saturating_add(line_delta as u32);
        } else {
            view.start_line = view.start_line.saturating_sub((-line_delta) as u32);
        }
    }
}

// ── Fragment splicing ───────────────────────────────────────────────

/// Result of splicing a fragment into its full file.
struct SpliceOutcome {
    /// The new full file content.
    spliced: String,
    /// New byte end of the fragment within the file (excludes any separator).
    new_view_end: u32,
    /// Byte start of the replaced range in the old file content.
    replaced_start: u32,
    /// Byte end (exclusive) of the replaced range in the old file content.
    replaced_end: u32,
    /// Whether a `\n` separator was appended after the fragment.
    inserted_separator: bool,
}

/// Splice `source` into `full` over the view's `[start, end)` byte range.
///
/// If the original boundary had a newline separator and the fragment doesn't
/// end with one, a `\n` separator is inserted after the fragment to prevent
/// merging with the next section. `new_view_end` tracks only the fragment,
/// NOT the separator — the separator lives at `spliced[new_view_end]` and is
/// preserved across splices.
fn splice_fragment(full: &str, view: &ViewContext, source: &str) -> SpliceOutcome {
    let start = (view.start as usize).min(full.len());
    let end = (view.end as usize).clamp(start, full.len());

    let after = &full[end..];
    // If the original boundary had a newline separator and the fragment
    // doesn't end with one, insert a newline to prevent merging.
    let needs_sep = view.trailing_newline
        && !source.ends_with('\n')
        && !after.starts_with('\n')
        && !after.is_empty();
    let sep = if needs_sep { "\n" } else { "" };
    let mut spliced = String::with_capacity(start + source.len() + sep.len() + after.len());
    spliced.push_str(&full[..start]);
    spliced.push_str(source);
    spliced.push_str(sep);
    spliced.push_str(after);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ink files are always < 4GB"
    )]
    SpliceOutcome {
        spliced,
        new_view_end: view.start + source.len() as u32,
        replaced_start: start as u32,
        replaced_end: end as u32,
        inserted_separator: needs_sep,
    }
}

// ── Serialization types ─────────────────────────────────────────────

#[derive(Serialize)]
struct ProjectFileJs {
    path: String,
}

/// Change spec returned by `update_document`, describing what actually
/// changed in the underlying file in UTF-16 **file** coordinates. `[start,
/// end)` is the replaced range of the file's previous content. The inserted
/// text is the caller's `source` argument, except when `text` is present —
/// then a fragment splice appended a `\n` separator and `text` carries the
/// actually-inserted text. Consumed by sibling editor views to live-mirror
/// the change as a CM6 change spec.
#[derive(Serialize)]
struct ChangeSpecJs {
    path: String,
    start: u32,
    end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Serialize)]
struct IncludeInfoJs {
    path: String,
    resolved: String,
    loaded: bool,
}

#[derive(Serialize)]
struct FileOutlineJs {
    path: String,
    symbols: Vec<DocumentSymbolJs>,
}

/// Whole-project story graph (spec §4.1) — mirrored as `StoryGraph` in
/// `@brink/wasm-types`.
#[derive(Serialize)]
struct StoryGraphJs {
    nodes: Vec<StoryGraphNodeJs>,
    edges: Vec<StoryGraphEdgeJs>,
}

/// A story-graph node. `file`/`start`/`end` are absent on the `END`/`DONE`
/// pseudo-nodes; `start`/`end` are UTF-16 offsets of the declaration name in
/// `file`. `parent` is the owning knot's id, present on stitches.
#[derive(Serialize)]
struct StoryGraphNodeJs {
    id: String,
    name: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
}

/// A story-graph edge. `occurrences` lists the divert sites that produced
/// it (aggregated edges keep one entry per site); omitted when empty.
#[derive(Serialize)]
struct StoryGraphEdgeJs {
    from: String,
    to: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    occurrences: Vec<StoryGraphEdgeOccurrenceJs>,
}

/// A source site of a story-graph edge: the target path's span (or the
/// whole divert statement for `-> DONE`/`-> END`), as UTF-16 offsets in
/// `file` — the same convention as node spans.
#[derive(Serialize)]
struct StoryGraphEdgeOccurrenceJs {
    file: String,
    start: u32,
    end: u32,
}

#[derive(Serialize)]
struct CompletionItemJs {
    name: String,
    kind: String,
    detail: Option<String>,
    /// Literal to insert when the display label differs from it — host value
    /// picker (#174): show `HarborGate`, insert `5`. `None` ⇒ insert `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    insert: Option<String>,
    /// `true` when the symbol is defined in a file NOT reachable from the
    /// current file's INCLUDE graph (#312 F). The editor tags such rows with a
    /// "from <file>" affordance and, on accept, auto-inserts the INCLUDE.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    out_of_scope: bool,
    /// The project-relative path of the file that declares this symbol, set
    /// only for out-of-scope completions — the auto-import target.
    #[serde(skip_serializing_if = "Option::is_none")]
    source_file: Option<String>,
}

/// Relative-path distance from `current` to `target` (#312 F). Lower is nearer:
/// primarily the number of `..` hops out of the current directory, then total
/// segment count, then the path string for a deterministic final tie-break.
fn include_distance(current: &str, target: &str) -> (usize, usize, String) {
    let rel = brink_db::compute_relative_path(current, target);
    let dotdots = rel.split('/').filter(|s| *s == "..").count();
    let segments = rel.split('/').count();
    (dotdots, segments, rel)
}

/// Collapse multiple out-of-scope definitions of one name down to the single
/// nearest one (#312 F): when a name is offered from several not-yet-reachable
/// files, keep only the closest by [`include_distance`] so the auto-import
/// targets one deterministic file. If a name also has an in-scope definition
/// (already reachable), its out-of-scope variants are dropped entirely — the
/// in-scope row inserts with no INCLUDE. Order is otherwise preserved for
/// in-scope items; the surviving out-of-scope items are stably ordered.
fn dedupe_out_of_scope(current: &str, items: Vec<CompletionItemJs>) -> Vec<CompletionItemJs> {
    use std::collections::HashSet;

    // Names that have at least one in-scope definition — their out-of-scope
    // duplicates are redundant.
    let in_scope_names: HashSet<String> = items
        .iter()
        .filter(|i| !i.out_of_scope)
        .map(|i| i.name.clone())
        .collect();

    // For each out-of-scope name, remember the index of the nearest variant.
    let mut best: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        if !item.out_of_scope || in_scope_names.contains(item.name.as_str()) {
            continue;
        }
        let dist = item
            .source_file
            .as_deref()
            .map(|f| include_distance(current, f));
        let is_better = match best.get(&item.name) {
            None => true,
            Some(&prev) => {
                let prev_dist = items[prev]
                    .source_file
                    .as_deref()
                    .map(|f| include_distance(current, f));
                dist < prev_dist
            }
        };
        if is_better {
            best.insert(item.name.clone(), idx);
        }
    }

    items
        .into_iter()
        .enumerate()
        .filter(|(idx, item)| {
            if !item.out_of_scope {
                return true;
            }
            if in_scope_names.contains(item.name.as_str()) {
                return false;
            }
            best.get(&item.name) == Some(idx)
        })
        .map(|(_, item)| item)
        .collect()
}

/// Build a typed signature detail for a callable (external, knot, stitch)
/// from its symbol metadata, e.g. `(item: bool) -> bool [query]`. `None` when
/// the symbol has no type-bearing metadata, so plain symbols keep their
/// kind-derived detail (e.g. `function`).
fn typed_detail(
    analysis: &brink_analyzer::AnalysisResult,
    info: &brink_ir::SymbolInfo,
) -> Option<String> {
    if !matches!(
        info.kind,
        brink_ir::SymbolKind::External | brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::Stitch
    ) {
        return None;
    }
    let meta = analysis.symbol_meta.get(&info.id)?;
    let has_types = meta.params.iter().any(|p| p.ty.is_some())
        || meta.returns.is_some()
        || meta.kind != brink_ir::ExternalKind::Plain;
    if !has_types {
        return None;
    }
    let params = meta
        .params
        .iter()
        .map(|p| match &p.ty {
            Some(ty) => format!("{}: {}", p.name, ty.name),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = meta
        .returns
        .as_ref()
        .map_or(String::new(), |t| format!(" -> {}", t.name));
    let kind = match meta.kind {
        brink_ir::ExternalKind::Plain => "",
        brink_ir::ExternalKind::Query => " [query]",
        brink_ir::ExternalKind::Effect => " [effect]",
        brink_ir::ExternalKind::Presentation => " [presentation]",
    };
    Some(format!("({params}){ret}{kind}"))
}

#[derive(Serialize)]
struct HoverInfoJs {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<u32>,
}

#[derive(Serialize)]
struct LocationJs {
    file: String,
    start: u32,
    end: u32,
}

#[derive(Serialize)]
struct InlayHintJs {
    offset: u32,
    label: String,
    kind: String,
    padding_right: bool,
}

#[derive(Serialize)]
struct ColorHintJs {
    start: u32,
    end: u32,
    value: String,
}

#[derive(Serialize)]
struct CallWidgetSiteJs {
    callee: String,
    name_start: u32,
    name_end: u32,
    slots: Vec<SlotWidgetJs>,
    groups: Vec<GroupWidgetSiteJs>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    declared_groups: Vec<DeclaredGroupJs>,
}

#[derive(Serialize)]
struct DeclaredGroupJs {
    #[serde(rename = "type")]
    ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    surface: Option<String>,
    param_indices: Vec<u32>,
    param_names: Vec<String>,
    context_params: std::collections::BTreeMap<String, u32>,
}

fn declared_group_js(g: &brink_ide::argument_widgets::DeclaredGroup) -> DeclaredGroupJs {
    DeclaredGroupJs {
        ty: g.ty.clone(),
        surface: g.surface.clone(),
        param_indices: g.param_indices.clone(),
        param_names: g.param_names.clone(),
        context_params: g.context_params.iter().cloned().collect(),
    }
}

#[derive(Serialize)]
struct GroupWidgetSiteJs {
    #[serde(rename = "type")]
    ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    surface: Option<String>,
    param_indices: Vec<u32>,
    param_names: Vec<String>,
    state: GroupStateJs,
    context: std::collections::BTreeMap<String, String>,
    /// Raw key → param-index map (#174) — the Form resolves context from its
    /// live draft values via this, before anything is written to the document.
    context_params: std::collections::BTreeMap<String, u32>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GroupStateJs {
    Filled {
        spans: Vec<(u32, u32)>,
        values: Vec<String>,
    },
    Empty {
        insert_at: u32,
        needs_leading_comma: bool,
    },
}

#[derive(Serialize)]
struct SlotWidgetJs {
    param_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    widget: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    type_name: Option<String>,
    /// Static value-list items (#174) for the Form dropdown; omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    values: Vec<ValueItemJs>,
    state: SlotStateJs,
}

#[derive(Serialize)]
struct ValueItemJs {
    value: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SlotStateJs {
    Filled {
        start: u32,
        end: u32,
        value: String,
    },
    Empty {
        insert_at: u32,
        needs_leading_comma: bool,
    },
    Expr,
}

#[derive(Serialize)]
struct SignatureInfoJs {
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<String>,
    parameters: Vec<ParamLabelJs>,
    active_parameter: u32,
}

#[derive(Serialize)]
struct ParamLabelJs {
    label: String,
}

#[derive(Serialize)]
struct FoldRangeJs {
    start_line: u32,
    end_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    collapsed_text: Option<String>,
    /// Whole-line declaration fold (docs + header + body); the editor folds
    /// from the start of `start_line` and renders a header placeholder.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    from_line_start: bool,
    /// The fold's kind (#365): `"structural"` (everything folding.rs emitted
    /// before #365 — never auto-collapsed), `"machinery"`, or `"narrative"`
    /// (run-based folds over the line classification).
    kind: &'static str,
}

/// `FoldKind` → the wire string the editor's `foldingExtension` switches on.
fn fold_kind_str(kind: brink_ide::folding::FoldKind) -> &'static str {
    match kind {
        brink_ide::folding::FoldKind::Structural => "structural",
        brink_ide::folding::FoldKind::Machinery => "machinery",
        brink_ide::folding::FoldKind::Narrative => "narrative",
    }
}

#[derive(Serialize)]
struct DocumentSymbolJs {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    start: u32,
    end: u32,
    full_start: u32,
    full_end: u32,
    children: Vec<DocumentSymbolJs>,
}

#[derive(Serialize)]
struct CodeActionJs {
    title: String,
    kind: String,
    /// Self-describing, internally-tagged payload (the `action` field is the
    /// discriminator) identifying which transformation this action performs.
    /// Pass it straight back to `resolve_code_action` to apply the action — the
    /// studio never has to reconstruct it from the cursor position.
    data: serde_json::Value,
}

#[derive(Serialize)]
struct TokenJs {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

// ── Helper functions ────────────────────────────────────────────────

fn symbol_kind_str(kind: brink_ir::SymbolKind) -> &'static str {
    match kind {
        brink_ir::SymbolKind::Knot => "knot",
        brink_ir::SymbolKind::Stitch => "stitch",
        brink_ir::SymbolKind::Variable => "variable",
        brink_ir::SymbolKind::Constant => "constant",
        brink_ir::SymbolKind::List => "list",
        brink_ir::SymbolKind::ListItem => "list_item",
        brink_ir::SymbolKind::External => "external",
        brink_ir::SymbolKind::Label => "label",
        brink_ir::SymbolKind::Param => "param",
        brink_ir::SymbolKind::Temp => "temp",
    }
}

fn story_node_kind_str(kind: brink_ide::story_graph::StoryNodeKind) -> &'static str {
    match kind {
        brink_ide::story_graph::StoryNodeKind::Knot => "knot",
        brink_ide::story_graph::StoryNodeKind::Stitch => "stitch",
        brink_ide::story_graph::StoryNodeKind::End => "end",
        brink_ide::story_graph::StoryNodeKind::Done => "done",
    }
}

fn story_edge_kind_str(kind: brink_ide::story_graph::StoryEdgeKind) -> &'static str {
    match kind {
        brink_ide::story_graph::StoryEdgeKind::Divert => "divert",
        brink_ide::story_graph::StoryEdgeKind::Choice => "choice",
        brink_ide::story_graph::StoryEdgeKind::Tunnel => "tunnel",
        brink_ide::story_graph::StoryEdgeKind::Thread => "thread",
    }
}

fn code_action_kind_str(kind: &brink_ide::code_actions::CodeActionKind) -> &'static str {
    match kind {
        brink_ide::code_actions::CodeActionKind::QuickFix => "quickfix",
        brink_ide::code_actions::CodeActionKind::Refactor => "refactor",
        brink_ide::code_actions::CodeActionKind::Source => "source",
    }
}

fn inlay_hint_kind_str(kind: &brink_ide::inlay_hints::InlayHintKind) -> &'static str {
    match kind {
        brink_ide::inlay_hints::InlayHintKind::Parameter => "parameter",
        brink_ide::inlay_hints::InlayHintKind::Value => "value",
    }
}

/// Convert a compiler diagnostic to JSON, translating its byte range to UTF-16
/// offsets against `source` (the diagnostic's own file) and attaching `file`
/// (that file's path).
/// Convert a resolved diagnostic to its JS shape. `source` is the diagnostic's
/// OWN file source (offsets are file-relative), used only to translate byte
/// offsets into UTF-16 for the editor. The file path comes from the resolved
/// diagnostic itself, so an included file's error lands on the right tab.
fn diagnostic_to_js(d: &brink_compiler::ResolvedDiagnostic, source: &str) -> DiagnosticJs {
    DiagnosticJs {
        message: d.message.clone(),
        start: byte_to_utf16(source, d.range.start().into()),
        end: byte_to_utf16(source, d.range.end().into()),
        severity: format!("{:?}", d.code.severity()),
        file: d.path.clone(),
    }
}

/// Convert a symbol tree to JSON, translating byte ranges to UTF-16 offsets
/// against `source` (the file the symbols belong to).
fn convert_document_symbol(
    sym: brink_ide::document::DocumentSymbol,
    source: &str,
) -> DocumentSymbolJs {
    DocumentSymbolJs {
        name: sym.name,
        kind: symbol_kind_str(sym.kind).to_owned(),
        detail: sym.detail,
        start: byte_to_utf16(source, sym.range.start().into()),
        end: byte_to_utf16(source, sym.range.end().into()),
        full_start: byte_to_utf16(source, sym.full_range.start().into()),
        full_end: byte_to_utf16(source, sym.full_range.end().into()),
        children: sym
            .children
            .into_iter()
            .map(|c| convert_document_symbol(c, source))
            .collect(),
    }
}

// ── Auto-import helper ───────────────────────────────────────────────

#[derive(Serialize)]
struct AutoImportJs {
    ok: bool,
    already_reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    edit: Option<brink_ide::line_convert::TextEdit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ── Structural result helpers (#316) ─────────────────────────────────

/// The unified JSON payload for every mutating structural op (rename, move,
/// promote, demote, reorder, file-rename, delete). `new_source` is the rewritten
/// primary file; `cross_file_edits` carry the referencing files' rewrites
/// (resolved to full source). `safe` + `introduced_diagnostics` are the
/// safe-by-default breakage gate — empty/`true` for reorders and clean ops.
#[derive(Serialize)]
struct StructuralResultJs {
    ok: bool,
    /// The file path this result applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_source: Option<String>,
    cross_file_edits: Vec<CrossFileEditJs>,
    /// Diagnostics present after the op but not before. Empty ⇒ `safe`.
    introduced_diagnostics: Vec<RenameDiagJs>,
    /// True when the op introduces no new diagnostics.
    safe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// A cross-file reference edit, resolved to the full new source of the file.
///
/// brink-ide reports cross-file edits as `(FileId, byte range, new_text)`, but
/// the editor works in paths and UTF-16 offsets. We resolve each affected file
/// here — applying its byte-range edits against its source — so the consumer
/// just replaces the file's content by path.
#[derive(Serialize)]
struct CrossFileEditJs {
    path: String,
    new_source: String,
}

/// Apply non-overlapping byte-range edits to `src`. Edits are applied from the
/// highest start offset down so earlier offsets stay valid; out-of-bounds or
/// non-char-boundary edits are skipped (defensive — they should never occur).
fn apply_edits(src: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = src.to_owned();
    for (start, end, text) in edits {
        if start <= end
            && end <= out.len()
            && out.is_char_boundary(start)
            && out.is_char_boundary(end)
        {
            out.replace_range(start..end, &text);
        }
    }
    out
}

/// One entry in a structural op's breakage report.
#[derive(Serialize)]
struct RenameDiagJs {
    severity: String,
    code: String,
    message: String,
    path: String,
    /// 1-based line of the diagnostic's start.
    line: u32,
    /// 1-based column of the diagnostic's start.
    col: u32,
}

/// Map a brink-ide [`IntroducedDiagnostic`](brink_ide::structural_result::IntroducedDiagnostic)
/// to its JSON shape.
fn diag_js(d: &brink_ide::structural_result::IntroducedDiagnostic) -> RenameDiagJs {
    RenameDiagJs {
        severity: match d.severity {
            brink_ir::Severity::Error => "error",
            brink_ir::Severity::Warning => "warning",
        }
        .to_owned(),
        code: d.code.as_str().to_owned(),
        message: d.message.clone(),
        path: d.path.clone(),
        line: d.line,
        col: d.col,
    }
}

/// Resolve a [`StructuralResult`](brink_ide::structural_result::StructuralResult)'s
/// cross-file `FileEdit`s to full new file sources (applying each file's
/// byte-range edits against its current source), excluding the primary `path`
/// (already covered by `new_source`). Deterministic (BTreeMap-grouped).
fn resolve_cross_file_edits(
    session: &IdeSession,
    result: &brink_ide::structural_result::StructuralResult,
    path: &str,
) -> Vec<CrossFileEditJs> {
    let mut by_file: std::collections::BTreeMap<u32, Vec<(usize, usize, String)>> =
        std::collections::BTreeMap::new();
    for e in &result.cross_file_edits {
        by_file.entry(e.file.0).or_default().push((
            usize::from(e.range.start()),
            usize::from(e.range.end()),
            e.new_text.clone(),
        ));
    }

    let mut edits: Vec<CrossFileEditJs> = Vec::new();
    for (file_raw, file_edits) in by_file {
        let file_id = brink_ir::FileId(file_raw);
        let (Some(src), Some(fpath)) = (session.source(file_id), session.file_path(file_id)) else {
            continue;
        };
        if fpath == path {
            continue;
        }
        edits.push(CrossFileEditJs {
            path: fpath.to_owned(),
            new_source: apply_edits(src, file_edits),
        });
    }
    edits
}

/// Serialize a fully-formed [`StructuralResult`](brink_ide::structural_result::StructuralResult)
/// (already carrying `safe` / `introduced`) to the unified `StructuralResultJs`
/// JSON. Used by rename, file-rename, and delete — ops that gate themselves.
fn structural_result_json(
    session: &IdeSession,
    result: &brink_ide::structural_result::StructuralResult,
    path: &str,
) -> String {
    let cross_file_edits = resolve_cross_file_edits(session, result, path);
    let introduced_diagnostics: Vec<RenameDiagJs> = result.introduced.iter().map(diag_js).collect();
    let resp = StructuralResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: result.new_source.clone(),
        cross_file_edits,
        safe: result.safe,
        introduced_diagnostics,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

/// Run the op-agnostic breakage gate over a structural *move*'s result (which
/// arrives un-gated from the pure ops), then serialize. The move's primary
/// source is a full-file rewrite, so the gate overlays it wholesale and the
/// cross-file edits onto their own files.
fn gated_move_json(
    session: &IdeSession,
    mut result: brink_ide::structural_result::StructuralResult,
    path: &str,
) -> String {
    if let Some(new_source) = result.new_source.as_deref() {
        let introduced = brink_ide::structural_result::gate_with_source(
            session,
            path,
            new_source,
            &result.cross_file_edits,
        );
        result.safe = introduced.is_empty();
        result.introduced = introduced;
    }
    structural_result_json(session, &result, path)
}

// ── Directory-move result helpers (#314) ─────────────────────────────

/// The JSON payload for an atomic directory rename/move (#314) — the multi-file
/// analog of [`StructuralResultJs`]. `moved_files` are the relocated files (each
/// carrying its new path + rewritten source); `cross_file_edits` carry the
/// outside referrers' rewrites. `safe` + `introduced_diagnostics` are the shared
/// safe-by-default breakage gate.
#[derive(Serialize)]
struct DirMoveResultJs {
    ok: bool,
    /// Every file relocated by the move.
    moved_files: Vec<MovedFileJs>,
    /// Reference edits in files outside the moved directory, resolved to full
    /// new source.
    cross_file_edits: Vec<CrossFileEditJs>,
    /// Diagnostics present after the move but not before. Empty ⇒ `safe`.
    introduced_diagnostics: Vec<RenameDiagJs>,
    /// True when the move introduces no new diagnostics.
    safe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// One relocated file: written at `new_path` (with `new_source`), removed from
/// `old_path`.
#[derive(Serialize)]
struct MovedFileJs {
    old_path: String,
    new_path: String,
    new_source: String,
}

/// Serialize a [`DirMoveResult`](brink_ide::dir_rename::DirMoveResult) to its
/// JSON shape. Cross-file (inbound) edits are resolved to full new source
/// deterministically (BTreeMap-grouped); moved files are already full sources.
fn dir_move_result_json(
    session: &IdeSession,
    result: &brink_ide::dir_rename::DirMoveResult,
) -> String {
    let moved_files: Vec<MovedFileJs> = result
        .moved_files
        .iter()
        .map(|m| MovedFileJs {
            old_path: m.old_path.clone(),
            new_path: m.new_path.clone(),
            new_source: m.new_source.clone(),
        })
        .collect();

    // Inbound edits land in files outside the folder — resolve each to full
    // source. Group by file id for determinism (BTreeMap), splice from the end.
    let mut by_file: std::collections::BTreeMap<u32, Vec<(usize, usize, String)>> =
        std::collections::BTreeMap::new();
    for e in &result.cross_file_edits {
        by_file.entry(e.file.0).or_default().push((
            usize::from(e.range.start()),
            usize::from(e.range.end()),
            e.new_text.clone(),
        ));
    }
    let mut cross_file_edits: Vec<CrossFileEditJs> = Vec::new();
    for (file_raw, file_edits) in by_file {
        let file_id = brink_ir::FileId(file_raw);
        let (Some(src), Some(fpath)) = (session.source(file_id), session.file_path(file_id)) else {
            continue;
        };
        cross_file_edits.push(CrossFileEditJs {
            path: fpath.to_owned(),
            new_source: apply_edits(src, file_edits),
        });
    }

    let introduced_diagnostics: Vec<RenameDiagJs> = result.introduced.iter().map(diag_js).collect();
    let resp = DirMoveResultJs {
        ok: true,
        moved_files,
        cross_file_edits,
        safe: result.safe,
        introduced_diagnostics,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

/// A `DirMoveResult`-shaped error payload (`ok: false`).
fn dir_error_json(msg: &str) -> String {
    let resp = DirMoveResultJs {
        ok: false,
        moved_files: Vec::new(),
        cross_file_edits: Vec::new(),
        introduced_diagnostics: Vec::new(),
        safe: true,
        error: Some(msg.to_owned()),
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

/// A trivially-safe single-file rewrite (reorders): no gate, empty breakage.
fn move_result_json_simple(new_source: String, path: &str) -> String {
    let resp = StructuralResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: Some(new_source),
        cross_file_edits: Vec::new(),
        introduced_diagnostics: Vec::new(),
        safe: true,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

fn error_json(msg: &str) -> String {
    let resp = StructuralResultJs {
        ok: false,
        path: None,
        new_source: None,
        cross_file_edits: Vec::new(),
        introduced_diagnostics: Vec::new(),
        safe: true,
        error: Some(msg.to_owned()),
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

// ── Legacy stateless functions (token legend) ───────────────────────

/// Get token type names for the legend.
#[wasm_bindgen]
pub fn token_type_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_type_names()).unwrap_or_default()
}

/// Get token modifier names for the legend.
#[wasm_bindgen]
pub fn token_modifier_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_modifier_names()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        reason = "tiny literal test strings; offsets cannot overflow u32"
    )]
    use super::{byte_to_utf16, utf16_to_byte};

    #[test]
    fn ascii_is_identity() {
        let s = "hello world";
        for i in 0..=s.len() as u32 {
            assert_eq!(byte_to_utf16(s, i), i, "byte_to_utf16 at {i}");
            assert_eq!(utf16_to_byte(s, i), i, "utf16_to_byte at {i}");
        }
    }

    #[test]
    fn two_byte_char_caf_e() {
        // "café": c=0, a=1, f=2, é=3..5 (2 bytes, 1 UTF-16 unit), len=5 bytes / 4 units
        let s = "café";
        assert_eq!(byte_to_utf16(s, 3), 3); // start of é
        assert_eq!(byte_to_utf16(s, 5), 4); // end of string
        assert_eq!(utf16_to_byte(s, 3), 3);
        assert_eq!(utf16_to_byte(s, 4), 5);
    }

    #[test]
    fn three_byte_em_dash() {
        // "a—b": a=0, —=1..4 (U+2014, 3 bytes, 1 unit), b=4..5
        let s = "a—b";
        assert_eq!(byte_to_utf16(s, 4), 2); // start of b
        assert_eq!(byte_to_utf16(s, 5), 3); // end
        assert_eq!(utf16_to_byte(s, 2), 4);
        assert_eq!(utf16_to_byte(s, 3), 5);
    }

    #[test]
    fn four_byte_astral_emoji() {
        // "a😀b": a=0, 😀=1..5 (U+1F600, 4 bytes, 2 UTF-16 units), b=5..6
        let s = "a😀b";
        assert_eq!(byte_to_utf16(s, 1), 1); // start of emoji
        assert_eq!(byte_to_utf16(s, 5), 3); // after emoji (1 + 2 units)
        assert_eq!(byte_to_utf16(s, 6), 4); // end
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(utf16_to_byte(s, 4), 6);
    }

    #[test]
    fn round_trip_on_char_boundaries() {
        let s = "x—y😀zé!";
        for (i, _) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
            let units = byte_to_utf16(s, i as u32);
            assert_eq!(utf16_to_byte(s, units), i as u32, "round-trip at byte {i}");
        }
    }

    #[test]
    fn clamps_past_end() {
        let s = "café";
        assert_eq!(byte_to_utf16(s, 999), 4); // total UTF-16 length
        assert_eq!(utf16_to_byte(s, 999), 5); // total byte length
    }

    // ── End-to-end boundary tests ───────────────────────────────────
    // These prove the EditorSession surfaces UTF-16 offsets even when a
    // non-ASCII char shifts the byte/UTF-16 mapping. The é before the knot
    // makes every byte offset past it 1 larger than its UTF-16 offset.

    use super::EditorSession;

    #[test]
    fn document_symbols_returns_utf16_offsets() {
        // "é\n=== k ===\n": é = 2 bytes / 1 UTF-16 unit, so the knot header
        // starts at byte 3 but UTF-16 offset 2; the name `k` at byte 7 / unit 6.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é\n=== k ===\nhi\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.document_symbols();
        let syms: serde_json::Value = serde_json::from_str(&json).unwrap();
        let knot = &syms[0];
        assert_eq!(knot["name"], "k");
        // UTF-16 offsets, not bytes (byte would be 7 / full_start 3).
        assert_eq!(
            knot["start"].as_u64().unwrap(),
            6,
            "name start must be UTF-16"
        );
        assert_eq!(
            knot["full_start"].as_u64().unwrap(),
            2,
            "knot full_start must be UTF-16"
        );
    }

    #[test]
    fn goto_definition_round_trips_utf16() {
        // Divert target on line 1, knot on line 3, with é shifting offsets.
        // "é -> k\n\n=== k ===\nhi\n"
        //  byte:  é(0..2) space(2) -(3) >(4) space(5) k(6) \n(7) ...
        //  utf16: é(0..1) space(1) -(2) >(3) space(4) k(5) \n(6) ...
        // Cursor on the `k` of `-> k` is UTF-16 offset 5.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é -> k\n\n=== k ===\nhi\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.goto_definition(5);
        assert_ne!(json, "null", "should resolve the divert target");
        let loc: serde_json::Value = serde_json::from_str(&json).unwrap();
        // goto resolves to the knot's name `k` inside `=== k ===`. That `k` is
        // at byte 13 but UTF-16 offset 12 (one é before it). A byte-based
        // result would be 13 — so 12 proves both the input offset (UTF-16 5 →
        // byte 6, the divert's `k`) and the output (byte 13 → UTF-16 12).
        assert_eq!(
            loc["start"].as_u64().unwrap(),
            12,
            "definition start must be UTF-16, not bytes"
        );
    }

    #[test]
    fn story_graph_edges_carry_utf16_occurrences() {
        // "é\n=== a ===\n-> b\n\n=== b ===\n-> DONE\n"
        //  The é (2 bytes / 1 UTF-16 unit) shifts every later byte offset
        //  1 past its UTF-16 offset. The divert target `b` on line 3 is at
        //  byte 16 → UTF-16 15.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é\n=== a ===\n-> b\n\n=== b ===\n-> DONE\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.story_graph();
        assert_ne!(json, "null");
        let graph: serde_json::Value = serde_json::from_str(&json).unwrap();
        let edges = graph["edges"].as_array().unwrap();

        let edge = |from: &str, to: &str| {
            edges
                .iter()
                .find(|e| e["from"] == from && e["to"] == to)
                .expect("missing edge")
        };

        // a -> b: one occurrence anchored on the target path `b`.
        let occ = &edge("a", "b")["occurrences"][0];
        assert_eq!(occ["file"], "main.ink");
        assert_eq!(occ["start"].as_u64().unwrap(), 15, "must be UTF-16");
        assert_eq!(occ["end"].as_u64().unwrap(), 16);

        // b -> DONE: occurrence spans the whole `-> DONE` statement.
        // Bytes: `-> DONE` starts at byte 29 → UTF-16 28, 7 chars long.
        let occ = &edge("b", "DONE")["occurrences"][0];
        assert_eq!(occ["file"], "main.ink");
        assert_eq!(occ["start"].as_u64().unwrap(), 28);
        assert_eq!(occ["end"].as_u64().unwrap(), 35);
    }

    // ── Cross-file structural-move edits (#12) ──────────────────────

    use super::apply_edits;

    #[test]
    fn apply_edits_applies_descending_and_preserves_offsets() {
        let src = "alpha beta gamma";
        // Two edits given out of order; both must land correctly.
        let out = apply_edits(
            src,
            vec![
                (11, 16, "GAMMA".to_owned()), // gamma -> GAMMA
                (0, 5, "ALPHA".to_owned()),   // alpha -> ALPHA
            ],
        );
        assert_eq!(out, "ALPHA beta GAMMA");
    }

    #[test]
    fn apply_edits_skips_out_of_bounds() {
        let src = "short";
        let out = apply_edits(src, vec![(0, 999, "x".to_owned())]);
        assert_eq!(out, "short", "out-of-bounds edit is skipped, not panicking");
    }

    #[test]
    fn cross_file_move_resolves_reference_edit_to_new_source() {
        // `other.ink` diverts into a stitch of `main.ink`; moving that stitch to
        // another knot must produce a cross-file edit updating the divert, now
        // delivered as the full new source of `other.ink`.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== a ===\n= s\nstuff\n-> END\n\n=== b ===\nbee\n-> END\n",
        );
        s.update_file("other.ink", "-> a.s\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.move_stitch("main.ink", "a", "s", "b");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], true, "move should succeed: {json}");

        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other_edit = cfe.iter().find(|e| e["path"] == "other.ink");
        assert!(
            other_edit.is_some(),
            "expected a cross-file edit for other.ink, got {cfe:?}"
        );
        let other = other_edit.unwrap();
        // The divert `-> a.s` must now point at the moved stitch's new path.
        assert!(
            other["new_source"].as_str().unwrap().contains("b.s"),
            "cross-file new_source should reference b.s: {other:?}"
        );
        // It carries the full file source (path-keyed), not byte ranges.
        assert!(other.get("new_source").is_some());
        assert!(other.get("start").is_none());
    }

    // ── Directory rename/move (#314) ────────────────────────────────

    #[test]
    fn rename_dir_returns_moved_files_and_inbound_edits() {
        let mut s = EditorSession::new();
        // main.ink (outside) includes into the folder; a folder file includes an
        // outside lib; two folder siblings include each other.
        s.update_file("main.ink", "INCLUDE chapters/intro.ink\n-> END\n");
        s.update_file("lib.ink", "=== helper ===\n-> END\n");
        s.update_file(
            "chapters/intro.ink",
            "INCLUDE ../lib.ink\nINCLUDE scene.ink\n-> END\n",
        );
        s.update_file("chapters/scene.ink", "=== scene ===\n-> END\n");

        let json = s.rename_dir("chapters", "book/chapters");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], true, "dir move should succeed: {json}");
        assert_eq!(v["safe"], true, "content-preserving move is safe: {json}");

        // Two moved files at their new paths.
        let moved = v["moved_files"].as_array().unwrap();
        assert_eq!(moved.len(), 2);
        let intro = moved
            .iter()
            .find(|m| m["new_path"] == "book/chapters/intro.ink")
            .unwrap();
        assert_eq!(intro["old_path"], "chapters/intro.ink");
        // Outbound: ../lib.ink now two levels deep; sibling stays bare.
        let intro_src = intro["new_source"].as_str().unwrap();
        assert!(
            intro_src.contains("INCLUDE ../../lib.ink"),
            "outbound not recomputed: {intro_src}"
        );
        assert!(
            intro_src.contains("INCLUDE scene.ink"),
            "sibling include should stay bare: {intro_src}"
        );

        // Inbound: main re-points into the new folder, delivered as full source.
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let main_edit = cfe.iter().find(|e| e["path"] == "main.ink").unwrap();
        assert!(
            main_edit["new_source"]
                .as_str()
                .unwrap()
                .contains("INCLUDE book/chapters/intro.ink"),
            "inbound not rewritten: {main_edit:?}"
        );
    }

    #[test]
    fn rename_dir_error_is_dir_move_shaped_json() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "-> END\n");
        let json = s.rename_dir("ghost", "x");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().is_some());
        assert!(v["moved_files"].as_array().unwrap().is_empty());
    }

    // ── Safe symbol rename (#305) ───────────────────────────────────

    #[test]
    fn rename_symbol_safe_rewrites_refs_with_empty_breakage() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "hello", "", "greeting")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "consistent rename is safe: {v}");
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
        let new_source = v["new_source"].as_str().unwrap();
        assert!(new_source.contains("=== greeting ==="));
        assert!(
            new_source.contains("-> greeting"),
            "divert rewritten: {new_source}"
        );
    }

    #[test]
    fn rename_symbol_collision_reports_breakage_and_cross_file_edits() {
        // `other.ink` diverts `-> a`; renaming knot `a` to `b` both collides
        // with the existing `b` (breakage) and rewrites the cross-file divert.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n\n=== b ===\n-> END\n");
        s.update_file("other.ink", "-> a\n");
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "a", "", "b")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], false, "collision is unsafe: {v}");

        let diags = v["introduced_diagnostics"].as_array().unwrap();
        assert!(
            diags.iter().any(|d| d["code"] == "E022"),
            "expected E022 duplicate-knot in breakage report: {diags:?}"
        );
        // Every diag carries the fields the report renders.
        let first = &diags[0];
        for key in ["severity", "code", "message", "path", "line", "col"] {
            assert!(first.get(key).is_some(), "diag missing {key}: {first:?}");
        }

        // The cross-file divert is still rewritten (edits computed regardless).
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other = cfe.iter().find(|e| e["path"] == "other.ink").unwrap();
        assert!(other["new_source"].as_str().unwrap().contains("-> b"));
    }

    #[test]
    fn rename_symbol_at_renames_by_offset_cross_file() {
        // F2's offset-based path: rename the knot whose declaration the cursor
        // sits in, rewriting a divert in another file.
        let mut s = EditorSession::new();
        let main = "=== hello ===\nHi.\n-> END\n";
        s.update_file("main.ink", main);
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset of the `hello` name in `=== hello ===` (ASCII ⇒ UTF-16 == byte).
        let offset = u32::try_from(main.find("hello").unwrap()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol_at("main.ink", offset, "greeting")).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["safe"], true);
        assert!(
            v["new_source"]
                .as_str()
                .unwrap()
                .contains("=== greeting ===")
        );
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other = cfe.iter().find(|e| e["path"] == "other.ink").unwrap();
        assert!(
            other["new_source"]
                .as_str()
                .unwrap()
                .contains("-> greeting")
        );
    }

    #[test]
    fn rename_symbol_unknown_returns_error() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "nope", "", "x")).unwrap();
        assert_eq!(v["ok"], false);
    }

    // ── Unified StructuralResult + deleteSymbol (#316) ──────────────

    #[test]
    fn delete_symbol_referenced_knot_reports_breakage() {
        // `start` diverts to `target`; deleting `target` dangles that divert.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n-> target\n=== target ===\n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "target", "")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(
            v["safe"], false,
            "deleting a referenced knot is unsafe: {v}"
        );
        let diags = v["introduced_diagnostics"].as_array().unwrap();
        assert!(
            !diags.is_empty(),
            "the dangling divert is reported: {diags:?}"
        );
        // Every diag carries the breakage-report fields.
        for key in ["severity", "code", "message", "path", "line", "col"] {
            assert!(diags[0].get(key).is_some(), "diag missing {key}: {diags:?}");
        }
        let new_source = v["new_source"].as_str().unwrap();
        assert!(
            !new_source.contains("=== target ==="),
            "target removed: {new_source}"
        );
    }

    #[test]
    fn delete_symbol_unreferenced_is_safe() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n=== b ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "b", "")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "no references, so safe: {v}");
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
    }

    #[test]
    fn delete_symbol_stitch_keeps_siblings() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== k ===\n= a\nA.\n= b\nB.\n= c\nC.\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "k", "b")).unwrap();
        let new_source = v["new_source"].as_str().unwrap();
        assert!(!new_source.contains("= b"), "b removed: {new_source}");
        assert!(
            new_source.contains("= a") && new_source.contains("= c"),
            "siblings kept: {new_source}"
        );
    }

    #[test]
    fn delete_symbol_unknown_returns_error() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "ghost", "")).unwrap();
        assert_eq!(v["ok"], false);
    }

    // ── extract-selection ops (#315 H) ─────────────────────────────

    #[test]
    fn extract_to_knot_returns_structural_result_with_tunnel_call() {
        let mut s = EditorSession::new();
        let src = "=== start ===\nHello.\nWorld.\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        // UTF-16 offsets == byte offsets here (ASCII source).
        let start = src.find("Hello.").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "greeting")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "self-contained extraction is safe: {v}");
        let new_source = v["new_source"].as_str().unwrap();
        assert!(new_source.contains("=== greeting ==="), "{new_source}");
        assert!(new_source.contains("-> greeting ->"), "{new_source}");
        assert!(new_source.contains("->->"), "tunnel return: {new_source}");
    }

    #[test]
    fn extract_to_function_returns_structural_result_with_call() {
        let mut s = EditorSession::new();
        let src = "=== start ===\n{2 + 3}\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("{2 + 3}").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_function("main.ink", start, end, "calc")).unwrap();
        assert_eq!(v["ok"], true);
        let new_source = v["new_source"].as_str().unwrap();
        assert!(
            new_source.contains("=== function calc() ==="),
            "{new_source}"
        );
        assert!(new_source.contains("{calc()}"), "inline call: {new_source}");
    }

    #[test]
    fn extract_that_breaks_scope_reports_breakage() {
        let mut s = EditorSession::new();
        let src = "=== start ===\n~ temp count = 3\n{count}\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("{count}").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "shower")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], false, "out-of-scope temp makes it unsafe: {v}");
        assert!(
            !v["introduced_diagnostics"].as_array().unwrap().is_empty(),
            "breakage reported: {v}"
        );
    }

    #[test]
    fn extract_header_crossing_returns_error() {
        let mut s = EditorSession::new();
        let src = "=== a ===\nContent.\n=== b ===\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("Content.").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "x")).unwrap();
        assert_eq!(v["ok"], false, "crossing a header is rejected: {v}");
    }

    #[test]
    fn reorder_returns_safe_with_empty_breakage() {
        // Reorders change no qualification — the unified result is trivially safe.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n=== b ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.reorder_knot("main.ink", "a", 1)).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true);
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
        // The unified result still round-trips through the StructuralResult JSON:
        // every field the studio reads is present.
        assert!(v.get("new_source").is_some());
        assert!(v.get("cross_file_edits").is_some());
    }

    #[test]
    fn breaking_move_reports_introduced_diagnostics() {
        // Moving a stitch whose bare same-knot reference can't be requalified
        // into the destination is gated. Here a divert in `other` targets the
        // qualified stitch; the move rewrites it, staying safe — but a move that
        // collides surfaces breakage. We assert the unified result always carries
        // the gate fields regardless of outcome.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== src ===\n= movable\n-> END\n=== dst ===\nDest.\n",
        );
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.move_stitch("main.ink", "src", "movable", "dst")).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v.get("safe").is_some(), "move carries the gate flag: {v}");
        assert!(
            v.get("introduced_diagnostics").is_some(),
            "move carries the breakage list: {v}"
        );
    }

    // ── Document handles (#122) ─────────────────────────────────────

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn two_handles_on_different_files_query_independently() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "=== alpha ===\nA line\n-> END\n");
        s.update_file("b.ink", "hello b\n=== beta ===\nB line\n-> END\n");
        // No set_active_file: the singleton still points at the unloaded
        // default, proving handles don't depend on it.
        let da = s.open_document("a.ink");
        let db = s.open_document("b.ink");
        assert_ne!(da, 0);
        assert_ne!(db, 0);
        assert_ne!(da, db);
        assert_eq!(s.active_file(), "main.ink");

        // hover over each file's knot name resolves per-handle.
        // a.ink: `alpha` at offsets 4..9; b.ink: `beta` at 12..16.
        let ha = s.hover_doc(da, 5);
        let hb = s.hover_doc(db, 13);
        assert!(ha.contains("alpha"), "hover via handle a: {ha}");
        assert!(hb.contains("beta"), "hover via handle b: {hb}");

        // line_contexts are file-specific per handle.
        let la = json(&s.line_contexts_doc(da));
        let lb = json(&s.line_contexts_doc(db));
        assert_eq!(la[0]["element"], "knot_header", "a.ink starts with a knot");
        assert_eq!(lb[0]["element"], "narrative", "b.ink starts with narrative");

        // completions work through a handle with no active file set.
        let ca = json(&s.completions_doc(da, 0));
        let names: Vec<&str> = ca
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|i| i["name"].as_str())
            .collect();
        assert!(names.contains(&"alpha"), "completions: {names:?}");
        assert!(names.contains(&"beta"), "completions: {names:?}");

        // The singleton queries still target the (unloaded) active file.
        assert_eq!(s.line_contexts(), "[]");
    }

    #[test]
    fn fragment_update_splices_and_reports_change_spec() {
        // é = 2 bytes / 1 UTF-16 unit: every offset past it differs between
        // byte and UTF-16 coordinates, proving the spec is UTF-16.
        //   bytes: "é intro\n"(0..9) "=== a ===\n"(9..19) "A line\n"(19..26)
        //          "=== b ===\n"(26..36) "B line\n"(36..43)
        //   utf16: 0..8 / 8..18 / 18..25 / 25..35 / 35..42
        let full = "é intro\n=== a ===\nA line\n=== b ===\nB line\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", full);

        let file_doc = s.open_document("main.ink");
        // Fragment over knot `a`: UTF-16 [8, 25).
        let frag = s.open_fragment("main.ink", 8, 25);
        assert_ne!(file_doc, 0);
        assert_ne!(frag, 0);
        assert_eq!(
            s.get_view_source_doc(frag),
            serde_json::to_string("=== a ===\nA line\n").unwrap()
        );

        // Update the fragment WITHOUT a trailing newline: the splice inserts
        // a `\n` separator, and the spec must carry the actually-inserted
        // text (source + "\n").
        let spec = json(&s.update_document(frag, "=== a ===\nA new"));
        assert_eq!(spec["path"], "main.ink");
        assert_eq!(spec["start"], 8, "UTF-16 start of replaced range");
        assert_eq!(spec["end"], 25, "UTF-16 end of replaced range");
        assert_eq!(
            spec["text"], "=== a ===\nA new\n",
            "separator nuance: actually-inserted text differs from source"
        );

        let expected = "é intro\n=== a ===\nA new\n=== b ===\nB line\n";
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string(expected).unwrap()
        );
        // The full-file handle on the same file sees the spliced content.
        assert_eq!(
            s.get_view_source_doc(file_doc),
            serde_json::to_string(expected).unwrap()
        );
        // The fragment handle's own view tracked the new fragment extent
        // (excluding the separator).
        assert_eq!(
            s.get_view_source_doc(frag),
            serde_json::to_string("=== a ===\nA new").unwrap()
        );

        // Update again WITH a trailing newline: no separator inserted (one
        // already follows the fragment), so no `text` in the spec. New file
        // coords: fragment is bytes [9, 24) = UTF-16 [8, 23).
        let spec = json(&s.update_document(frag, "=== a ===\nA two"));
        assert_eq!(spec["start"], 8);
        assert_eq!(spec["end"], 23);
        assert!(
            spec.get("text").is_none(),
            "no separator inserted -> no text field: {spec}"
        );
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("é intro\n=== a ===\nA two\n=== b ===\nB line\n").unwrap()
        );
    }

    #[test]
    fn full_file_handle_update_reports_whole_file_spec() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é one\n"); // 7 bytes, 6 UTF-16 units
        let d = s.open_document("main.ink");

        let spec = json(&s.update_document(d, "two\n"));
        assert_eq!(spec["path"], "main.ink");
        assert_eq!(spec["start"], 0);
        assert_eq!(spec["end"], 6, "whole previous file range, in UTF-16");
        assert!(spec.get("text").is_none());
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("two\n").unwrap()
        );
    }

    #[test]
    fn close_reopen_handle_lifecycle() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "=== alpha ===\nA\n-> END\n");

        // Unknown files don't get handles.
        assert_eq!(s.open_document("nope.ink"), 0);
        assert_eq!(s.open_fragment("nope.ink", 0, 1), 0);

        let d1 = s.open_document("a.ink");
        assert_eq!(d1, 1, "ids start at 1");
        assert!(s.close_document(d1));
        assert!(!s.close_document(d1), "double close reports unknown");

        // Closed handles answer with the same sentinels as a missing file.
        assert_eq!(s.hover_doc(d1, 5), "null");
        assert_eq!(s.line_contexts_doc(d1), "[]");
        assert_eq!(s.get_view_source_doc(d1), "null");
        assert_eq!(s.update_document(d1, "x"), "null");

        // Reopen: a fresh id, never a reused one.
        let d2 = s.open_document("a.ink");
        assert_ne!(d2, 0);
        assert_ne!(d2, d1, "handle ids are not reused");
        assert_ne!(s.get_view_source_doc(d2), "null");
    }

    #[test]
    fn singleton_api_unaffected_by_handle_operations() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\nA line\n=== b ===\nB line\n");
        s.update_file("other.ink", "hello\n");
        assert!(s.set_active_file("main.ink"));
        // Singleton view over knot `a`: UTF-16 [0, 17).
        s.set_view_context(0, 17);
        let before = s.get_view_source();
        assert_eq!(
            before,
            serde_json::to_string("=== a ===\nA line\n").unwrap()
        );

        // Handle operations on another file leave the singleton alone.
        let d = s.open_document("other.ink");
        let _ = s.update_document(d, "world\n");
        assert!(s.close_document(d));
        assert_eq!(s.active_file(), "main.ink");
        assert_eq!(s.get_view_source(), before);

        // The singleton splice path still works after handle traffic.
        s.update_source("=== a ===\nA edit");
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("=== a ===\nA edit\n=== b ===\nB line\n").unwrap()
        );
        assert_eq!(
            s.get_view_source(),
            serde_json::to_string("=== a ===\nA edit").unwrap()
        );
    }

    // ── Story graph (#96) ───────────────────────────────────────────

    const GRAPH_MAIN: &str = "é\n=== start ===\n* [Go] -> east.gate\n- -> END\n";
    const GRAPH_EAST: &str = "=== east ===\n= gate\nGate.\n-> start\n";

    #[test]
    fn story_graph_null_without_analysis() {
        let s = EditorSession::new();
        assert_eq!(s.story_graph(), "null");
    }

    #[test]
    fn story_graph_shape_and_utf16_offsets() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", GRAPH_MAIN);
        s.update_file("east.ink", GRAPH_EAST);

        let json = s.story_graph();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Nodes sorted by id; pseudo-node END present (referenced), DONE not.
        let ids: Vec<&str> = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["END", "east", "east.gate", "start"]);

        let nodes = v["nodes"].as_array().unwrap();
        let node = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();

        // `start` is declared after the 2-byte/1-unit `é`: its name sits at
        // byte 7 but UTF-16 offset 6 — the span must be UTF-16.
        let start = node("start");
        assert_eq!(start["kind"], "knot");
        assert_eq!(start["file"], "main.ink");
        assert_eq!(start["start"].as_u64().unwrap(), 6, "must be UTF-16");
        assert_eq!(start["end"].as_u64().unwrap(), 11);
        assert!(start.get("parent").is_none(), "knots carry no parent");

        let gate = node("east.gate");
        assert_eq!(gate["kind"], "stitch");
        assert_eq!(gate["file"], "east.ink");
        assert_eq!(gate["parent"], "east");

        let end = node("END");
        assert_eq!(end["kind"], "end");
        assert!(end.get("file").is_none(), "pseudo-nodes have no file");
        assert!(end.get("start").is_none(), "pseudo-nodes have no span");

        // Edges sorted by (from, to, kind); choice aggregation + cross-file
        // resolution + the auto-enter divert east -> east.gate.
        let edges: Vec<(String, String, String)> = v["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["from"].as_str().unwrap().to_owned(),
                    e["to"].as_str().unwrap().to_owned(),
                    e["kind"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let owned: Vec<(&str, &str, &str)> = edges
            .iter()
            .map(|(f, t, k)| (f.as_str(), t.as_str(), k.as_str()))
            .collect();
        assert_eq!(
            owned,
            vec![
                ("east", "east.gate", "divert"),
                ("east.gate", "start", "divert"),
                ("start", "END", "divert"),
                ("start", "east.gate", "choice"),
            ]
        );
    }

    #[test]
    fn story_graph_deterministic_across_file_insertion_order() {
        let mut a = EditorSession::new();
        a.update_file("main.ink", GRAPH_MAIN);
        a.update_file("east.ink", GRAPH_EAST);

        let mut b = EditorSession::new();
        b.update_file("east.ink", GRAPH_EAST);
        b.update_file("main.ink", GRAPH_MAIN);

        assert_eq!(
            a.story_graph(),
            b.story_graph(),
            "story graph JSON must be identical regardless of insertion order"
        );
    }

    // ── Document-agnostic / symbol-keyed references (#317) ──────────

    fn refs_count(json: &str) -> usize {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v.as_array().map_or(0, Vec::len)
    }

    #[test]
    fn find_references_at_same_file() {
        // Two diverts into `hello` plus its declaration, all in one file.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset on the first `-> hello` reference (the `h` of the target).
        let json = s.find_references_at("main.ink", 3, true);
        // Declaration + two divert references = 3.
        assert_eq!(refs_count(&json), 3, "same-file refs incl. decl: {json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        for loc in v.as_array().unwrap() {
            assert_eq!(loc["file"], "main.ink");
        }
    }

    #[test]
    fn find_references_at_cross_file() {
        // `other.ink` diverts into `hello` declared in `main.ink`.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset on the `hello` of the declaration `=== hello ===` (utf16 = byte).
        let json = s.find_references_at("main.ink", 4, true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files: std::collections::BTreeSet<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["file"].as_str().unwrap())
            .collect();
        assert!(
            files.contains("main.ink") && files.contains("other.ink"),
            "cross-file refs must span both files: {json}"
        );
    }

    #[test]
    fn find_references_at_honors_include_declaration() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> hello\n");
        assert!(s.set_active_file("main.ink"));

        let with_decl = refs_count(&s.find_references_at("main.ink", 3, true));
        let without_decl = refs_count(&s.find_references_at("main.ink", 3, false));
        assert_eq!(
            with_decl,
            without_decl + 1,
            "excluding the declaration drops exactly one result"
        );
    }

    #[test]
    fn references_to_symbol_name_keyed_lookup() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.references_to_symbol("hello", true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files: std::collections::BTreeSet<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["file"].as_str().unwrap())
            .collect();
        assert!(
            files.contains("main.ink") && files.contains("other.ink"),
            "symbol-keyed lookup resolves the declaration + cross-file ref: {json}"
        );

        // include_declaration is honored through the symbol-keyed path too.
        let with_decl = refs_count(&json);
        let without_decl = refs_count(&s.references_to_symbol("hello", false));
        assert_eq!(with_decl, without_decl + 1);
    }

    #[test]
    fn references_to_symbol_nonexistent_is_empty() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        assert_eq!(
            s.references_to_symbol("does_not_exist", true),
            "[]",
            "unknown symbol fails safe to []"
        );
    }

    // ── resolve_code_action (#321 Track N) ──────────────────────────
    // The code_actions JSON carries a self-describing `data` discriminator;
    // feeding that payload back to resolve_code_action applies the action and
    // returns StructuralResult-shaped JSON with the rewritten source.

    /// The byte offset (cursor) inside the first knot's body — enough to scope
    /// cursor-anchored actions to that knot.
    const UNSORTED_KNOTS: &str = "=== beta ===\nhi\n-> END\n=== alpha ===\nyo\n-> END\n";

    fn find_action<'a>(
        actions: &'a serde_json::Value,
        title_contains: &str,
    ) -> &'a serde_json::Value {
        actions
            .as_array()
            .expect("code_actions returns a JSON array")
            .iter()
            .find(|a| {
                a["title"]
                    .as_str()
                    .is_some_and(|t| t.contains(title_contains))
            })
            .expect("a code action whose title matches the expected substring")
    }

    #[test]
    fn code_action_data_is_self_describing() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(0)).expect("valid JSON array");
        let sort = find_action(&actions, "Sort knots");
        // The tagged discriminator must be present and round-trippable.
        assert_eq!(
            sort["data"]["action"], "SortKnots",
            "data carries the tagged discriminator: {actions}"
        );
    }

    #[test]
    fn resolve_sort_action_yields_sorted_source() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(0)).expect("valid JSON array");
        let data = find_action(&actions, "Sort knots")["data"].to_string();

        let result: serde_json::Value = serde_json::from_str(&s.resolve_code_action(&data, 0))
            .expect("valid StructuralResult JSON");
        assert_eq!(result["ok"], true, "resolve succeeds: {result}");
        let new_source = result["new_source"]
            .as_str()
            .expect("new_source is present and a string");
        assert!(
            !new_source.is_empty(),
            "sort action produces non-empty edits"
        );
        // alpha now precedes beta.
        let alpha = new_source.find("alpha").expect("alpha knot present");
        let beta = new_source.find("beta").expect("beta knot present");
        assert!(
            alpha < beta,
            "knots are sorted alphabetically: {new_source:?}"
        );
    }

    #[test]
    fn references_to_symbol_ambiguous_is_empty() {
        // A knot and a variable share the name `dup`. They are different kinds,
        // so both land under one `by_name` key → ambiguous (two ids).
        let mut s = EditorSession::new();
        s.update_file("main.ink", "VAR dup = 0\n=== dup ===\nhi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        assert_eq!(
            s.references_to_symbol("dup", true),
            "[]",
            "ambiguous symbol name fails safe to []"
        );
    }

    #[test]
    fn completions_tag_out_of_scope_symbols_with_source_file() {
        // main.ink INCLUDEs included.ink but NOT economy.ink. A knot from the
        // reachable file is in scope; one from the unreachable file is tagged
        // out-of-scope with its source path (#312 F).
        let mut s = EditorSession::new();
        s.update_file("included.ink", "=== reachable_knot ===\nhi.\n-> END\n");
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        let main = "INCLUDE included.ink\n=== start ===\n-> re\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        // Cursor after `-> re` (a divert context, which surfaces knots).
        let offset = u32::try_from(main.find("-> re").expect("divert present") + 5)
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let arr = items.as_array().expect("completions is an array");

        let reachable = arr
            .iter()
            .find(|i| i["name"] == "reachable_knot")
            .expect("reachable knot offered");
        assert!(
            reachable.get("out_of_scope").is_none(),
            "in-scope knot is not flagged out_of_scope: {reachable}"
        );

        let trade = arr
            .iter()
            .find(|i| i["name"] == "trade")
            .expect("out-of-scope knot offered");
        assert_eq!(
            trade["out_of_scope"], true,
            "unreachable knot flagged out_of_scope: {trade}"
        );
        assert_eq!(
            trade["source_file"], "economy.ink",
            "out-of-scope knot carries its source file: {trade}"
        );
    }

    #[test]
    fn completions_dedupe_out_of_scope_keeps_nearest() {
        // Two unreachable files both define `dup`. The nearer one (same dir as
        // the current file) wins deterministically; only one row survives.
        let mut s = EditorSession::new();
        s.update_file("near.ink", "=== dup ===\nn.\n-> END\n");
        s.update_file("deep/far.ink", "=== dup ===\nf.\n-> END\n");
        let main = "=== start ===\n-> du\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        let offset = u32::try_from(main.find("-> du").expect("divert present") + 5)
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let dups: Vec<&serde_json::Value> = items
            .as_array()
            .expect("array")
            .iter()
            .filter(|i| i["name"] == "dup")
            .collect();

        assert_eq!(
            dups.len(),
            1,
            "duplicate out-of-scope name collapses to one: {dups:?}"
        );
        assert_eq!(
            dups[0]["source_file"], "near.ink",
            "nearest source file wins: {:?}",
            dups[0]
        );
    }

    #[test]
    fn auto_import_apply_include_doc_rebases_open_fragment_view() {
        // Regression (#312 F): the fragment-view auto-import path. A raw
        // whole-file INCLUDE write shifts the fragment content right but leaves
        // the open fragment handle's view range at pre-shift offsets, so the
        // NEXT fragment splice clobbers the INCLUDE line and the knot header.
        // `auto_import_apply_include_doc` must apply the INCLUDE *and* rebase
        // the open fragment view so the subsequent splice lands correctly.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        let main_src = "=== start ===\nThe cursor is here.\n";
        s.update_file("main.ink", main_src);

        // Open a fragment over the knot BODY ("The cursor is here.\n"). The body
        // begins right after the "=== start ===\n" header (byte 14) and runs to
        // end of file.
        let body_start = "=== start ===\n".len() as u32;
        let body_end = main_src.len() as u32;
        let doc = s.open_fragment("main.ink", body_start, body_end);
        assert_ne!(doc, 0, "fragment handle opened");

        // Accept an out-of-scope completion: auto-import economy.ink into the
        // fragment's file, applying + rebasing out-of-band.
        let applied: serde_json::Value =
            serde_json::from_str(&s.auto_import_apply_include_doc(doc, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(applied["ok"], true, "op ok: {applied}");
        assert_eq!(
            applied["already_reachable"], false,
            "not yet reachable: {applied}"
        );
        // The returned edit DESCRIBES the applied shift (for the caller to
        // rebase its own TS-side range) — it is NOT to be re-applied.
        assert_eq!(
            applied["edit"]["insert"].as_str(),
            Some("INCLUDE economy.ink\n"),
            "returned edit describes the applied INCLUDE shift: {applied}"
        );
        assert_eq!(
            applied["edit"]["from"], 0,
            "INCLUDE inserted at file top: {applied}"
        );

        // The whole file now carries the INCLUDE above the untouched knot.
        let full_after_import = s.source_of("main.ink").expect("source").to_owned();
        assert_eq!(
            full_after_import, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.\n",
            "INCLUDE prepended, knot intact"
        );

        // Now the completion dispatches the accepted symbol into the FRAGMENT
        // view (the edited body). This routes through update_document, which
        // splices at the (now rebased) view range.
        let edited_body = "The cursor is here.trade\n";
        let spec = s.update_document(doc, edited_body);
        assert_ne!(spec, "null", "fragment push produced a change spec");

        // The INCLUDE line and knot header must survive; only the body changed.
        let full_after_push = s.source_of("main.ink").expect("source");
        assert_eq!(
            full_after_push, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.trade\n",
            "INCLUDE + header intact; only the fragment body was replaced"
        );
    }

    #[test]
    fn raw_update_file_then_fragment_push_corrupts_without_rebase() {
        // Documents the pre-fix bug (#312 F): applying the INCLUDE via the raw
        // whole-file `update_file` (which does NOT rebase open fragment views)
        // and then pushing the fragment splices at the STALE view range,
        // clobbering the INCLUDE line and the knot header. This is exactly the
        // corruption `auto_import_apply_include_doc` avoids. If this assertion
        // ever flips to producing clean output, `update_file` grew rebase
        // semantics and the fragment auto-import path can be simplified.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\n-> END\n");
        let main_src = "=== start ===\nThe cursor is here.\n";
        s.update_file("main.ink", main_src);

        let body_start = "=== start ===\n".len() as u32;
        let doc = s.open_fragment("main.ink", body_start, main_src.len() as u32);

        // OLD path: prepend INCLUDE via a raw whole-file replace (no rebase).
        s.update_file("main.ink", &format!("INCLUDE economy.ink\n{main_src}"));
        // Then push the edited fragment — splices at the stale [14, 34) range.
        s.update_document(doc, "The cursor is here.trade\n");

        let corrupted = s.source_of("main.ink").expect("source");
        assert_ne!(
            corrupted, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.trade\n",
            "raw path corrupts — this is the bug the apply-and-rebase op fixes"
        );
    }

    #[test]
    fn auto_import_apply_include_doc_idempotent_when_reachable() {
        // When the target is already reachable, the apply-and-rebase op is a
        // no-op: no INCLUDE added, view range untouched.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\n-> END\n");
        let main_src = "INCLUDE economy.ink\n=== start ===\nbody.\n";
        s.update_file("main.ink", main_src);
        // Re-analyze so the INCLUDE edge binds to the now-loaded target.
        s.update_file("main.ink", main_src);

        let body_start = "INCLUDE economy.ink\n=== start ===\n".len() as u32;
        let doc = s.open_fragment("main.ink", body_start, main_src.len() as u32);
        assert_ne!(doc, 0);

        let applied: serde_json::Value =
            serde_json::from_str(&s.auto_import_apply_include_doc(doc, "economy.ink"))
                .expect("valid JSON");
        assert_eq!(applied["already_reachable"], true, "already reachable");
        assert!(applied.get("edit").is_none(), "no edit");

        // Pushing the fragment still lands correctly (view range never moved).
        s.update_document(doc, "body.\n-> trade\n");
        assert_eq!(
            s.source_of("main.ink").expect("source"),
            "INCLUDE economy.ink\n=== start ===\nbody.\n-> trade\n"
        );
    }

    #[test]
    fn auto_import_doc_edit_is_utf16_and_idempotent() {
        // The doc-based auto-import returns a whole-file edit for an unreachable
        // target, and no edit once the file already reaches it.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        s.update_file("main.ink", "=== start ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let doc = s.open_document("main.ink");

        let first: serde_json::Value =
            serde_json::from_str(&s.auto_import_include_doc(doc, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(first["ok"], true, "op ok: {first}");
        assert_eq!(
            first["already_reachable"], false,
            "not yet reachable: {first}"
        );
        let insert = first["edit"]["insert"]
            .as_str()
            .expect("edit carries an insert string");
        assert!(
            insert.contains("INCLUDE economy.ink"),
            "insert adds the INCLUDE: {first}"
        );

        // Apply the edit, re-analyze, then a second call is a no-op.
        s.update_file("main.ink", &format!("{insert}=== start ===\n-> END\n"));
        assert!(s.set_active_file("main.ink"));
        let doc2 = s.open_document("main.ink");
        let second: serde_json::Value =
            serde_json::from_str(&s.auto_import_include_doc(doc2, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(second["already_reachable"], true, "now reachable: {second}");
        assert!(
            second.get("edit").is_none(),
            "idempotent — no second INCLUDE: {second}"
        );
    }

    #[test]
    fn resolve_format_action_yields_formatted_source() {
        // A knot whose body has a formatting deviation the formatter fixes.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== k ===\n* [opt]   trailing spaces   \n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        // Cursor inside the knot body.
        let offset = 12;
        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(offset)).expect("valid JSON array");
        let format = find_action(&actions, "Format knot");
        assert_eq!(
            format["data"]["action"], "FormatKnot",
            "format action carries its discriminator: {actions}"
        );
        let data = format["data"].to_string();

        let result: serde_json::Value = serde_json::from_str(&s.resolve_code_action(&data, offset))
            .expect("valid StructuralResult JSON");
        assert_eq!(result["ok"], true, "resolve succeeds: {result}");
        let new_source = result["new_source"]
            .as_str()
            .expect("new_source is present and a string");
        assert!(
            !new_source.is_empty(),
            "format action produces non-empty edits"
        );
    }

    #[test]
    fn resolve_rejects_malformed_data() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let result: serde_json::Value =
            serde_json::from_str(&s.resolve_code_action("{ not valid }", 0))
                .expect("error is still StructuralResult-shaped JSON");
        assert_eq!(result["ok"], false, "malformed data -> ok:false: {result}");
        assert!(
            result["error"].as_str().is_some(),
            "an error message is present"
        );
    }

    // ── Dialogue dialect (#368) ──────────────────────────────────────

    #[test]
    fn set_dialect_then_line_contexts_reports_character_kind() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\nHello there.\n");
        assert!(s.set_active_file("main.ink"));

        // No dialect registered yet: no `dialect` facet on any line.
        let before = json(&s.line_contexts());
        assert!(before[1].get("dialect").is_none(), "{before}");

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");

        let after = json(&s.line_contexts());
        assert_eq!(after[1]["dialect"]["kind"], "character", "{after}");
        assert_eq!(after[2]["dialect"]["kind"], "dialogue", "{after}");
    }

    #[test]
    fn folding_ranges_include_machinery_and_narrative_runs() {
        // #365: `folding_ranges()` returns structural folds (from the
        // pre-existing pass) plus machinery/narrative fold runs computed
        // from the same `line_contexts` classification the editor already
        // consumes — a real user path (folding gutter), not a separate
        // code path only a unit test reaches.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n~ temp x = 1\n~ temp y = 2\nHello there.\nHow are you?\n",
        );
        assert!(s.set_active_file("main.ink"));

        let ranges = json(&s.folding_ranges());
        let kinds: Vec<&str> = ranges
            .as_array()
            .expect("array")
            .iter()
            .map(|r| r["kind"].as_str().expect("kind is a string"))
            .collect();
        assert!(kinds.contains(&"machinery"), "{ranges}");
        assert!(kinds.contains(&"narrative"), "{ranges}");

        let machinery = ranges
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["kind"] == "machinery")
            .expect("machinery fold present");
        assert_eq!(machinery["start_line"], 1);
        assert_eq!(machinery["end_line"], 2);
    }

    #[test]
    fn folding_ranges_respect_registered_dialect_nature() {
        // A dialect-classified cue+dialogue pair is Narrative-natured (the
        // at-cue preset) — the fold run must follow the registered dialect,
        // not a hardcoded kind list.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\nHello there.\n");
        assert!(s.set_active_file("main.ink"));

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");

        let ranges = json(&s.folding_ranges());
        let has_narrative_run = ranges
            .as_array()
            .expect("array")
            .iter()
            .any(|r| r["kind"] == "narrative" && r["start_line"] == 1 && r["end_line"] == 2);
        assert!(has_narrative_run, "{ranges}");
    }

    #[test]
    fn clear_dialect_reverts_to_plain_classification() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\n");
        assert!(s.set_active_file("main.ink"));

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");
        assert_eq!(json(&s.line_contexts())[1]["dialect"]["kind"], "character");

        s.clear_dialect();
        let after = json(&s.line_contexts());
        assert!(after[1].get("dialect").is_none(), "{after}");
    }

    // `set_dialect`'s error path constructs a `JsError`, which panics when
    // called on a non-wasm target ("cannot call wasm-bindgen imported
    // functions on non-wasm targets") — same constraint as every other
    // `Result<_, JsError>`-returning method in this file (see
    // `StoryRunner::new`, `continue_story`, etc., whose error paths are only
    // exercised under `binding_wasm_tests` below). The rejection tests live
    // there; validated JSON acceptance is what's tested natively above.
    //
    // `WebSession` test coverage lives in `websession_wasm_tests` below —
    // its methods take `&JsValue`/`Vec<JsValue>` args, and constructing a
    // `JsValue` (e.g. `JsValue::from_f64`) itself panics off wasm32 (not just
    // the error paths), unlike `StoryRunner`'s native-testable subset.

    // ── Lines table (#366) ────────────────────────────────────────────
    // `StoryRunner::lines_table`'s happy path never constructs a `JsError`
    // (only the `serde_json` error branch would, and that can't fail for a
    // `LinesJson` value), so — like `set_dialect`'s acceptance path above —
    // it's exercised natively here rather than under `binding_wasm_tests`.

    fn compiled(src: &str) -> super::StoryRunner {
        let out = brink_compiler::compile("main.ink", |_path| Ok(src.to_owned()))
            .expect("test source compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        super::StoryRunner::new(&bytes).expect("runner constructs")
    }

    #[test]
    fn lines_table_reports_text_and_source_span() {
        let runner = compiled("=== start ===\nHello, world!\n-> END\n");
        let table = runner.lines_table().expect("lines_table succeeds");
        let v: serde_json::Value = serde_json::from_str(&table).expect("valid json");

        assert_eq!(v["version"], 1);
        let scopes = v["scopes"].as_array().expect("scopes array");
        let scope = scopes
            .iter()
            .find(|s| s["name"] == "start")
            .expect("start scope present");
        let line = scope["lines"]
            .as_array()
            .expect("lines array")
            .iter()
            .find(|l| l["content"] == "Hello, world!")
            .expect("the line's plain text content is present");
        let source = &line["source"];
        assert_eq!(source["file"], "main.ink", "{table}");
        assert!(
            source["range_start"].as_u64().is_some() && source["range_end"].as_u64().is_some(),
            "source span present: {table}"
        );
    }

    #[test]
    fn lines_table_resolves_includes_project_wide() {
        let out = brink_compiler::compile("main.ink", |path| match path {
            "main.ink" => Ok(
                "INCLUDE included.ink\n=== start ===\n-> other.other_stitch\n-> END\n".to_owned(),
            ),
            "included.ink" => {
                Ok("=== other ===\n= other_stitch\nFrom the included file.\n-> END\n".to_owned())
            }
            _ => Err(std::io::Error::other(format!("unknown file {path}"))),
        })
        .expect("multi-file project compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        let runner = super::StoryRunner::new(&bytes).expect("runner constructs");

        let table = runner.lines_table().expect("lines_table succeeds");
        let v: serde_json::Value = serde_json::from_str(&table).expect("valid json");
        let scopes = v["scopes"].as_array().expect("scopes array");
        let has_included_line = scopes.iter().any(|scope| {
            scope["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|l| l["content"] == "From the included file.")
        });
        assert!(
            has_included_line,
            "the lines table covers the whole project, INCLUDEs resolved: {table}"
        );
        let included_source_file = scopes
            .iter()
            .flat_map(|scope| scope["lines"].as_array().into_iter().flatten())
            .find(|l| l["content"] == "From the included file.")
            .and_then(|l| l["source"]["file"].as_str().map(str::to_owned));
        assert_eq!(
            included_source_file.as_deref(),
            Some("included.ink"),
            "source span attributes the line to its own included file: {table}"
        );
    }
}

// ── External-binding wasm tests ──────────────────────────────────────
//
// Exercise the ink↔JS external-binding boundary end-to-end through the real
// exported `StoryRunner` API. Gated to wasm32 so they build/run only under
// `wasm-pack test --node` and never affect host `cargo test`.
#[cfg(all(test, target_arch = "wasm32"))]
mod binding_wasm_tests {
    use super::StoryRunner;
    use js_sys::Function;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// Compile inline ink source to `.inkb` bytes.
    fn bytes(src: &str) -> Vec<u8> {
        let out = brink_compiler::compile("main.ink", |_path| Ok(src.to_owned()))
            .expect("test source compiles");
        let mut b = Vec::new();
        brink_format::write_inkb(&out.data, &mut b);
        b
    }

    fn runner(src: &str) -> StoryRunner {
        StoryRunner::new(&bytes(src))
            .ok()
            .expect("runner constructs")
    }

    fn cont(r: &StoryRunner) -> String {
        r.continue_story().ok().expect("story continues")
    }

    #[wasm_bindgen_test]
    fn pure_binding_inlined_into_text() {
        let r = runner("EXTERNAL double(x)\nResult: {double(21)}.\n-> END\n");
        r.bind_external("double", Function::new_with_args("x", "return x * 2"));
        assert!(cont(&r).contains("Result: 42"));
    }

    #[wasm_bindgen_test]
    fn string_return_marshals() {
        let r = runner("EXTERNAL who()\nHello {who()}!\n-> END\n");
        r.bind_external("who", Function::new_no_args("return \"world\""));
        assert!(cont(&r).contains("Hello world!"));
    }

    #[wasm_bindgen_test]
    fn unbound_external_strict_errors() {
        let r = runner("EXTERNAL ping()\nA{ping()}B\n-> END\n");
        assert!(
            r.continue_story().is_err(),
            "strict mode: unbound external with no fallback errors"
        );
    }

    #[wasm_bindgen_test]
    fn unbound_external_lenient_resolves_null() {
        let r = runner("EXTERNAL ping()\nA{ping()}B\n-> END\n");
        r.set_lenient_unbound(true);
        let text = cont(&r);
        assert!(
            text.contains('A') && text.contains('B'),
            "lenient mode resolves to null without erroring; got {text:?}"
        );
    }

    #[wasm_bindgen_test]
    fn throwing_binding_resolves_null() {
        let r = runner("EXTERNAL boom()\nX{boom()}Y\n-> END\n");
        r.bind_external("boom", Function::new_no_args("throw new Error('nope')"));
        let text = cont(&r);
        assert!(
            text.contains('X') && text.contains('Y'),
            "a throwing binding resolves null, not a VM error; got {text:?}"
        );
    }

    #[wasm_bindgen_test]
    fn get_set_var_round_trip() {
        let r = runner("VAR hp = 10\nHP {hp}.\n-> END\n");
        assert_eq!(r.get_var("hp").as_f64(), Some(10.0));
        assert!(r.set_var("hp", &JsValue::from_f64(3.0)));
        assert_eq!(r.get_var("hp").as_f64(), Some(3.0));
        assert!(r.get_var("missing").is_undefined());
        assert!(!r.set_var("missing", &JsValue::from_f64(1.0)));
        assert!(cont(&r).contains("HP 3."));
    }

    #[wasm_bindgen_test]
    fn seed_is_deterministic_across_reset() {
        let r = runner("{RANDOM(1, 1000)}\n-> END\n");
        r.set_seed(7);
        let first = cont(&r);
        r.reset();
        let second = cont(&r);
        assert_eq!(
            first, second,
            "reset re-applies the seed -> identical RANDOM output"
        );
    }

    /// Record an external during live play, then `reload` + replay: the
    /// recorded value is served even though the binding now returns something
    /// else, so a query-gated branch reproduces faithfully across a hot-reload.
    #[wasm_bindgen_test]
    fn reload_replays_recorded_external() {
        let src = "EXTERNAL get_switch(n)\nSwitch {get_switch(1): ON|OFF}.\n-> END\n";
        let mut r = runner(src);

        // Live play with the switch ON — records get_switch(1) = true.
        r.bind_external("get_switch", Function::new_with_args("n", "return true"));
        assert!(cont(&r).contains("Switch ON."), "live play takes ON");

        // Hot-reload the same program, then flip the binding to false. Replay
        // must serve the recorded `true`, so the branch stays ON.
        r.reload(&bytes(src)).ok().expect("reload");
        r.bind_external("get_switch", Function::new_with_args("n", "return false"));
        r.begin_replay();
        let replayed = cont(&r);
        r.end_replay();
        assert!(
            replayed.contains("Switch ON."),
            "reload replays the recorded ON branch, not the live binding; got {replayed}"
        );
    }

    /// After `end_replay`, genuinely-new content runs the live binding again:
    /// content recorded before the reload replays from the recording, while a
    /// fresh continue past it consults the (now-live) binding.
    #[wasm_bindgen_test]
    fn live_resumes_after_replay() {
        // Two switches: the first is recorded pre-reload; the second is only
        // reached after we leave replay mode, so it uses the live binding.
        // (Single-line source — a `\`-continuation would inject leading
        // whitespace into the ink, breaking the `=== after ===` knot header.)
        let src = "EXTERNAL get_switch(n)\nA{get_switch(1): ON|OFF}.\n* [go] -> after\n=== after ===\nB{get_switch(2): ON|OFF}.\n-> END\n";
        let mut r = runner(src);
        r.bind_external("get_switch", Function::new_with_args("n", "return true"));
        let page = cont(&r);
        assert!(page.contains("ON."), "first page ON; recorded; got {page}");

        // Reload, flip the binding to false. Replay the first page from the
        // recording (stays ON), make the choice, then leave replay — the second
        // page's get_switch(2) is uncovered, so it uses the live (false) binding.
        r.reload(&bytes(src)).ok().expect("reload");
        r.bind_external("get_switch", Function::new_with_args("n", "return false"));
        r.begin_replay();
        let first = cont(&r);
        assert!(
            first.contains("ON.") && !first.contains("OFF"),
            "replayed first page ON from the recording, not the live (false) binding; got {first}"
        );
        r.choose(0).ok().expect("choose");
        r.end_replay();
        let second = cont(&r);
        assert!(
            second.contains("OFF."),
            "post-replay content uses the live (false) binding; got {second}"
        );
    }

    #[wasm_bindgen_test]
    fn save_load_json_round_trip() {
        let r = runner("VAR hp = 10\nHP {hp}.\n-> END\n");
        assert!(r.set_var("hp", &JsValue::from_f64(3.0)));
        let blob = r.save().ok().expect("save");
        assert!(r.set_var("hp", &JsValue::from_f64(99.0)));
        let report = r.load(&blob).ok().expect("load");
        assert!(report.contains("unknown_globals"), "report JSON: {report}");
        assert_eq!(r.get_var("hp").as_f64(), Some(3.0), "hp restored from save");
    }

    #[wasm_bindgen_test]
    fn save_load_bytes_round_trip() {
        let r = runner("VAR hp = 10\nHP {hp}.\n-> END\n");
        assert!(r.set_var("hp", &JsValue::from_f64(7.0)));
        let bytes = r.save_bytes().ok().expect("save_bytes");
        assert!(r.set_var("hp", &JsValue::from_f64(1.0)));
        r.load_bytes(&bytes).ok().expect("load_bytes");
        assert_eq!(
            r.get_var("hp").as_f64(),
            Some(7.0),
            "hp restored from bytes"
        );
    }

    #[wasm_bindgen_test]
    fn call_function_pure() {
        let r = runner("-> END\n=== function add(a, b) ===\n~ return a + b\n");
        let v = r
            .call_function("add", vec![JsValue::from_f64(2.0), JsValue::from_f64(3.0)])
            .ok()
            .expect("call");
        assert_eq!(v.as_f64(), Some(5.0));
    }

    /// Host-directed parameterized knot entry (#178): `go_to_path_with_args`
    /// binds the knot's declared params, and the arg count is validated.
    #[wasm_bindgen_test]
    fn go_to_path_with_args_binds_and_validates() {
        let src = "-> END\n=== call(action) ===\nYou {action} now.\n-> END\n";
        let r = runner(src);
        r.go_to_path_with_args("call", vec![JsValue::from_str("wave")])
            .ok()
            .expect("enters parameterized knot");
        assert!(cont(&r).contains("You wave now."));

        // Wrong arity errors instead of jumping.
        let r2 = runner(src);
        assert!(
            r2.go_to_path_with_args("call", vec![]).is_err(),
            "arity mismatch (0 of 1) must error"
        );
    }

    #[wasm_bindgen_test]
    fn call_function_uses_binding() {
        let r =
            runner("EXTERNAL dbl(x)\n-> END\n=== function scaled(n) ===\n~ return dbl(n) + 1\n");
        r.bind_external("dbl", Function::new_with_args("x", "return x * 2"));
        let v = r
            .call_function("scaled", vec![JsValue::from_f64(10.0)])
            .ok()
            .expect("call");
        assert_eq!(v.as_f64(), Some(21.0), "dbl(10) + 1");
    }

    /// A binding that returns a Promise suspends the flow; the driver awaits the
    /// Promise, resolves the external, and resumes — end-to-end.
    #[wasm_bindgen_test]
    async fn async_binding_suspends_and_resolves() {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;

        let r = runner("EXTERNAL slow(x)\nGot {slow(20)}.\n-> END\n");
        r.bind_external(
            "slow",
            Function::new_with_args("x", "return Promise.resolve(x + 22)"),
        );

        let mut text = String::new();
        let mut suspended = false;
        for _ in 0..50 {
            let json = r.advance_one().ok().expect("advance_one");
            let v: serde_json::Value = serde_json::from_str(&json).ok().expect("json");
            let ty = v
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if ty == "awaiting_external" {
                suspended = true;
                let promise: js_sys::Promise =
                    r.take_pending_promise().dyn_into().ok().expect("promise");
                let value = JsFuture::from(promise).await.ok().expect("await");
                r.resolve_external(&value);
            } else {
                if let Some(t) = v.get("text").and_then(serde_json::Value::as_str) {
                    text.push_str(t);
                }
                if ty != "text" {
                    break; // terminal
                }
            }
        }
        assert!(suspended, "flow should have suspended on the async binding");
        assert!(
            text.contains("Got 42."),
            "resolved async value appears; got {text:?}"
        );
    }

    // ── Program identity (#181 / spec §5) ────────────────────────────

    #[wasm_bindgen_test]
    fn program_checksum_matches_program_model() {
        let b = bytes("Hello.\n-> END\n");
        let sum = super::program_checksum(&b).ok().expect("checksum decodes");
        assert!(sum.starts_with("0x"), "formatted as hex: {sum}");
        // The standalone checksum must match the one the runner's program model
        // reports — the studio compares the two to detect source-out-of-sync.
        let model = runner("Hello.\n-> END\n")
            .program_model()
            .ok()
            .expect("model");
        assert!(
            model.contains(&format!("\"checksum\":\"{sum}\"")),
            "program_model carries the same checksum; model={model}"
        );
    }

    #[wasm_bindgen_test]
    fn program_checksum_differs_for_different_sources() {
        let a = super::program_checksum(&bytes("Apple.\n-> END\n"))
            .ok()
            .expect("a");
        let b = super::program_checksum(&bytes("Banana.\n-> END\n"))
            .ok()
            .expect("b");
        assert_ne!(a, b, "distinct sources have distinct identity");
    }

    // ── Shared flows (#200) ──────────────────────────────────────────

    #[wasm_bindgen_test]
    fn shared_flow_reads_default_globals() {
        // A flow spawned at `other` reads the global the default flow's context
        // holds — proving the shared context (#200).
        let r = runner("VAR x = 0\nMain.\n-> END\n=== other ===\nx is {x}.\n-> END\n");
        assert!(r.set_var("x", &JsValue::from_f64(7.0)));

        r.spawn_flow("f", Some("other".to_owned()))
            .ok()
            .expect("spawn");
        let line = r.continue_flow("f").ok().expect("continue flow");
        assert!(
            line.contains("x is 7"),
            "shared flow reads the shared global; got {line}"
        );

        // The flow is listed, then destroyable.
        assert!(r.flow_names().ok().expect("names").contains("\"f\""));
        r.destroy_flow("f").ok().expect("destroy");
        assert!(!r.flow_names().ok().expect("names2").contains("\"f\""));
    }

    #[wasm_bindgen_test]
    fn shared_flow_writes_visible_to_default() {
        // The default flow at root reads a global the *flow* wrote — sharing is
        // bidirectional.
        let r = runner("VAR x = 0\n{x}\n-> END\n=== bump ===\n~ x = 9\nbumped.\n-> END\n");
        r.spawn_flow("f", Some("bump".to_owned()))
            .ok()
            .expect("spawn");
        // Drive the flow so it sets x = 9 in the shared context.
        let _ = r.continue_flow("f").ok().expect("flow line");
        // The default flow now reads 9.
        let line = r.continue_single().ok().expect("default line");
        assert!(
            line.contains('9'),
            "default sees the flow's write; got {line}"
        );
    }
}

// ── Dialect (#368) wasm-only error-path tests ─────────────────────────
//
// `set_dialect`'s rejection path constructs a `JsError`, which panics on a
// non-wasm target (`wasm-bindgen`'s "cannot call wasm-bindgen imported
// functions on non-wasm targets") — the same constraint every other
// `Result<_, JsError>` method in this file has. Acceptance-path coverage
// lives in the native `mod tests` above; rejection coverage lives here.
#[cfg(all(test, target_arch = "wasm32"))]
mod dialect_wasm_tests {
    use super::EditorSession;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn set_dialect_rejects_invalid_json() {
        let mut s = EditorSession::new();
        assert!(s.set_dialect("{ not valid }").is_err());
    }

    #[wasm_bindgen_test]
    fn set_dialect_rejects_undeclared_chain_kind() {
        let mut s = EditorSession::new();
        // Corrupt the preset: reference a kind nothing declares.
        let json_str =
            serde_json::to_string(&brink_ir::DialogueDialect::default()).expect("serializes");
        let mut value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        value["chain"][0]["after"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::String("nonexistent".to_owned()));
        assert!(s.set_dialect(&value.to_string()).is_err());
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
}
