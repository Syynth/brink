use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult, ReplayRecorder};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::value_marshal::{js_to_value, value_to_js};

// ── Reentrancy guard ─────────────────────────────────────────────────

/// RAII guard for [`StoryRunner::busy`]. [`acquire`](Self::acquire) returns
/// `None` if a VM-stepping method is already running (a binding callback or a
/// concurrent driver re-entered), so the caller can warn + error cleanly
/// instead of hitting a `RefCell` double-borrow panic. Clears the flag on drop,
/// including on an early `?`/error return.
pub(crate) struct BusyGuard<'a>(&'a Cell<bool>);

impl<'a> BusyGuard<'a> {
    pub(crate) fn acquire(flag: &'a Cell<bool>) -> Option<Self> {
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
pub(crate) fn reentrant_error(method: &str) -> JsError {
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
pub(crate) struct JsHandler<'a> {
    pub(crate) bindings: &'a HashMap<String, js_sys::Function>,
    pub(crate) lenient: bool,
    /// Where an async binding's returned `Promise` is stashed so the JS wrapper
    /// can await it (see [`StoryRunner::take_pending_promise`]).
    pub(crate) pending: &'a RefCell<Option<js_sys::Promise>>,
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
pub(crate) struct RecordingReplayHandler<'a> {
    pub(crate) inner: JsHandler<'a>,
    pub(crate) recorder: RefCell<&'a mut ReplayRecorder>,
    pub(crate) replaying: bool,
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

// ── External-binding wasm tests ──────────────────────────────────────
//
// Exercise the ink↔JS external-binding boundary end-to-end through the real
// exported `StoryRunner` API. Gated to wasm32 so they build/run only under
// `wasm-pack test --node` and never affect host `cargo test`.
#[cfg(all(test, target_arch = "wasm32"))]
mod binding_wasm_tests {
    use crate::story_runner::StoryRunner;
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
