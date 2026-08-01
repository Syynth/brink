use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use brink_format::Value;
use brink_runtime::FastRng;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::external_binding::{BusyGuard, JsHandler, reentrant_error};
use crate::value_marshal::{LineJs, js_to_value, line_to_js};

// ── Speculative evaluation (F4.3) ──────────────────────────────────────
//
// `WebSpeculation` wraps `brink_runtime::Speculation` — a sandboxed,
// side-effect-proof fork of the runner's current story state (see
// `StoryRunner::speculate`) — and exposes its verbs to JS. It is the
// composable primary surface celeris (and any other consumer) drives
// directly; `@brink-lang/web`'s `evaluate()` convenience composes these
// verbs into a single call for the common cases (a knot path or a
// literal-arg function call) rather than hiding them.
//
// Every VM-stepping verb (`advance`/`eval_function`/`resume_function_eval`)
// builds a fresh `JsHandler` (the runner's bound externals, snapshotted at
// `speculate()` time) wrapped in a `KindTieredHandler` (the caller-supplied
// `name -> "query"|"effect"` policy) — the same "assemble the handler stack
// per call" shape `StoryRunner`'s `continue_*` family already uses, since
// `JsHandler` borrows its bindings map for the call's duration. The runtime
// itself never sees the kind map; it is plain data threaded from JS through
// this wasm boundary, same as `KindTieredHandler`'s own module docs describe.

#[wasm_bindgen]
pub struct WebSpeculation {
    /// Kept alongside the `Speculation` (which owns its own `Arc` clone
    /// internally but doesn't expose it) so `Value` marshaling can resolve
    /// list-item and divert-target names for `eval_function`/
    /// `resume_function_eval` results.
    pub(crate) program: Arc<brink_runtime::Program>,
    pub(crate) speculation: RefCell<brink_runtime::Speculation<FastRng>>,
    pub(crate) budget: brink_runtime::Budget,
    /// Snapshot of the runner's bound externals at `speculate()` time —
    /// independent of any binding the host registers afterward, matching the
    /// "forked, self-contained, discarded" contract of a `Speculation`.
    pub(crate) bindings: RefCell<HashMap<String, js_sys::Function>>,
    pub(crate) lenient_unbound: bool,
    pub(crate) kinds: HashMap<String, brink_runtime::PolicyKind>,
    pub(crate) context: brink_runtime::EvalContext,
    pub(crate) live_effects: bool,
    pub(crate) pending_promise: RefCell<Option<js_sys::Promise>>,
    pub(crate) busy: Cell<bool>,
    /// Accumulated `KindTieredHandler` diagnostics across every verb call
    /// made on this speculation so far (each call builds its own handler,
    /// which starts its report empty — see `merge_report`).
    pub(crate) externals_live: RefCell<Vec<String>>,
    pub(crate) externals_fallback: RefCell<Vec<String>>,
}

#[wasm_bindgen]
impl WebSpeculation {
    /// Move this speculation's play head to a named knot/stitch path — the
    /// speculative equivalent of [`StoryRunner::go_to_path`]. Only this
    /// speculation's own sandboxed position moves.
    pub fn go_to_path(&self, path: &str) -> Result<(), JsError> {
        self.speculation
            .borrow_mut()
            .go_to_path(path)
            .map_err(|e| JsError::new(&format!("go_to_path error: {e}")))
    }

    /// Select a pending choice by index.
    pub fn choose(&self, index: usize) -> Result<(), JsError> {
        self.speculation
            .borrow_mut()
            .choose(index)
            .map_err(|e| JsError::new(&format!("choose error: {e}")))
    }

    /// Advance this speculation by one visible line, honoring its budget.
    /// Returns one `Line` JSON — possibly `{ "type": "awaiting_external",
    /// "name": … }` if a bound external returned a `Promise`: take it with
    /// [`take_pending_promise`](Self::take_pending_promise), await it,
    /// [`resolve_external`](Self::resolve_external), and call `advance`
    /// again. Mirrors [`StoryRunner::advance_one`].
    pub fn advance(&self) -> Result<String, JsError> {
        let _guard =
            BusyGuard::acquire(&self.busy).ok_or_else(|| reentrant_error("speculation advance"))?;
        let bindings = self.bindings.borrow();
        let js_handler = JsHandler {
            bindings: &bindings,
            lenient: self.lenient_unbound,
            pending: &self.pending_promise,
        };
        let tiered = brink_runtime::KindTieredHandler::new(
            &js_handler,
            self.kinds.clone(),
            self.context,
            self.live_effects,
        );
        let mut speculation = self.speculation.borrow_mut();
        let outcome = speculation
            .advance(self.budget, &tiered)
            .map_err(|e| JsError::new(&format!("speculation advance error: {e}")))?;
        self.merge_report(&tiered);

        let resp = match outcome {
            brink_runtime::SpeculationStep::Line(line) => line_to_js(line),
            brink_runtime::SpeculationStep::AwaitingExternal => LineJs {
                r#type: "awaiting_external",
                text: String::new(),
                tags: Vec::new(),
                choices: None,
                name: speculation.pending_external_name().map(str::to_owned),
            },
        };
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Resolve the external this speculation is parked on with a native JS
    /// value (the awaited result of an async binding's `Promise`). No-op if
    /// not awaiting.
    pub fn resolve_external(&self, value: &JsValue) {
        if self.busy.get() {
            web_sys::console::warn_1(&JsValue::from_str(
                "brink: reentrant 'speculation resolve_external' ignored",
            ));
            return;
        }
        let v = js_to_value(value);
        self.speculation.borrow_mut().resolve_external(v);
    }

    /// Take the `Promise` a suspended async binding returned, for the caller
    /// to `await`. `undefined` if none is pending. After awaiting, feed the
    /// result to [`resolve_external`](Self::resolve_external).
    pub fn take_pending_promise(&self) -> JsValue {
        if self.busy.get() {
            web_sys::console::warn_1(&JsValue::from_str(
                "brink: reentrant 'speculation take_pending_promise' ignored",
            ));
            return JsValue::UNDEFINED;
        }
        self.pending_promise
            .borrow_mut()
            .take()
            .map_or(JsValue::UNDEFINED, JsValue::from)
    }

    /// The ink-declared name of the external this speculation is paused on,
    /// if any.
    pub fn pending_external_name(&self) -> Option<String> {
        self.speculation
            .borrow()
            .pending_external_name()
            .map(str::to_owned)
    }

    /// Evaluate an ink function on this speculation, out-of-band: output is
    /// isolated and the transcript untouched (and, being on the sandboxed
    /// fork, can never reach the live story either way). Returns a
    /// `FunctionEval` JSON: `{ "type": "returned", "value": TypedValue }` or
    /// `{ "type": "awaiting_external", "name": string | null }` — resolve
    /// with [`resolve_external`](Self::resolve_external) and continue with
    /// [`resume_function_eval`](Self::resume_function_eval).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen passes a JS array as an owned Vec across the boundary"
    )]
    pub fn eval_function(&self, name: &str, args: Vec<JsValue>) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy)
            .ok_or_else(|| reentrant_error("speculation eval_function"))?;
        let ink_args: Vec<Value> = args.iter().map(js_to_value).collect();
        let bindings = self.bindings.borrow();
        let js_handler = JsHandler {
            bindings: &bindings,
            lenient: self.lenient_unbound,
            pending: &self.pending_promise,
        };
        let tiered = brink_runtime::KindTieredHandler::new(
            &js_handler,
            self.kinds.clone(),
            self.context,
            self.live_effects,
        );
        let mut speculation = self.speculation.borrow_mut();
        let outcome = speculation
            .eval_function(name, &ink_args, self.budget, &tiered)
            .map_err(|e| JsError::new(&format!("eval_function error: {e}")))?;
        self.merge_report(&tiered);
        let resp = self.function_eval_to_js(outcome, &speculation);
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Resume a function evaluation that paused on `AwaitingExternal`, after
    /// the pending external has been resolved via
    /// [`resolve_external`](Self::resolve_external). Same return shape as
    /// [`eval_function`](Self::eval_function).
    pub fn resume_function_eval(&self) -> Result<String, JsError> {
        let _guard = BusyGuard::acquire(&self.busy)
            .ok_or_else(|| reentrant_error("speculation resume_function_eval"))?;
        let bindings = self.bindings.borrow();
        let js_handler = JsHandler {
            bindings: &bindings,
            lenient: self.lenient_unbound,
            pending: &self.pending_promise,
        };
        let tiered = brink_runtime::KindTieredHandler::new(
            &js_handler,
            self.kinds.clone(),
            self.context,
            self.live_effects,
        );
        let mut speculation = self.speculation.borrow_mut();
        let outcome = speculation
            .resume_function_eval(self.budget, &tiered)
            .map_err(|e| JsError::new(&format!("resume_function_eval error: {e}")))?;
        self.merge_report(&tiered);
        let resp = self.function_eval_to_js(outcome, &speculation);
        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// This speculation's transcript so far, resolved against its own
    /// program and line tables — JSON array of `{ text, tags }`.
    pub fn transcript(&self) -> Result<String, JsError> {
        let speculation = self.speculation.borrow();
        let lines: Vec<TranscriptLineJs> = speculation
            .rendered_transcript()
            .into_iter()
            .map(|(text, tags)| TranscriptLineJs { text, tags })
            .collect();
        serde_json::to_string(&lines).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Which externals this speculation let through live versus fell back,
    /// across every verb call made on it so far — `{ "live": [...],
    /// "fallback": [...] }` (call order within each list, duplicates
    /// included). Diagnostic only.
    pub fn externals_report(&self) -> Result<String, JsError> {
        let report = ExternalsReportJs {
            live: self.externals_live.borrow().clone(),
            fallback: self.externals_fallback.borrow().clone(),
        };
        serde_json::to_string(&report).map_err(|e| JsError::new(&format!("json error: {e}")))
    }
}

impl WebSpeculation {
    /// Fold one call's `KindTieredHandler` diagnostics into this
    /// speculation's running `externals_report()` totals. Each verb call
    /// builds its own handler (it borrows the call-local `JsHandler`), so
    /// the accumulation has to happen here rather than by living inside a
    /// single handler for this speculation's whole lifetime.
    fn merge_report(&self, tiered: &brink_runtime::KindTieredHandler<'_>) {
        let report = tiered.report();
        self.externals_live.borrow_mut().extend(report.live);
        self.externals_fallback.borrow_mut().extend(report.fallback);
    }

    /// Map a `FunctionEval` outcome to its JSON mirror, resolving a returned
    /// value's list/divert names against this speculation's program.
    fn function_eval_to_js(
        &self,
        outcome: brink_runtime::FunctionEval,
        speculation: &brink_runtime::Speculation<FastRng>,
    ) -> FunctionEvalJs {
        match outcome {
            brink_runtime::FunctionEval::Returned(value) => FunctionEvalJs::Returned {
                value: value_to_typed_js(&value, &self.program),
            },
            brink_runtime::FunctionEval::AwaitingExternal => FunctionEvalJs::AwaitingExternal {
                name: speculation.pending_external_name().map(str::to_owned),
            },
        }
    }
}

/// Parsed `options_json` argument to [`StoryRunner::speculate`]. All fields
/// optional; see that method's doc for the JSON shape and defaults.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SpeculateOptionsJs {
    pub(crate) steps: Option<u64>,
    pub(crate) lines: Option<usize>,
    pub(crate) context: Option<String>,
    pub(crate) live_effects: bool,
    pub(crate) kinds: HashMap<String, String>,
}

#[derive(Serialize)]
struct TranscriptLineJs {
    text: String,
    tags: Vec<String>,
}

#[derive(Serialize)]
struct ExternalsReportJs {
    live: Vec<String>,
    fallback: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FunctionEvalJs {
    Returned { value: TypedValueJs },
    AwaitingExternal { name: Option<String> },
}

/// A richer, structured `Value` → JS marshaling than [`value_to_js`]'s
/// scalar-only external-binding boundary: used for `WebSpeculation`'s
/// `eval_function`/`resume_function_eval` results, where a list or divert
/// target is useful information rather than a binding argument to discard.
/// Tagged by `type`: `int`/`float`/`bool`/`string`/`null` carry a scalar
/// `value`; `list` carries display-resolved `items` (origin list name, item
/// name, ordinal); `divert` carries the resolved destination `path` (`null`
/// if it doesn't name a knot/stitch). `VariablePointer`/`TempPointer`/
/// `FragmentRef` are VM-internal and never expected here; they map to
/// `null` rather than erroring.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TypedValueJs {
    Int {
        value: i32,
    },
    Float {
        value: f32,
    },
    Bool {
        value: bool,
    },
    String {
        value: String,
    },
    Null,
    List {
        items: Vec<ListMemberJs>,
    },
    Divert {
        path: Option<String>,
    },
    /// A collection value ([`Value::Array`]) as a lossless, recursive tree of
    /// typed elements. The wasm boundary serializes trees rather than sharing
    /// Arc snapshots (value-model-spec §8) — sharing is not observable here.
    Array {
        items: Vec<TypedValueJs>,
    },
    /// A collection value ([`Value::Map`]) as an ordered list of typed
    /// key/value entries. Insertion order is preserved (the ratified §4 order),
    /// and each key retains its scalar type (`int`/`string`/`bool`) — the
    /// lossless counterpart to the native-JS-object mapping in [`value_to_js`].
    Map {
        entries: Vec<TypedMapEntryJs>,
    },
    /// A closed-shape record ([`Value::Record`], TM-4) as a shape id plus its
    /// flat field values, each recursively typed — same lossless-tree
    /// rationale as [`Array`](Self::Array)/[`Map`](Self::Map). Field *names*
    /// are not resolved here (that needs the compiled `StructShapes` table,
    /// not just the `Program` a binding return already carries); the shape
    /// id is enough to disambiguate two records with the same field count.
    Record {
        shape: u32,
        fields: Vec<TypedValueJs>,
    },
    /// A function value ([`Value::FnRef`]/[`Value::Closure`], T1c) as an
    /// **opaque token** (`docs/t1c-spec.md` §6 host boundary: "function values
    /// cross as opaque tokens `{DefinitionId, env}`; the host never
    /// dereferences the env"). `target` is the resolved knot/stitch path;
    /// `bound` carries the bound-arg prefix as a lossless typed tree so a
    /// speculation/eval result never silently drops a fn value. The
    /// host-side callback-invocation surface lands in T1c-3.
    Fn {
        target: Option<String>,
        bound: Vec<TypedValueJs>,
    },
    /// An opaque host-resource token ([`Value::Handle`], T1d,
    /// `docs/t1d-spec.md` §2/§6) — the lossless counterpart to
    /// [`value_to_js`](crate::value_marshal::value_to_js)'s native-object
    /// mapping. `kind` is the manifest-declared kind name, resolved from the
    /// program's name table (`"?"` for a stale `NameId` from a different
    /// compile — same convention as `Divert`'s unresolved-path case). `id`
    /// is the host-allocated token id, carried as a decimal string so a
    /// full-range `u64` never loses precision crossing to a JS `number`
    /// (`docs/value-model-spec.md` §8 native-leg lossiness class, the #667
    /// wildcard-arm hazard this arm exists to avoid).
    Handle {
        kind: String,
        id: String,
    },
    /// A symbolic path projection ([`Value::Projection`], T1e,
    /// `docs/t1e-spec.md` §3) as a lossless typed tree — the same rationale
    /// as [`Array`](Self::Array)/[`Map`](Self::Map): `root` is the resolved
    /// global var name (`None` for a stale/unresolvable cell — same
    /// convention as [`Divert`](Self::Divert)'s unresolved-path case),
    /// `segments` carries each snapshot segment typed (`Index` vs `Key`
    /// mirrors the wire's own two-kind encoding, `docs/format-v4-rfc.md`
    /// §1).
    Projection {
        root: Option<String>,
        segments: Vec<TypedProjSegmentJs>,
    },
    /// A typed-absence value ([`Value::OptionVal`], NS-A1,
    /// `docs/stdlib-spec.md` §1.4) as a lossless tree: `some` is `null` for
    /// `none`, or the recursively-typed inner value for `some(x)` — nesting
    /// (`some(none)`) is preserved, unlike the native-JS value-or-null
    /// mapping in [`value_to_js`](crate::value_marshal::value_to_js).
    Option {
        some: Option<Box<TypedValueJs>>,
    },
    /// An integer range value ([`Value::Range`], NS-A5,
    /// `docs/stdlib-spec.md` §7, F7) — the written form crosses losslessly:
    /// `start`/`end` are the authored bounds, `inclusive` distinguishes
    /// `..=` from `..` (content equality may identify `1..=6` and `1..7`;
    /// the boundary preserves the spelling).
    Range {
        start: i32,
        end: i32,
        inclusive: bool,
    },
    /// A numeric-tower value ([`Value::Vec2`] … [`Value::Mat4`], NS-A8,
    /// `docs/tower-mini-spec.md`) as its lossless lane form: `kind` is the
    /// tower type name (`"vec2"` … `"mat4"`), `lanes` the flat f32 lanes in
    /// the pinned wire order (vec/quat `x, y(, z, w)`; matrices
    /// column-major) — the same hand-serialized lane discipline as the
    /// `.inkb` wire (T5), never glam's memory layout.
    Tower {
        kind: String,
        lanes: Vec<f32>,
    },
    /// A weighted table ([`Value::Weighted`], NS-A7, `docs/stdlib-spec.md`
    /// §8) as its lossless entry form: `(weight, value)` pairs in
    /// construction order, values recursively typed.
    Weighted {
        entries: Vec<WeightedEntryJs>,
    },
}

/// One [`Value::Weighted`] entry, typed (NS-A7): a positive int weight and
/// its recursively-typed value.
#[derive(Serialize)]
struct WeightedEntryJs {
    weight: i32,
    value: Box<TypedValueJs>,
}

/// One [`Value::Projection`] path segment, typed (`docs/format-v4-rfc.md`
/// §1: `0=index i32, 1=key value`).
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TypedProjSegmentJs {
    Index { value: i32 },
    Key { value: Box<TypedValueJs> },
}

#[derive(Serialize)]
struct ListMemberJs {
    origin: String,
    name: String,
    ordinal: i32,
}

/// One `key: value` entry of a [`TypedValueJs::Map`], key type preserved.
#[derive(Serialize)]
struct TypedMapEntryJs {
    key: TypedMapKeyJs,
    value: TypedValueJs,
}

/// A [`Value::Map`] key rendered with its scalar type preserved, so the JSON
/// boundary is lossless (an `Int` key never collapses into a string key).
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TypedMapKeyJs {
    Int { value: i32 },
    String { value: String },
    Bool { value: bool },
}

/// Build the lossless tower form (NS-A8): kind name + flat lanes.
fn tower_typed_js(kind: &str, lanes: &[f32]) -> TypedValueJs {
    TypedValueJs::Tower {
        kind: kind.to_owned(),
        lanes: lanes.to_vec(),
    }
}

fn map_key_to_typed_js(key: &brink_format::MapKey) -> TypedMapKeyJs {
    match key {
        brink_format::MapKey::Int(n) => TypedMapKeyJs::Int { value: *n },
        brink_format::MapKey::Str(s) => TypedMapKeyJs::String {
            value: s.to_string(),
        },
        brink_format::MapKey::Bool(b) => TypedMapKeyJs::Bool { value: *b },
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one marshal arm per Value variant — the NS-A7 Weighted arm pushed this past 100"
)]
fn value_to_typed_js(v: &Value, program: &brink_runtime::Program) -> TypedValueJs {
    match v {
        Value::Int(i) => TypedValueJs::Int { value: *i },
        Value::Float(f) => TypedValueJs::Float { value: *f },
        Value::Bool(b) => TypedValueJs::Bool { value: *b },
        Value::String(s) => TypedValueJs::String {
            value: s.to_string(),
        },
        Value::List(list) => TypedValueJs::List {
            items: program
                .list_members(list)
                .into_iter()
                .map(|m| ListMemberJs {
                    origin: m.origin,
                    name: m.name,
                    ordinal: m.ordinal,
                })
                .collect(),
        },
        Value::DivertTarget(id) => TypedValueJs::Divert {
            path: program.divert_target_path(*id),
        },
        // Collections serialize as lossless recursive trees (value-model-spec
        // §8 host boundary). No opcode emits a collection yet, but a JS binding
        // *return* can already carry one through `js_to_value`, so this arm is
        // reachable and JS-observable — hence the `@brink-lang/web` changeset.
        Value::Array(items) => TypedValueJs::Array {
            items: items
                .iter()
                .map(|item| value_to_typed_js(item, program))
                .collect(),
        },
        Value::Map(map) => TypedValueJs::Map {
            entries: map
                .iter()
                .map(|(k, val)| TypedMapEntryJs {
                    key: map_key_to_typed_js(k),
                    value: value_to_typed_js(val, program),
                })
                .collect(),
        },
        Value::Record { shape, fields } => TypedValueJs::Record {
            shape: shape.0,
            fields: fields
                .iter()
                .map(|f| value_to_typed_js(f, program))
                .collect(),
        },
        // Function values (T1c, spec §6): opaque token — target path plus the
        // bound-arg prefix as a lossless typed tree.
        Value::FnRef(id) => TypedValueJs::Fn {
            target: program.divert_target_path(*id),
            bound: Vec::new(),
        },
        Value::Closure(c) => TypedValueJs::Fn {
            target: program.divert_target_path(c.target),
            bound: c
                .env
                .iter()
                .map(|e| value_to_typed_js(&e.payload, program))
                .collect(),
        },
        // Handle values (T1d, spec §2/§6): opaque token, kind name resolved
        // where possible. Never falls to `Null` — the #667 wildcard-arm
        // hazard class this exhaustive match structurally rules out.
        Value::Handle { kind, id } => TypedValueJs::Handle {
            kind: program.name_checked(*kind).unwrap_or("?").to_owned(),
            id: id.to_string(),
        },
        // Projection values (T1e, spec §3): a lossless typed tree, root cell
        // resolved where possible (same "never silently drop" reasoning as
        // every other non-`Null` arm here).
        Value::Projection(p) => TypedValueJs::Projection {
            root: program.global_var_name(p.cell).map(ToOwned::to_owned),
            segments: p
                .segments
                .iter()
                .map(|seg| match seg {
                    brink_format::ProjSegment::Index(n) => TypedProjSegmentJs::Index { value: *n },
                    brink_format::ProjSegment::Key(v) => TypedProjSegmentJs::Key {
                        value: Box::new(value_to_typed_js(v, program)),
                    },
                })
                .collect(),
        },
        Value::OptionVal(inner) => TypedValueJs::Option {
            some: inner
                .as_deref()
                .map(|v| Box::new(value_to_typed_js(v, program))),
        },
        Value::Range {
            start,
            end,
            inclusive,
        } => TypedValueJs::Range {
            start: *start,
            end: *end,
            inclusive: *inclusive,
        },
        // Tower values (NS-A8): lossless kind + lane form, lanes through
        // glam's explicit array conversions (T5 — never memory layout).
        Value::Vec2(v) => tower_typed_js("vec2", &v.to_array()),
        Value::Vec3(v) => tower_typed_js("vec3", &v.to_array()),
        Value::Vec4(v) => tower_typed_js("vec4", &v.to_array()),
        Value::Quat(q) => tower_typed_js("quat", &q.to_array()),
        Value::Mat2(m) => tower_typed_js("mat2", &m.to_cols_array()),
        Value::Mat3(m) => tower_typed_js("mat3", &m.to_cols_array()),
        Value::Mat4(m) => tower_typed_js("mat4", &m.to_cols_array()),
        // Weighted tables (NS-A7): lossless entry form, construction order
        // preserved (order is semantic for display and the roll walk).
        Value::Weighted(w) => TypedValueJs::Weighted {
            entries: w
                .entries
                .iter()
                .map(|(weight, value)| WeightedEntryJs {
                    weight: *weight,
                    value: Box::new(value_to_typed_js(value, program)),
                })
                .collect(),
        },
        Value::Null
        | Value::VariablePointer(_)
        | Value::TempPointer { .. }
        | Value::FragmentRef(_) => TypedValueJs::Null,
    }
}

// ── Typed-value JSON boundary (T1a-3 / #525) ─────────────────────────
//
// `value_to_typed_js` is the lossless JSON marshaling used for `eval_function`
// results. It takes `&Value`/`&Program` (no `JsValue`), so it is plain-Rust
// testable on the host — no wasm32 needed. These tests lock the collection
// tree encoding: recursion, insertion order, and key-type preservation.
#[cfg(test)]
mod typed_value_tests {
    use super::value_to_typed_js;
    use brink_format::{MapKey, OrderedMap, Value};

    /// A trivial linked program — `value_to_typed_js` only consults the
    /// program for `List`/`Divert`/`Handle`, so any program suffices for the
    /// collection arms.
    fn program() -> brink_runtime::Program {
        let out =
            brink_compiler::compile("m.ink", |_p| Ok::<_, std::io::Error>("-> END\n".to_owned()))
                .expect("compile");
        let (program, _tables) = brink_runtime::link(&out.data).expect("link");
        program
    }

    fn typed_json(v: &Value) -> serde_json::Value {
        let p = program();
        let s = serde_json::to_string(&value_to_typed_js(v, &p)).expect("serialize typed value");
        serde_json::from_str(&s).expect("valid json")
    }

    #[test]
    fn array_is_recursive_typed_tree() {
        let v = Value::array(vec![Value::Int(1), Value::from("x"), Value::Bool(true)]);
        let j = typed_json(&v);
        assert_eq!(j["type"], "array");
        assert_eq!(j["items"][0]["type"], "int");
        assert_eq!(j["items"][0]["value"], 1);
        assert_eq!(j["items"][1]["type"], "string");
        assert_eq!(j["items"][1]["value"], "x");
        assert_eq!(j["items"][2]["type"], "bool");
        assert_eq!(j["items"][2]["value"], true);
    }

    #[test]
    fn map_preserves_insertion_order_and_key_types() {
        let m: OrderedMap = [
            (MapKey::from("z"), Value::Int(1)),
            (MapKey::from(10), Value::from("ten")),
            (MapKey::from(true), Value::Bool(false)),
        ]
        .into_iter()
        .collect();
        let j = typed_json(&Value::map(m));
        assert_eq!(j["type"], "map");
        let entries = j["entries"].as_array().expect("entries array");
        assert_eq!(entries.len(), 3);
        // Insertion order preserved: z, 10, true.
        assert_eq!(entries[0]["key"]["type"], "string");
        assert_eq!(entries[0]["key"]["value"], "z");
        assert_eq!(entries[0]["value"]["value"], 1);
        assert_eq!(entries[1]["key"]["type"], "int");
        assert_eq!(entries[1]["key"]["value"], 10);
        assert_eq!(entries[1]["value"]["value"], "ten");
        assert_eq!(entries[2]["key"]["type"], "bool");
        assert_eq!(entries[2]["key"]["value"], true);
        assert_eq!(entries[2]["value"]["value"], false);
    }

    #[test]
    fn nested_collection_tree() {
        let inner: OrderedMap = [(MapKey::from("hp"), Value::Int(9))].into_iter().collect();
        let v = Value::array(vec![Value::map(inner), Value::array(vec![Value::Null])]);
        let j = typed_json(&v);
        assert_eq!(j["type"], "array");
        assert_eq!(j["items"][0]["type"], "map");
        assert_eq!(j["items"][0]["entries"][0]["key"]["value"], "hp");
        assert_eq!(j["items"][1]["type"], "array");
        assert_eq!(j["items"][1]["items"][0]["type"], "null");
    }

    // ── Handle (T1d, docs/t1d-spec.md §2/§6) ────────────────────────────────

    #[test]
    fn handle_marshals_as_opaque_token_not_null() {
        // Regression guard for the #667 wildcard-arm hazard class: a Handle
        // must never fall through to `TypedValueJs::Null`.
        let j = typed_json(&Value::handle(brink_format::NameId(0), 42));
        assert_eq!(j["type"], "handle");
        assert_eq!(j["id"], "42");
    }

    #[test]
    fn handle_id_crosses_as_a_string_not_a_lossy_f64_number() {
        // u64::MAX has no exact f64 representation; the id must cross as a
        // decimal string (JSON `"18446744073709551615"`), not a `number`
        // (which would silently round).
        let j = typed_json(&Value::handle(brink_format::NameId(0), u64::MAX));
        assert_eq!(j["id"], u64::MAX.to_string());
        assert!(
            j["id"].is_string(),
            "id must be a JSON string, not a number"
        );
    }

    #[test]
    fn handle_kind_resolves_to_question_mark_when_unresolvable() {
        // A NameId with no entry in this program's name table (the trivial
        // "-> END" program has an empty/near-empty table) renders "?" — same
        // convention as `display_fn_value`'s unresolvable-name fallback.
        let j = typed_json(&Value::handle(brink_format::NameId(u16::MAX), 1));
        assert_eq!(j["kind"], "?");
    }
}

// ── Speculative evaluation wasm tests (F4.3) ──────────────────────────
//
// Exercise `StoryRunner::speculate`/`WebSpeculation` end-to-end through the
// real exported API: the sandboxed fork produces correct output, never
// mutates the runner it was forked from, marshals list/divert-target values
// structurally, and gates externals by the caller-supplied kind map exactly
// as `KindTieredHandler` documents. Gated to wasm32, like the sibling
// `binding_wasm_tests` module above.
#[cfg(all(test, target_arch = "wasm32"))]
mod speculation_wasm_tests {
    use crate::story_runner::StoryRunner;
    use js_sys::Function;
    use wasm_bindgen_test::wasm_bindgen_test;

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

    const SRC: &str = "LIST Weekday = Monday, Tuesday, Wednesday\n\
VAR day = Weekday.Monday\n\
\n\
EXTERNAL get_query()\n\
EXTERNAL do_effect()\n\
\n\
=== function make_list() ===\n\
~ return day + Weekday.Tuesday\n\
\n\
=== function get_target() ===\n\
~ return -> intro\n\
\n\
=== function call_query() ===\n\
~ return get_query()\n\
\n\
=== function call_effect() ===\n\
~ return do_effect()\n\
\n\
=== intro ===\n\
Hello from intro.\n\
-> END\n\
\n\
-> END\n";

    #[wasm_bindgen_test]
    fn advance_resolves_text_and_never_mutates_live_story() {
        let r = runner(SRC);
        let spec = r.speculate("{}").ok().expect("speculate");
        spec.go_to_path("intro").ok().expect("go_to_path");
        let line = spec.advance().ok().expect("advance");
        assert!(
            line.contains("Hello from intro"),
            "speculation reaches intro's text; got {line}"
        );
        // The live story's own position is untouched: continuing it still
        // starts from the top, not from `intro`.
        let live = r.continue_story().ok().expect("live continue");
        assert!(
            !live.contains("Hello from intro"),
            "the live story was not diverted by the speculation; got {live}"
        );
    }

    #[wasm_bindgen_test]
    fn eval_function_marshals_list_value() {
        let r = runner(SRC);
        let spec = r.speculate("{}").ok().expect("speculate");
        let json = spec
            .eval_function("make_list", Vec::new())
            .ok()
            .expect("eval_function");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["type"], "returned");
        let typed = &value["value"];
        assert_eq!(typed["type"], "list");
        let items = typed["items"].as_array().expect("items array");
        let names: Vec<&str> = items
            .iter()
            .map(|it| it["name"].as_str().expect("name"))
            .collect();
        assert_eq!(
            names,
            vec!["Monday", "Tuesday"],
            "list members resolved by name, sorted by ordinal"
        );
        assert_eq!(items[0]["origin"], "Weekday");
    }

    #[wasm_bindgen_test]
    fn eval_function_marshals_divert_target() {
        let r = runner(SRC);
        let spec = r.speculate("{}").ok().expect("speculate");
        let json = spec
            .eval_function("get_target", Vec::new())
            .ok()
            .expect("eval_function");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let typed = &value["value"];
        assert_eq!(typed["type"], "divert");
        assert_eq!(typed["path"], "intro");
    }

    #[wasm_bindgen_test]
    fn query_kind_runs_live_effect_kind_falls_back() {
        let r = runner(SRC);
        r.bind_external("get_query", Function::new_no_args("return 99"));
        r.bind_external("do_effect", Function::new_no_args("return 1"));

        // "query" tiering: the live JS binding's value comes through.
        let spec = r
            .speculate(r#"{"kinds":{"get_query":"query"}}"#)
            .ok()
            .expect("speculate query");
        let json = spec
            .eval_function("call_query", Vec::new())
            .ok()
            .expect("eval_function query");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["value"]["type"], "int");
        assert_eq!(value["value"]["value"], 99);

        // Unclassified (default "effect") under the default "watch" context:
        // gated to Fallback, and `do_effect` has no ink fallback body, so the
        // call errors rather than running the bound JS function live.
        let spec2 = r.speculate("{}").ok().expect("speculate default");
        assert!(
            spec2.eval_function("call_effect", Vec::new()).is_err(),
            "an unclassified external under watch never runs live"
        );
    }

    #[wasm_bindgen_test]
    fn transcript_and_externals_report_reflect_the_run() {
        let r = runner(SRC);
        r.bind_external("get_query", Function::new_no_args("return 1"));
        let spec = r
            .speculate(r#"{"kinds":{"get_query":"query"}}"#)
            .ok()
            .expect("speculate");
        spec.go_to_path("intro").ok().expect("go_to_path");
        let _ = spec.advance().ok().expect("advance");
        let transcript_json = spec.transcript().ok().expect("transcript");
        let transcript: serde_json::Value =
            serde_json::from_str(&transcript_json).expect("valid json");
        assert!(
            transcript[0]["text"]
                .as_str()
                .unwrap_or_default()
                .contains("Hello from intro"),
            "transcript resolves the same text advance() produced; got {transcript_json}"
        );

        let _ = spec
            .eval_function("call_query", Vec::new())
            .ok()
            .expect("eval");
        let report_json = spec.externals_report().ok().expect("externals_report");
        let report: serde_json::Value = serde_json::from_str(&report_json).expect("valid json");
        assert_eq!(report["live"], serde_json::json!(["get_query"]));
        assert!(report["fallback"].as_array().unwrap().is_empty());
    }
}
