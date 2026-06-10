use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use brink_format::Value;
use brink_ide::session::IdeSession;
use brink_runtime::{ExternalFnHandler, ExternalResult, FastRng};
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
                .map(|d| diagnostic_to_js(d, source, "main.ink".to_owned()))
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
                    diagnostics = diags
                        .iter()
                        .map(|d| diagnostic_to_js(d, source, "main.ink".to_owned()))
                        .collect();
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
        let handler = JsHandler {
            bindings: &bindings,
            lenient: self.lenient_unbound.get(),
            pending: &self.pending_promise,
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

// ── EditorSession ───────────────────────────────────────────────────

/// Stateful IDE session for the web editor. Wraps `IdeSession` and exposes
/// all IDE queries as methods that return JSON strings.
/// A view context scopes the editor to a sub-region of a file.
/// When active, `update_source` splices the fragment into the full file
/// at `[start, end)`, and IDE responses adjust offsets relative to the view.
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

#[wasm_bindgen]
pub struct EditorSession {
    session: IdeSession,
    /// The active file path for IDE queries.
    active_path: String,
    /// Optional sub-file view context for focused editing.
    view: Option<ViewContext>,
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
        }
    }

    /// Update the active file's source text. Reparses, lowers, and analyzes.
    ///
    /// When a view context is active, `source` is treated as a fragment that
    /// gets spliced into the full file at `[view.start, view.end)`.
    pub fn update_source(&mut self, source: &str) {
        if let Some(ref mut view) = self.view {
            let full = self
                .session
                .file_id(&self.active_path)
                .and_then(|id| self.session.source(id).map(str::to_owned))
                .unwrap_or_default();
            let start = view.start as usize;
            let end = (view.end as usize).min(full.len());

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
            // view.end tracks only the fragment, NOT the separator.
            // The separator lives at full[view.end] and is preserved across splices.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink files are always < 4GB"
            )]
            {
                view.end = view.start + source.len() as u32;
            }
            self.session.update_and_analyze(&self.active_path, spliced);
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
        // Boundary offsets are UTF-16 code units; convert to bytes for the
        // internal byte-indexed logic below (and stored ViewContext range).
        let (start, end) = match self.active_source() {
            Some(s) => (utf16_to_byte(s, start), utf16_to_byte(s, end)),
            None => (start, end),
        };
        // Check if there's a newline right at `end` (the separator between this
        // section and the next). If so, we'll ensure it's preserved after splices.
        // Trim trailing blank lines from the view range and check if there's a
        // newline separator at the boundary that should be preserved across splices.
        let (end, trailing_newline) = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))
            .map_or((end, false), |s| {
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
                        && s.as_bytes().get((trimmed_end as usize).wrapping_sub(1))
                            == Some(&b'\n'));
                (trimmed_end, has_nl)
            });

        let start_line = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))
            .map_or(0, |s| {
                let byte_start = (start as usize).min(s.len());
                count_newlines(&s[..byte_start])
            });
        self.view = Some(ViewContext {
            start,
            end,
            start_line,
            trailing_newline,
        });
    }

    /// Clear the view context, returning to full-file mode.
    pub fn clear_view_context(&mut self) {
        self.view = None;
    }

    /// Get the source text for the current view. Returns the fragment if a view
    /// context is active, or the full file otherwise. Returns a JSON string.
    pub fn get_view_source(&self) -> String {
        let source = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id));
        match (source, &self.view) {
            (Some(s), Some(v)) => {
                let start = (v.start as usize).min(s.len());
                let end = (v.end as usize).min(s.len());
                serde_json::to_string(&s[start..end]).unwrap_or_default()
            }
            (Some(s), None) => serde_json::to_string(s).unwrap_or_default(),
            _ => "null".to_owned(),
        }
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
        // file-relative) and attach that file's path, so an INCLUDEd file's
        // error lands on the right tab instead of collapsing onto the entry.
        let to_js = |d: &brink_ir::Diagnostic| {
            let src = session.source(d.file).unwrap_or("");
            let file = session.file_path(d.file).unwrap_or_default().to_owned();
            diagnostic_to_js(d, src, file)
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

    /// Compute per-line context from the HIR. Returns JSON array of `LineContext`.
    pub fn line_contexts(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source), Some(root)) = (
            self.session.hir(file_id),
            self.session.source(file_id),
            self.session.syntax_root(file_id),
        ) else {
            return "[]".to_owned();
        };

        let contexts = brink_ide::line_context::line_contexts(hir, source, &root);
        if let Some(v) = &self.view {
            let start = v.start_line as usize;
            let end_line = self.view_end_line().map_or(contexts.len(), |l| l as usize);
            let slice = &contexts[start..end_line.min(contexts.len())];
            serde_json::to_string(slice).unwrap_or_default()
        } else {
            serde_json::to_string(&contexts).unwrap_or_default()
        }
    }

    /// Compute semantic tokens. Returns JSON array of tokens.
    pub fn semantic_tokens(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
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
                let line = self.to_relative_line(t.line)?;
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

    /// Compute completions at the given byte offset. Returns JSON array.
    pub fn completions(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(offset);
        let ctx = brink_ide::detect_completion_context(source, abs_offset as usize);
        let scope = brink_ide::cursor_scope(source, abs_offset as usize);

        let items: Vec<CompletionItemJs> = analysis
            .index
            .symbols
            .values()
            .filter(|info| brink_ide::is_visible_in_context(&ctx, info, &scope))
            .map(|info| CompletionItemJs {
                name: info.name.clone(),
                kind: symbol_kind_str(info.kind).to_owned(),
                // Callables get a typed signature from /// docs or the host
                // manifest, if any; otherwise the kind-derived detail.
                detail: typed_detail(analysis, info).or_else(|| info.detail.clone()),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compute hover info at the given byte offset. Returns JSON or "null".
    pub fn hover(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let project_files = [(file_id, self.active_path.clone(), source.to_owned())];

        let abs_offset = self.to_absolute(offset);
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
                    start: info.range.and_then(|r| self.to_relative(r.start().into())),
                    end: info.range.and_then(|r| self.to_relative(r.end().into())),
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    /// Compute goto-definition at the given byte offset. Returns JSON or "null".
    pub fn goto_definition(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(offset);
        match brink_ide::navigation::goto_definition(analysis, file_id, TextSize::new(abs_offset)) {
            Some(loc) => {
                let db = self.session.db();
                let file_path = db.file_path(loc.file).unwrap_or_default().to_owned();
                let (start, end) = if loc.file == file_id {
                    // Same file: adjust to view-relative UTF-16 offsets
                    (
                        self.to_relative(loc.range.start().into())
                            .unwrap_or(loc.range.start().into()),
                        self.to_relative(loc.range.end().into())
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

    /// Find all references. Returns JSON array.
    pub fn find_references(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(offset);
        let refs = brink_ide::navigation::find_references(
            analysis,
            file_id,
            TextSize::new(abs_offset),
            true,
        );

        let db = self.session.db();
        let items: Vec<LocationJs> = refs
            .iter()
            .filter_map(|loc| {
                if loc.file == file_id {
                    // Same file: adjust offsets, filter out-of-view
                    let start = self.to_relative(loc.range.start().into())?;
                    let end = self.to_relative(loc.range.end().into())?;
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

    /// Check if rename is possible. Returns JSON or "null".
    pub fn prepare_rename(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(offset);
        match brink_ide::rename::prepare_rename(analysis, file_id, TextSize::new(abs_offset)) {
            Some(range) => {
                let start = self.to_relative(range.start().into());
                let end = self.to_relative(range.end().into());
                match (start, end) {
                    (Some(s), Some(e)) => {
                        let js = LocationJs {
                            file: self.active_path.clone(),
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

    /// Compute rename edits. Returns JSON array or "null".
    pub fn rename(&self, offset: u32, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(offset);
        match brink_ide::rename::rename(analysis, file_id, TextSize::new(abs_offset), new_name) {
            Some(result) => {
                let edits: Vec<FileEditJs> = result
                    .edits
                    .iter()
                    .filter_map(|e| {
                        let start = self.to_relative(e.range.start().into())?;
                        let end = self.to_relative(e.range.end().into())?;
                        Some(FileEditJs {
                            start,
                            end,
                            new_text: e.new_text.clone(),
                        })
                    })
                    .collect();
                serde_json::to_string(&edits).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    /// Compute code actions. Returns JSON array.
    pub fn code_actions(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(offset);
        let actions = brink_ide::code_actions::code_actions(source, abs_offset as usize);

        let items: Vec<CodeActionJs> = actions
            .iter()
            .map(|a| CodeActionJs {
                title: a.title.clone(),
                kind: code_action_kind_str(&a.kind).to_owned(),
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compute inlay hints. Returns JSON array.
    pub fn inlay_hints(&self, start: u32, end: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(start);
        let abs_end = self.to_absolute(end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::inlay_hints::inlay_hints(&root, analysis, range);

        let items: Vec<InlayHintJs> = hints
            .iter()
            .filter_map(|h| {
                let offset = self.to_relative(h.offset.into())?;
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

    /// Compute signature help. Returns JSON or "null".
    pub fn signature_help(&self, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(offset);
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

    /// Compute folding ranges. Returns JSON array.
    pub fn folding_ranges(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source)) = (self.session.hir(file_id), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        let ranges = brink_ide::folding::folding_ranges(hir, source);

        let items: Vec<FoldRangeJs> = ranges
            .iter()
            .filter_map(|r| {
                let start_line = self.to_relative_line(r.start_line)?;
                let end_line = self.to_relative_line(r.end_line)?;
                Some(FoldRangeJs {
                    start_line,
                    end_line,
                    collapsed_text: r.collapsed_text.clone(),
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Compute document symbols (outline). Returns JSON array.
    pub fn document_symbols(&self) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
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

    /// Convert a line element to a different type. Returns JSON text edit or "null".
    ///
    /// Target values: `"narrative"`, `"choice"`, `"sticky_choice"`, `"gather"`, `"choice_body"`.
    pub fn convert_element(&self, offset: u32, target: &str) -> String {
        let Some(file_id) = self.session.file_id(&self.active_path) else {
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

        let abs_offset = self.to_absolute(offset);
        match brink_ide::line_convert::convert_element(
            source,
            hir,
            &root,
            abs_offset,
            convert_target,
        ) {
            Some(edit) => match (self.to_relative(edit.from), self.to_relative(edit.to)) {
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
        let Some(file_id) = self.session.file_id(&self.active_path) else {
            return "\"\"".to_owned();
        };
        let Some(source) = self.session.source(file_id) else {
            return "\"\"".to_owned();
        };

        let formatted = brink_ide::sort_knots_in_source(source);
        serde_json::to_string(&formatted).unwrap_or_default()
    }

    /// Reorder a stitch within its parent knot. Returns JSON `MoveResult` or error string.
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

    /// Move a stitch from one knot to another. Returns JSON `MoveResult` or error.
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
            Ok(result) => move_result_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Promote a stitch to a top-level knot. Returns JSON `MoveResult` or error.
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
            Ok(result) => move_result_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder a knot within the top-level knot list. Returns JSON `MoveResult` or error.
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
    /// which know the full destination order. Returns JSON `MoveResult` or error.
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
    /// names). Returns JSON `MoveResult` or error.
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

    /// Demote a top-level knot to a stitch inside another knot. Returns JSON `MoveResult` or error.
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
            Ok(result) => move_result_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }
}

// ── View context helpers (private, not wasm-exported) ───────────────

impl EditorSession {
    /// Source text of the active file, if loaded.
    fn active_source(&self) -> Option<&str> {
        self.session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))
    }

    /// Convert a UTF-16 view-relative offset (the boundary convention) to a
    /// file-absolute **byte** offset for `brink-ide`/rowan.
    ///
    /// When a view context is active the offset is relative to the displayed
    /// fragment (`source[view.start..view.end]`); otherwise it's relative to
    /// the whole file.
    fn to_absolute(&self, offset: u32) -> u32 {
        let Some(source) = self.active_source() else {
            return offset;
        };
        match self.view.as_ref() {
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
    fn to_relative(&self, offset: u32) -> Option<u32> {
        let source = self.active_source()?;
        match self.view.as_ref() {
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
    fn to_relative_line(&self, line: u32) -> Option<u32> {
        self.view.as_ref().map_or(Some(line), |v| {
            (line >= v.start_line).then(|| line - v.start_line)
        })
    }

    /// Compute the end line of the view in the current source.
    fn view_end_line(&self) -> Option<u32> {
        let v = self.view.as_ref()?;
        let source = self
            .session
            .file_id(&self.active_path)
            .and_then(|id| self.session.source(id))?;
        let byte_end = (v.end as usize).min(source.len());
        Some(count_newlines(&source[..byte_end]))
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

// ── Serialization types ─────────────────────────────────────────────

#[derive(Serialize)]
struct ProjectFileJs {
    path: String,
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

#[derive(Serialize)]
struct CompletionItemJs {
    name: String,
    kind: String,
    detail: Option<String>,
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
struct FileEditJs {
    start: u32,
    end: u32,
    new_text: String,
}

#[derive(Serialize)]
struct InlayHintJs {
    offset: u32,
    label: String,
    kind: String,
    padding_right: bool,
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
    }
}

/// Convert a compiler diagnostic to JSON, translating its byte range to UTF-16
/// offsets against `source` (the diagnostic's own file) and attaching `file`
/// (that file's path).
fn diagnostic_to_js(d: &brink_ir::Diagnostic, source: &str, file: String) -> DiagnosticJs {
    DiagnosticJs {
        message: d.message.clone(),
        start: byte_to_utf16(source, d.range.start().into()),
        end: byte_to_utf16(source, d.range.end().into()),
        severity: format!("{:?}", d.code.severity()),
        file,
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

// ── Structural move helpers ──────────────────────────────────────────

#[derive(Serialize)]
struct MoveResultJs {
    ok: bool,
    /// The file path this result applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_source: Option<String>,
    cross_file_edits: Vec<CrossFileEditJs>,
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

fn move_result_json(
    session: &IdeSession,
    result: brink_ide::structural_move::MoveResult,
    path: &str,
) -> String {
    // Group reference edits by file (BTreeMap for deterministic output).
    let mut by_file: std::collections::BTreeMap<u32, Vec<(usize, usize, String)>> =
        std::collections::BTreeMap::new();
    for e in &result.cross_file_edits {
        by_file.entry(e.file.0).or_default().push((
            usize::from(e.range.start()),
            usize::from(e.range.end()),
            e.new_text.clone(),
        ));
    }

    let db = session.db();
    let mut edits: Vec<CrossFileEditJs> = Vec::new();
    for (file_raw, file_edits) in by_file {
        let file_id = brink_ir::FileId(file_raw);
        let (Some(src), Some(fpath)) = (session.source(file_id), db.file_path(file_id)) else {
            continue;
        };
        // The primary file is already covered by `new_source`.
        if fpath == path {
            continue;
        }
        edits.push(CrossFileEditJs {
            path: fpath.to_owned(),
            new_source: apply_edits(src, file_edits),
        });
    }

    let resp = MoveResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: Some(result.new_source),
        cross_file_edits: edits,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

fn move_result_json_simple(new_source: String, path: &str) -> String {
    let resp = MoveResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: Some(new_source),
        cross_file_edits: Vec::new(),
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

fn error_json(msg: &str) -> String {
    let resp = MoveResultJs {
        ok: false,
        path: None,
        new_source: None,
        cross_file_edits: Vec::new(),
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
}
